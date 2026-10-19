//! Synchronous JavaScript execution. Engine values never cross this boundary.
//!
//! A session owns one heap and canonical runtime. Reusable hosts create fresh
//! sessions; interactive consumers may deliberately retain a session instead.
use crate::compiler_data::{Datum, Form};
use crate::form_bridge::ConversionLimits;
#[cfg(light_quickjs)]
use crate::form_bridge::{BridgeState, FORM_BRIDGE_VERSION};
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
use host_clock::Instant;
#[cfg(light_quickjs)]
use rquickjs::allocator::{Allocator, RustAllocator};
#[cfg(light_quickjs)]
use rquickjs::{Array, CatchResultExt, Context, Ctx, Function, Object, Runtime};
use std::cell::RefCell;
#[cfg(light_quickjs)]
use std::sync::Weak;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
use std::time::Instant;
#[cfg(light_quickjs)]
use std::{cell::Cell, rc::Rc};
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
    time::Duration,
};

/// The session clock on wasm32-unknown-unknown, where `std::time::Instant`
/// is unavailable: a host-supplied clock, microseconds since the Unix epoch,
/// which the embedder defines as `__rquickjs_host_now_us` (named for the
/// QuickJS wasm shim, `vendor/rquickjs-sys/wasm-shim/shim.c`, which imported
/// it too while wasm builds embedded QuickJS).
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod host_clock {
    use std::time::Duration;

    unsafe extern "C" {
        fn __rquickjs_host_now_us() -> f64;
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Instant(f64);

    impl Instant {
        pub fn now() -> Self {
            // SAFETY: a pure host import with no arguments; the embedder links
            // it or the module fails to instantiate.
            Self(unsafe { __rquickjs_host_now_us() })
        }

        pub fn elapsed(&self) -> Duration {
            Duration::from_micros((Self::now().0 - self.0).max(0.0) as u64)
        }
    }
}

/// The shared runtime factory: `src/js_runtime.js` without its unit manifest
/// headers (see [`crate::runtime_manifest`]), with the hooks only the
/// compiler uses ([`crate::runtime_manifest::COMPILER_RUNTIME`]).
pub const RUNTIME_SOURCE: &str = crate::runtime_manifest::COMPILER_RUNTIME;
pub(crate) static NEXT_SESSION: AtomicU64 = AtomicU64::new(1);

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
    /// How many more [`ExecutionControl::check`]s pass before one cancels
    /// ([`ExecutionControl::cancel_after_checks`]).
    checks_left: AtomicU64,
    /// The control of the build this one's isolate belongs to, whose
    /// cancellation and deadline apply to this one too, and transitively
    /// its own build's ([`ExecutionControl::linked`]).
    build: Option<ExecutionControl>,
}
/// Clones share sticky cancellation, deadline and fuel across nested Rust work.
#[derive(Clone)]
pub struct ExecutionControl(Arc<ControlState>);
impl ExecutionControl {
    /// A control whose deadline and poll budget, from `options`, start now.
    pub(crate) fn new(options: &EvaluatorOptions) -> Result<Self, EvalError> {
        Ok(Self(Arc::new(ControlState {
            cancelled: AtomicBool::new(false),
            terminal: AtomicU8::new(0),
            remaining: AtomicU64::new(options.poll_budget),
            clock_origin: Instant::now(),
            deadline_nanos: AtomicU64::new(
                u64::try_from(options.timeout.as_nanos())
                    .map_err(|_| EvalError("invalid session timeout".into()))?,
            ),
            checks_left: AtomicU64::new(u64::MAX),
            build: None,
        })))
    }
    /// A control with a poll budget of its own, from `options`, that is
    /// also cancelled when `build` is and ends at `build`'s deadline if not
    /// before: one isolate's of several that share a build's deadline and
    /// cancellation but count their own polls (docs/compiler-concurrency.md,
    /// "As built (light-cljs-341)").
    pub(crate) fn linked(
        options: &EvaluatorOptions,
        build: &ExecutionControl,
    ) -> Result<Self, EvalError> {
        let mut control = Self::new(options)?;
        Arc::get_mut(&mut control.0).expect("a new control").build = Some(build.clone());
        Ok(control)
    }
    /// Why the build this control is linked to has ended: 1 when it is
    /// cancelled, 2 when its deadline has passed, else 0.
    fn build_cause(&self) -> u8 {
        let mut build = self.0.build.as_ref();
        while let Some(control) = build {
            if control.0.cancelled.load(Ordering::Relaxed) {
                return 1;
            }
            if control.0.clock_origin.elapsed().as_nanos()
                >= u128::from(control.0.deadline_nanos.load(Ordering::Relaxed))
            {
                return 2;
            }
            build = control.0.build.as_ref();
        }
        0
    }
    /// Begin an operation: a new deadline and poll budget from `options`,
    /// unless the control is cancelled or terminal, which stays.
    pub(crate) fn begin(&self, options: &EvaluatorOptions) -> Result<(), EvalError> {
        if self.0.terminal.load(Ordering::Relaxed) != 0 || self.0.cancelled.load(Ordering::Relaxed)
        {
            return self.poll();
        }
        let deadline = self
            .0
            .clock_origin
            .elapsed()
            .as_nanos()
            .checked_add(options.timeout.as_nanos())
            .and_then(|n| u64::try_from(n).ok())
            .ok_or_else(|| EvalError("invalid operation timeout".into()))?;
        self.0.deadline_nanos.store(deadline, Ordering::Relaxed);
        self.0
            .remaining
            .store(options.poll_budget, Ordering::Relaxed);
        self.poll()
    }
    pub fn cancel(&self) {
        self.0.cancelled.store(true, Ordering::Relaxed);
    }
    /// Cancel at the check after the next `checks` [`Self::check`]s pass, as
    /// a cancellation arriving that far into compiler work would. Tests use
    /// it to stop work between two of its checks.
    #[doc(hidden)]
    pub fn cancel_after_checks(&self, checks: u64) {
        self.0.checks_left.store(checks, Ordering::Relaxed);
    }
    #[allow(deprecated)] // fetch_update supports the stable compiler used in CI.
    pub fn poll(&self) -> Result<(), EvalError> {
        let build = if self.0.build.is_some() {
            self.build_cause()
        } else {
            0
        };
        let cause = if self.0.cancelled.load(Ordering::Relaxed) {
            1
        } else if self.0.clock_origin.elapsed().as_nanos()
            >= u128::from(self.0.deadline_nanos.load(Ordering::Relaxed))
        {
            2
        } else if build != 0 {
            build
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
        self.status()
    }
    /// Whether work may go on, without spending the poll budget or reading
    /// the deadline: an error once the control is cancelled or terminal.
    /// Compiler work that runs no JavaScript, such as installing namespace
    /// artifacts, checks this; the deadline and budget bound engine work.
    #[allow(deprecated)] // fetch_update supports the stable compiler used in CI.
    pub fn check(&self) -> Result<(), EvalError> {
        if self
            .0
            .checks_left
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| n.checked_sub(1))
            .is_err()
        {
            self.cancel();
        }
        if self.0.cancelled.load(Ordering::Relaxed)
            || (self.0.build.is_some() && self.build_cause() == 1)
        {
            let _ = self
                .0
                .terminal
                .compare_exchange(0, 1, Ordering::Relaxed, Ordering::Relaxed);
        }
        self.status()
    }
    fn status(&self) -> Result<(), EvalError> {
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
    #[cfg(light_v8)]
    pub(crate) fn memory_exhausted(&self) {
        let _ = self
            .0
            .terminal
            .compare_exchange(0, 5, Ordering::Relaxed, Ordering::Relaxed);
    }
}

/// The macro memory limit of one build, which all its compile-time
/// isolates share (docs/compiler-concurrency.md, "As built
/// (light-cljs-341)"): each isolate is allowed [`Self::base`] bytes, and
/// grows past that only by taking from the headroom the others have not
/// taken, which it gives back when it is destroyed, so the isolates never
/// hold more than the limit together. An isolate that cannot take what it
/// needs fails; none takes more silently.
#[derive(Debug)]
#[cfg_attr(not(light_v8), allow(dead_code))]
pub(crate) struct MemoryPool {
    base: usize,
    headroom: std::sync::atomic::AtomicUsize,
}
#[cfg_attr(not(light_v8), allow(dead_code))]
impl MemoryPool {
    /// The pool of `limit` bytes for `isolates` isolates, each allowed a
    /// base of half its even share, at least 32 MiB and at most 256 MiB,
    /// but never more than the even share, so that what is left of the
    /// limit is headroom to grow into.
    pub(crate) fn new(limit: usize, isolates: usize) -> Arc<Self> {
        let isolates = isolates.max(1);
        let base = (limit / (2 * isolates))
            .clamp(32 << 20, 256 << 20)
            .min(limit / isolates);
        Arc::new(Self {
            base,
            headroom: std::sync::atomic::AtomicUsize::new(limit - base * isolates),
        })
    }
    /// The bytes each isolate is allowed before it takes headroom.
    pub(crate) fn base(&self) -> usize {
        self.base
    }
    /// Take up to `wanted` bytes of headroom, but nothing unless at least
    /// `needed` are left: what was taken.
    #[allow(deprecated)] // fetch_update supports the stable compiler used in CI.
    pub(crate) fn take(&self, wanted: usize, needed: usize) -> usize {
        let mut taken = 0;
        let _ = self
            .headroom
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |left| {
                taken = if left >= needed { left.min(wanted) } else { 0 };
                (taken > 0).then(|| left - taken)
            });
        taken
    }
    /// Give back `bytes` taken before.
    pub(crate) fn give(&self, bytes: usize) {
        self.headroom.fetch_add(bytes, Ordering::Relaxed);
    }
    /// The headroom no isolate has taken.
    #[cfg(test)]
    pub(crate) fn headroom(&self) -> usize {
        self.headroom.load(Ordering::Relaxed)
    }
}
/// What the compile-time isolates of one build with several unit slots
/// share: the build's control, whose deadline and cancellation every
/// isolate's own control follows; the workers' control, linked to it, which
/// the coordinator cancels when the build fails; and the memory pool.
#[derive(Clone)]
#[cfg_attr(not(light_v8), allow(dead_code))]
pub(crate) struct BuildResources {
    pub(crate) control: ExecutionControl,
    pub(crate) workers: ExecutionControl,
    pub(crate) memory: Arc<MemoryPool>,
}
impl BuildResources {
    /// The resources of a build with `options`, whose deadline starts now,
    /// for `isolates` isolates.
    pub(crate) fn new(options: &EvaluatorOptions, isolates: usize) -> Result<Self, EvalError> {
        let control = ExecutionControl::new(options)?;
        Ok(Self {
            workers: ExecutionControl::linked(options, &control)?,
            control,
            memory: MemoryPool::new(options.memory_limit, isolates),
        })
    }
}

/// Opaque identity, valid only in its originating session, until release/drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueId {
    pub(crate) session: u64,
    pub(crate) index: u64,
}

/// A build without QuickJS (WebAssembly, light-cljs-357) embeds no engine:
/// its embedder runs compiler macros through a
/// [`crate::host_evaluator::ScriptHost`]. Every constructor fails, so no
/// value of this type exists and the paths that would use one, such as
/// interactive sessions and compiler data readers, report [`no_quickjs`].
#[cfg(not(light_quickjs))]
pub enum Evaluator {}
/// Why a build without QuickJS cannot make an [`Evaluator`].
#[cfg(not(light_quickjs))]
pub(crate) fn no_quickjs() -> EvalError {
    EvalError(
        "this light-cljs build embeds no QuickJS: run compiler macros through a ScriptHost (MacroHost::set_script_host)"
            .into(),
    )
}
#[cfg(not(light_quickjs))]
impl Evaluator {
    pub fn new(_: EvaluatorOptions) -> Result<Self, EvalError> {
        Err(no_quickjs())
    }
    pub fn interactive(_: EvaluatorOptions) -> Result<Self, EvalError> {
        Err(no_quickjs())
    }
    pub(crate) fn interactive_with_control(
        _: EvaluatorOptions,
        _: ExecutionControl,
    ) -> Result<Self, EvalError> {
        Err(no_quickjs())
    }
    pub fn export_data(&self, _: ValueId, _: ConversionLimits) -> Result<Datum, EvalError> {
        match *self {}
    }
    pub fn evaluate_unit(&self, _: &str) -> Result<ValueId, EvalError> {
        match *self {}
    }
    pub fn release(&self, _: ValueId) -> Result<(), EvalError> {
        match *self {}
    }
    pub fn import_data(&self, _: &Datum, _: ConversionLimits) -> Result<ValueId, EvalError> {
        match *self {}
    }
    pub fn call(&self, _: ValueId, _: &[ValueId]) -> Result<ValueId, EvalError> {
        match *self {}
    }
}
#[cfg(not(light_quickjs))]
impl MacroEvaluator for Evaluator {
    fn control(&self) -> ExecutionControl {
        match *self {}
    }
    fn evaluate_unit(&self, _: &str) -> Result<ValueId, EvalError> {
        match *self {}
    }
    fn call(&self, _: ValueId, _: &[ValueId]) -> Result<ValueId, EvalError> {
        match *self {}
    }
    fn import_form(&self, _: &Form, _: ConversionLimits) -> Result<ValueId, EvalError> {
        match *self {}
    }
    fn import_data(&self, _: &Datum, _: ConversionLimits) -> Result<ValueId, EvalError> {
        match *self {}
    }
    fn export_form(
        &self,
        _: ValueId,
        _: &Form,
        _: crate::compiler_data::ExpansionOrigin,
        _: ConversionLimits,
    ) -> Result<Form, EvalError> {
        match *self {}
    }
    fn release(&self, _: ValueId) -> Result<(), EvalError> {
        match *self {}
    }
}
#[cfg(light_quickjs)]
pub struct Evaluator {
    // Drop context roots before runtime. No Persistent or Rust-owned JS edges.
    context: Option<Context>,
    runtime: Runtime,
    control: ExecutionControl,
    generation: u64,
    bridge: Rc<RefCell<BridgeState>>,
    preserve_language_errors: bool,
    /// Whether a promise was rejected with no handler since the last
    /// `unhandled_rejections`, which then has a record to take.
    rejected: Rc<Cell<bool>>,
    /// Counts the runtime in `LiveCounts::engines` until it is dropped.
    _live: crate::compile_stats::Live,
}
#[cfg(light_quickjs)]
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
        crate::generated_names::forget_installed_scopes();
        let control = shared.unwrap_or(ExecutionControl::new(&options)?);
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
            rejected: Rc::new(Cell::new(false)),
            _live: crate::compile_stats::Live::new(&crate::compile_stats::LIVE_ENGINES),
        };
        // QuickJS frees short-lived values by reference counting, so a WeakMap
        // entry costs it less than adding a private field: every runtime this
        // session loads keeps its payload stores in WeakMaps (light-cljs-292).
        session.enter(|ctx| ctx.eval::<(), _>(format!(r#"
            globalThis.lightCljsWeakMapStores = true;
            const __lightEvaluator = (() => {{
              const rt = {RUNTIME_SOURCE};
              const values = new Map(); let next = 1; let lastError = null;
              let hostDiagnostic = null;
              let heldError = null; let jobFailure = null;
              const rejections = new Map();
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
                importMacroForm: descriptor => save(bridge.decode(descriptor, null, true)),
                macroArguments: (form, nodes, depth) => {{
                  const [inputs, snapshots] = bridge.macroArguments(get(form), nodes, depth), ids = [];
                  try {{ for (const value of inputs) ids.push(save(value)); return [ids, snapshots]; }}
                  catch (error) {{ for (const id of ids) values.delete(id); throw error; }}
                }},
                importEnvironment: (descriptor, nodes, depth) => {{ const [value, snapshots, published] = bridge.decode(descriptor, [nodes, depth]); return [save(value), snapshots, published]; }},
                exportForm: (id, nodes, depth) => bridge.encode(get(id), nodes, depth),
                formServices: (one, all, nodes, depth) => rt.setMacroexpandServices({{macroexpand1: form => bridge.decode(one(bridge.encode(form, nodes, depth))), macroexpand: form => bridge.decode(all(bridge.encode(form, nodes, depth)))}}),
                bridgeVersion: bridge.version,
                checkBridge: frame => {{ bridge.check(); if (frame !== undefined) globalThis.__lightRootReadsChecked?.(frame); }},
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
                compilerFrontier: next => {{
                  lastError = null;
                  try {{
                    if (next == null) globalThis.__lightLeaveCompilerFrontier();
                    else globalThis.__lightEnterCompilerFrontier(next);
                    hostDiagnostic = null;
                  }} catch (error) {{ lastError = error; const message = exceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; }}
                }},
                gensymScope: (serial, prefix, retired) => bridge.gensymScope(serial, prefix, retired),
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
                // Promise jobs may evaluate, which clears lastError: hold it while
                // they run, then restore it, or report the first job exception.
                holdError: () => {{ heldError = {{value: lastError}}; jobFailure = null; }},
                jobError: error => {{ jobFailure ??= {{value: error}}; }},
                releaseError: keep => {{
                  const failure = keep ? null : jobFailure;
                  lastError = (failure ?? heldError).value; heldError = jobFailure = null;
                  if (failure !== null) {{ const message = exceptionMessage(failure.value); if (message !== undefined) throw new Error(message); throw failure.value; }}
                }},
                // The host's rejection tracker records a promise rejected with no
                // handler until one is attached; takeRejections forgets them all
                // and returns the reasons of up to `limit`, oldest first and
                // printed. With `raise` the first becomes the last exception;
                // otherwise the last exception stays, even when a printer evaluates.
                trackRejection: (promise, reason, handled) => {{
                  if (handled) rejections.delete(promise); else rejections.set(promise, reason);
                }},
                takeRejections: (limit, raise) => {{
                  const reasons = limit > 0 ? [...rejections.values()].slice(0, limit) : []; rejections.clear();
                  const last = raise && reasons.length > 0 ? reasons[0] : lastError;
                  const describe = reason => {{
                    try {{ return rt.core.printForHost(reason); }} catch {{}}
                    try {{ return Object.prototype.toString.call(reason); }} catch {{ return '<unprintable reason>'; }}
                  }};
                  try {{ return reasons.map(describe); }} finally {{ lastError = last; }}
                }},
                inputWindow: more => rt.core.compilerInputWindow(more),
                inputConsume: units => rt.core.compilerInputConsume(units),
                inputRecover: () => rt.core.compilerInputRecover(),
                inputDiscard: () => rt.core.compilerInputDiscard(),
                inputCompact: () => rt.core.compilerInputCompact(),
                print: id => {{ try {{ return rt.core.printForHost(get(id)); }} catch (error) {{ lastError = error; const message = rt.compilerExceptionMessage(error); if (message !== undefined) throw new Error(message); throw error; }} }},
                validate: id => {{ get(id); }},
                valueCount: () => values.size,
                release: id => {{ bridge.releaseMacroInput(get(id)); values.delete(id); }},
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
        self.control.begin(options)
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
                // While a trace records, the check names the frame the
                // entry ran in for its root reads (`compile_trace::frame`).
                match crate::compile_trace::frame() {
                    Some(frame) => ctx
                        .eval::<Object, _>("__lightEvaluator")?
                        .get::<_, Function>("checkBridge")?
                        .call::<_, ()>((frame,))?,
                    None => ctx.eval::<(), _>("__lightEvaluator.checkBridge()")?,
                }
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
            services: ctx
                .eval::<Object, _>("__lightEvaluator")
                .catch(&ctx)
                .map_err(|error| EvalError(error.to_string()))?,
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

    /// Run the heap's pending promise jobs (`.then` and `queueMicrotask`
    /// callbacks, and the rest of an async function after an `await`),
    /// including those they queue, until none is left. Each job costs a poll
    /// and runs under the current operation's deadline, poll budget and memory
    /// limit, so a job that loops, an endless chain of jobs and unbounded
    /// allocation end it as they end any evaluation. A `.then` reaction's
    /// throw rejects its promise. An exception that leaves a job (a
    /// `queueMicrotask` callback's) does not stop the others; the first is
    /// reported and becomes the last exception, unless `keep` keeps the one
    /// from before the jobs (a failed form's). Call it only at an outer
    /// boundary, never from a callback: it locks the runtime.
    pub(crate) fn run_jobs(&self, keep: bool) -> Result<(), EvalError> {
        if !self.runtime.is_job_pending() {
            return Ok(());
        }
        self.enter(|ctx| ctx.eval::<(), _>("__lightEvaluator.holdError()"))?;
        // Budget the jobs' engine stack from this frame, as `enter` does;
        // rquickjs refreshes it only with its parallel feature.
        unsafe {
            rquickjs::qjs::JS_UpdateStackTop(rquickjs::qjs::JS_GetRuntime(
                self.context.as_ref().unwrap().as_raw().as_ptr(),
            ));
        }
        loop {
            self.control.poll()?;
            match self.runtime.execute_pending_job() {
                Ok(true) => {}
                Ok(false) => break,
                Err(exception) => exception.0.with(|ctx| {
                    // Take the exception from the job's context, so that it cannot
                    // surface in a later operation. An interrupt for a resource
                    // limit reports as that limit.
                    let error = ctx.catch();
                    self.control.poll()?;
                    if !self.preserve_language_errors {
                        self.control.invalidate();
                        return self.control.poll();
                    }
                    ctx.eval::<Function, _>("__lightEvaluator.jobError")
                        .and_then(|record| record.call::<_, ()>((error,)))
                        .catch(&ctx)
                        .map_err(|error| EvalError(error.to_string()))
                })?,
            }
        }
        self.enter(|ctx| {
            let release: Function = ctx.eval("__lightEvaluator.releaseError")?;
            release.call((keep,))
        })
    }
    /// Track promise rejections for `unhandled_rejections`: the engine's
    /// tracker records a promise rejected with no handler in
    /// `__lightEvaluator`, and drops it when a handler is attached first. The
    /// tracker holds no engine value; a record it cannot make (an interrupt
    /// for a limit, which ends the operation at its next poll) is dropped.
    pub(crate) fn track_rejections(&self) {
        let rejected = self.rejected.clone();
        self.runtime
            .set_host_promise_rejection_tracker(Some(Box::new(
                move |ctx, promise, reason, handled| {
                    // Nothing is recorded since the last take: no promise to drop.
                    if handled && !rejected.get() {
                        return;
                    }
                    rejected.set(true);
                    let _ = ctx
                        .eval::<Function, _>("__lightEvaluator.trackRejection")
                        .and_then(|track| track.call::<_, ()>((promise, reason, handled)))
                        .catch(&ctx);
                },
            )));
    }
    /// Forget the promises rejected with no handler attached since the last
    /// call (see `track_rejections`), returning the printed reasons of up to
    /// `limit` of them, oldest first; with `raise` the first reason becomes
    /// the last exception. Call it at an outer boundary, after `run_jobs`.
    pub(crate) fn unhandled_rejections(
        &self,
        limit: u32,
        raise: bool,
    ) -> Result<Vec<String>, EvalError> {
        if !self.rejected.replace(false) {
            return Ok(vec![]);
        }
        self.enter(|ctx| {
            let take: Function = ctx.eval("__lightEvaluator.takeRejections")?;
            take.call((f64::from(limit), raise))
        })
    }
    /// Force collection at an outer boundary. Callbacks must use the active Ctx,
    /// never this runtime-locking method.
    pub fn collect_garbage(&self) {
        self.runtime.run_gc();
    }

    /// The values the private registry roots for their `ValueId`s, each
    /// until it is released or the engine is dropped.
    pub fn value_roots(&self) -> Result<usize, EvalError> {
        self.enter(|ctx| ctx.eval::<f64, _>("__lightEvaluator.valueCount()"))
            .map(|count| count as usize)
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
                let host = QuickHost {
                    active: ActiveEvaluator {
                        services: ctx.eval("__lightEvaluator")?,
                        ctx: ctx.clone(),
                        control: &control,
                        generation,
                        bridge: &bridge,
                        preserve_language_errors: true,
                    },
                    unit_exception: RefCell::new(None),
                };
                let call = HostCall::new(&host);
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
                    Ok(result) => result.and_then(|id| HostScope::check(&host, id)),
                    Err(_) => {
                        control.invalidate();
                        Err(EvalError(format!("{name}: host callback panicked")))
                    }
                }
                .and_then(|index| {
                    control.poll()?;
                    Ok(index)
                });
                let owned = call.into_owned();
                match result {
                    Ok(index) => Ok(std::iter::once(index)
                        .chain(owned.into_iter().map(|id| id.index))
                        .collect::<Vec<_>>()),
                    Err(error) => {
                        for id in owned {
                            let _ = host.active.release(id);
                        }
                        if let Some(exception) = host.unit_exception.into_inner() {
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
pub(crate) fn compiler_root_source(
    name: &str,
    unit: Option<usize>,
    once: bool,
    location: &str,
    test: Option<usize>,
) -> String {
    let escape = |text: &str| {
        text.replace('\\', "\\\\")
            .replace('"', "\\\"")
            .replace('\n', "\\n")
            .replace('\r', "\\r")
    };
    format!(
        "rt => globalThis.__lightRegisterUnit(\"{}\",{}, {once}, \"{}\", {})",
        escape(name),
        unit.map_or("null".into(), |n| n.to_string()),
        escape(location),
        test.map_or("null".into(), |n| n.to_string())
    )
}
/// Common macro operations at either an outer boundary or an active callback.
pub trait MacroEvaluator {
    fn control(&self) -> ExecutionControl;
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError>;
    fn register_compiler_root(
        &self,
        name: &str,
        unit: Option<usize>,
        once: bool,
        location: &str,
        test: Option<usize>,
    ) -> Result<(), EvalError> {
        let value = self.evaluate_unit(&compiler_root_source(name, unit, once, location, test))?;
        self.release(value)
    }
    /// Execute all namespace registrations in order. Engines may cache the
    /// compiled chunks, but must execute every registration on every call.
    fn evaluate_registration(
        &self,
        source: &str,
        _chunks: &[(String, std::ops::Range<usize>)],
    ) -> Result<(), EvalError> {
        let value = self.evaluate_unit(&format!("rt => {{ {source} return null; }}"))?;
        self.release(value)
    }
    /// Enter a source declaration view, or restore its caller's view with None.
    /// The default preserves compatibility with external evaluator adapters.
    fn compiler_frontier(&self, next: Option<usize>) -> Result<(), EvalError> {
        let source = match next {
            Some(next) => format!("rt => globalThis.__lightEnterCompilerFrontier({next})"),
            None => "rt => globalThis.__lightLeaveCompilerFrontier()".into(),
        };
        let value = self.evaluate_unit(&source)?;
        self.release(value)
    }
    /// What tells this evaluator's heap apart from every other living one,
    /// the same for every handle to it, or 0 when the evaluator does not say
    /// ([`crate::generated_names::sync_gensym_scope`]).
    fn heap(&self) -> usize {
        0
    }
    /// Make the compile-time runtime's `gensym` number in the scope of
    /// generated names `serial`, appending `prefix` and the scope's own
    /// count, or with its counter for serial 0
    /// ([`crate::generated_names::Scope`]).
    fn gensym_scope(&self, serial: u64, prefix: &str, retired: &str) -> Result<(), EvalError> {
        debug_assert!(prefix.bytes().all(|b| b.is_ascii_digit() || b == b'_'));
        let value = self.evaluate_unit(&format!(
            "rt => {{ rt.compilerForms.gensymScope({serial}, \"{prefix}\", \"{retired}\"); return null; }}"
        ))?;
        self.release(value)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError>;
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError>;
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError>;
    /// Begin a call input batch. Native evaluators keep its decoded graph until
    /// arguments are prepared or the input is released; adapters may use the
    /// ordinary independent-import path below.
    fn import_macro_form(
        &self,
        form: &Form,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.import_form(form, limits)
    }
    /// Prepare arguments from the matching call after importing its environment.
    /// Returned IDs are owned by the caller, including when a later call fails.
    fn macro_arguments(
        &self,
        _form: ValueId,
        call: &Form,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        let mut values = Vec::new();
        for argument in call.children().iter().skip(1) {
            match self.import_form(argument, limits) {
                Ok(value) => values.push(value),
                Err(error) => {
                    for value in values {
                        let _ = self.release(value);
                    }
                    return Err(error);
                }
            }
        }
        Ok(values)
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.import_data(data, limits)
    }
    fn export_form(
        &self,
        id: ValueId,
        call: &Form,
        origin: crate::compiler_data::ExpansionOrigin,
        limits: ConversionLimits,
    ) -> Result<Form, EvalError>;
    fn release(&self, id: ValueId) -> Result<(), EvalError>;
}
#[cfg(light_quickjs)]
impl MacroEvaluator for Evaluator {
    fn control(&self) -> ExecutionControl {
        Evaluator::control(self)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_unit(self, source)
    }
    fn compiler_frontier(&self, next: Option<usize>) -> Result<(), EvalError> {
        self.with_active(|active| active.compiler_frontier(next))
    }
    fn heap(&self) -> usize {
        Rc::as_ptr(&self.bridge) as usize
    }
    fn gensym_scope(&self, serial: u64, prefix: &str, retired: &str) -> Result<(), EvalError> {
        self.with_active(|active| active.gensym_scope(serial, prefix, retired))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        Evaluator::call(self, function, arguments)
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        Evaluator::import_form(self, form, limits)
    }
    fn import_macro_form(
        &self,
        form: &Form,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.with_active(|active| {
            active.bridge.borrow_mut().retain(form);
            active.import_data_with_service(&form.data(), limits, "importMacroForm")
        })
    }
    fn macro_arguments(
        &self,
        form: ValueId,
        _call: &Form,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        self.with_active(|active| active.macro_arguments(form, limits))
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        Evaluator::import_data(self, data, limits)
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        // Constructing a map can demand user hash/equality code and reenter
        // the compiler. Pending cache slots are not installed in JS yet.
        let nested = self.bridge.borrow().importing_environment;
        if nested {
            return self.import_data(data, limits);
        }
        self.bridge.borrow_mut().importing_environment = true;
        let result = self.enter(|ctx| {
            let descriptor =
                self.bridge
                    .borrow_mut()
                    .encode_environment(&ctx, data, &self.control, limits)?;
            let import: Function = ctx.eval("__lightEvaluator.importEnvironment")?;
            let result: Array = import.call((descriptor, limits.nodes, limits.depth))?;
            let index = result.get(0)?;
            let snapshot = (|| {
                let snapshots: Array = result.get(1)?;
                for snapshot in snapshots.iter::<Array>() {
                    self.bridge
                        .borrow_mut()
                        .decode(snapshot?, &self.control, limits, true)?;
                }
                if !result.get::<bool>(2)? {
                    self.bridge.borrow_mut().reset_environment_children();
                }
                Ok(())
            })();
            snapshot.map_err(|error| release_failed_import(&ctx, index, error))?;
            Ok(ValueId {
                session: self.generation,
                index,
            })
        });
        self.bridge.borrow_mut().importing_environment = false;
        if result.is_err() {
            self.bridge.borrow_mut().reset_environment_cache();
        }
        result
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
#[cfg(light_quickjs)]
impl MacroEvaluator for ActiveEvaluator<'_, '_> {
    fn control(&self) -> ExecutionControl {
        ActiveEvaluator::control(self)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        ActiveEvaluator::evaluate_unit(self, source)
    }
    fn compiler_frontier(&self, next: Option<usize>) -> Result<(), EvalError> {
        // A frontier transition has no result to retain and no source to parse.
        // Keep the normal control/error boundary, including compiler reentry.
        self.enter(|_| {
            let f: Function = self.services.get("compilerFrontier")?;
            f.call((next,))
        })
    }
    fn heap(&self) -> usize {
        Rc::as_ptr(self.bridge) as usize
    }
    fn gensym_scope(&self, serial: u64, prefix: &str, retired: &str) -> Result<(), EvalError> {
        self.enter(|_| {
            let f: Function = self.services.get("gensymScope")?;
            f.call::<_, ()>((serial as f64, prefix, retired))
        })
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        ActiveEvaluator::call(self, function, arguments)
    }
    fn import_form(&self, form: &Form, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        ActiveEvaluator::import_form(self, form, limits)
    }
    fn import_macro_form(
        &self,
        form: &Form,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        self.bridge.borrow_mut().retain(form);
        self.import_data_with_service(&form.data(), limits, "importMacroForm")
    }
    fn macro_arguments(
        &self,
        form: ValueId,
        _call: &Form,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        ActiveEvaluator::macro_arguments(self, form, limits)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        ActiveEvaluator::import_data(self, data, limits)
    }
    fn import_environment(
        &self,
        data: &Datum,
        limits: ConversionLimits,
    ) -> Result<ValueId, EvalError> {
        // Constructing a map can demand user hash/equality code and reenter
        // the compiler. Pending cache slots are not installed in JS yet.
        let nested = self.bridge.borrow().importing_environment;
        if nested {
            return self.import_data(data, limits);
        }
        self.bridge.borrow_mut().importing_environment = true;
        let result = self.enter(|ctx| {
            let descriptor =
                self.bridge
                    .borrow_mut()
                    .encode_environment(&ctx, data, self.control, limits)?;
            let import: Function = self.services.get("importEnvironment")?;
            let result: Array = import.call((descriptor, limits.nodes, limits.depth))?;
            let index = result.get(0)?;
            let snapshot = (|| {
                let snapshots: Array = result.get(1)?;
                for snapshot in snapshots.iter::<Array>() {
                    self.bridge
                        .borrow_mut()
                        .decode(snapshot?, self.control, limits, true)?;
                }
                if !result.get::<bool>(2)? {
                    self.bridge.borrow_mut().reset_environment_children();
                }
                Ok(())
            })();
            snapshot.map_err(|error| release_failed_import(&ctx, index, error))?;
            Ok(ValueId {
                session: self.generation,
                index,
            })
        });
        self.bridge.borrow_mut().importing_environment = false;
        if result.is_err() {
            self.bridge.borrow_mut().reset_environment_cache();
        }
        result
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
#[cfg(light_quickjs)]
pub struct ActiveEvaluator<'a, 'js> {
    ctx: Ctx<'js>,
    // Borrow the immutable service table for this active scope. Nested compiler
    // work can use it without reparsing a JS expression for every operation.
    // No Persistent root or engine value survives the scoped context lifetime.
    services: Object<'js>,
    control: &'a ExecutionControl,
    generation: u64,
    bridge: &'a Rc<RefCell<BridgeState>>,
    preserve_language_errors: bool,
}

/// An import owns its temporary roots until canonical validation succeeds and
/// hands its ID to the caller. Preserve a pending JS exception across cleanup;
/// a terminal engine failure can prevent release, but ordinary errors cannot
/// keep an unreachable imported value rooted for the rest of the session.
#[cfg(light_quickjs)]
fn release_failed_import(ctx: &Ctx<'_>, index: u64, error: rquickjs::Error) -> rquickjs::Error {
    release_failed_imports(ctx, &[index], error)
}
#[cfg(light_quickjs)]
fn release_failed_imports(
    ctx: &Ctx<'_>,
    indices: &[u64],
    error: rquickjs::Error,
) -> rquickjs::Error {
    let exception = error.is_exception().then(|| ctx.catch());
    for index in indices {
        let released = ctx
            .eval::<Function, _>("__lightEvaluator.release")
            .and_then(|release| release.call::<_, ()>((*index,)));
        if released.is_err() {
            let _ = ctx.catch();
        }
    }
    if let Some(exception) = exception {
        let _ = ctx.throw(exception);
    }
    error
}

#[cfg(light_quickjs)]
impl<'js> ActiveEvaluator<'_, 'js> {
    pub fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    fn enter<T>(&self, f: impl FnOnce(Ctx<'js>) -> rquickjs::Result<T>) -> Result<T, EvalError> {
        self.control.poll()?;
        let result = f(self.ctx.clone())
            .and_then(|value| {
                let check = self.services.get::<_, Function>("checkBridge")?;
                match crate::compile_trace::frame() {
                    Some(frame) => check.call::<_, ()>((frame,))?,
                    None => check.call::<_, ()>(())?,
                }
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
        let index = self.enter(|_| {
            let f: Function = self.services.get("evaluate")?;
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
        let index = self.enter(|_| {
            let f: Function = self.services.get("call")?;
            f.call((function, arguments))
        })?;
        Ok(ValueId {
            session: self.generation,
            index,
        })
    }
    pub(crate) fn bind(&self, name: &str, id: ValueId, core: bool) -> Result<(), EvalError> {
        let id = self.check(id)?;
        self.enter(|_| {
            let f: Function = self
                .services
                .get(if core { "coreBinding" } else { "bind" })?;
            f.call((name, id))
        })
    }
    pub(crate) fn lookup(&self, name: &str, core: bool) -> Result<ValueId, EvalError> {
        let index = self.enter(|_| {
            let f: Function = self
                .services
                .get(if core { "coreLookup" } else { "lookup" })?;
            f.call((name,))
        })?;
        Ok(ValueId {
            session: self.generation,
            index,
        })
    }
    pub fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        let id = self.check(id)?;
        self.enter(|_| {
            let f: Function = self.services.get("number")?;
            f.call((id,))
        })
    }
    pub fn string(&self, id: ValueId) -> Result<String, EvalError> {
        let id = self.check(id)?;
        self.enter(|_| {
            let f: Function = self.services.get("string")?;
            f.call((id,))
        })
    }
    pub fn print(&self, id: ValueId) -> Result<String, EvalError> {
        let id = self.check(id)?;
        self.enter(|_| {
            let f: Function = self.services.get("print")?;
            f.call((id,))
        })
    }
    pub fn release(&self, id: ValueId) -> Result<(), EvalError> {
        let id = self.check(id)?;
        self.enter(|_| {
            let f: Function = self.services.get("release")?;
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
        self.import_data_with_service(data, limits, "importForm")
    }
    fn macro_arguments(
        &self,
        form: ValueId,
        limits: ConversionLimits,
    ) -> Result<Vec<ValueId>, EvalError> {
        let form = self.check(form)?;
        let indices: Vec<u64> = self.enter(|ctx| {
            let import: Function = self.services.get("macroArguments")?;
            let result: Array = import.call((form, limits.nodes, limits.depth))?;
            let indices: Vec<u64> = result.get(0)?;
            let snapshot: rquickjs::Result<()> = (|| {
                let snapshots: Array = result.get(1)?;
                for item in snapshots.iter::<Array>() {
                    self.bridge
                        .borrow_mut()
                        .decode(item?, self.control, limits, true)?;
                }
                Ok(())
            })();
            snapshot.map_err(|error| release_failed_imports(&ctx, &indices, error))?;
            Ok(indices)
        })?;
        Ok(indices
            .into_iter()
            .map(|index| ValueId {
                session: self.generation,
                index,
            })
            .collect())
    }
    fn import_data_with_service(
        &self,
        data: &Datum,
        limits: ConversionLimits,
        service: &str,
    ) -> Result<ValueId, EvalError> {
        let index = self.enter(|ctx| {
            let descriptor = self
                .bridge
                .borrow_mut()
                .encode(&ctx, data, self.control, limits)?;
            let f: Function = self.services.get(service)?;
            let index: u64 = f.call((descriptor,))?;
            // Snapshot canonical payloads so mutable host literals are never reused
            // after modification. Reader lexical distinctions remain in originals.
            let snapshot = (|| {
                let export: Function = self.services.get("exportForm")?;
                let snapshot: Array = export.call((index, limits.nodes, limits.depth))?;
                self.bridge
                    .borrow_mut()
                    .decode(snapshot, self.control, limits, true)?;
                Ok(())
            })();
            snapshot.map_err(|error| release_failed_import(&ctx, index, error))?;
            Ok(index)
        })?;
        Ok(ValueId {
            session: self.generation,
            index,
        })
    }
    pub fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        let id = self.check(id)?;
        self.enter(|_| {
            let f: Function = self.services.get("exportForm")?;
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
#[cfg(light_quickjs)]
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
#[cfg(light_quickjs)]
struct LimitedAllocator {
    used: usize,
    limit: usize,
    control: Weak<ControlState>,
}
#[cfg(light_quickjs)]
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
#[cfg(light_quickjs)]
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

#[cfg(light_quickjs)]
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
#[cfg(light_quickjs)]
struct BridgeOutput {
    data: Form,
    bridge: Rc<RefCell<BridgeState>>,
    control: ExecutionControl,
    limits: ConversionLimits,
}
#[cfg(light_quickjs)]
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
    fn protocol_arity_cache_does_not_inherit_array_properties() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let result = evaluator
            .evaluate_js(
                r#"(() => {
            const value = rt.extendInstance({}, [['ILookup', 0],
                ['ILookup/-lookup', 2, () => 20], ['ILookup/-lookup', 3, () => 30]], true);
            const previous = Object.getOwnPropertyDescriptor(Array.prototype, '3');
            try {
                Object.defineProperty(Array.prototype, '3', {
                    value: 'wrong-slot', configurable: true, writable: true
                });
                return rt.core.get(value, 'key', 'fallback');
            } finally {
                if (previous) Object.defineProperty(Array.prototype, '3', previous);
                else delete Array.prototype[3];
            }
        })()"#,
            )
            .unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 30.0);
        evaluator.release(result).unwrap();
    }

    #[test]
    fn frontier_service_preserves_errors_dynamic_lookup_and_cancellation() {
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let setup = evaluator.evaluate_js(r#"(() => {
            globalThis.frontierEvents = [];
            globalThis.frontierError = rt.core.exInfo('frontier failed', rt.core.hashMap('code', 42));
            globalThis.__lightEnterCompilerFrontier = next => {
                frontierEvents.push(next);
                if (next === 7) throw frontierError;
                return {unneeded: new Array(10000)};
            };
            globalThis.__lightLeaveCompilerFrontier = () => frontierEvents.push('leave');
        })()"#).unwrap();
        evaluator.release(setup).unwrap();
        evaluator.compiler_frontier(Some(0)).unwrap();
        evaluator
            .with_active(|active| active.compiler_frontier(None))
            .unwrap();
        let error = evaluator.compiler_frontier(Some(7)).unwrap_err();
        assert!(error.0.contains("frontier failed"), "{error}");
        evaluator
            .enter(|ctx| {
                assert!(ctx.eval::<bool, _>("__lightEvaluator.errorValue() === frontierError")?);
                Ok(())
            })
            .unwrap();
        let change = evaluator
            .evaluate_js(
                "globalThis.__lightEnterCompilerFrontier = next => frontierEvents.push(next + 100)",
            )
            .unwrap();
        evaluator.release(change).unwrap();
        evaluator.compiler_frontier(Some(3)).unwrap();
        evaluator
            .enter(|ctx| {
                assert!(ctx.eval::<bool, _>("__lightEvaluator.errorValue() === null")?);
                assert_eq!(
                    ctx.eval::<String, _>("JSON.stringify(frontierEvents)")?,
                    "[0,\"leave\",7,103]"
                );
                Ok(())
            })
            .unwrap();
        evaluator.control().cancel();
        assert!(
            evaluator
                .compiler_frontier(None)
                .unwrap_err()
                .0
                .contains("cancelled")
        );
    }

    #[test]
    fn frontier_service_can_reenter_an_active_compilation() {
        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let weak = Rc::downgrade(&evaluator);
        evaluator
            .install_unit_service("nestedFrontier", move |ctx, next| {
                let evaluator = weak.upgrade().unwrap();
                let active = evaluator.active(ctx).map_err(|e| e.0)?;
                active
                    .compiler_frontier(Some(next.try_into().unwrap()))
                    .map_err(|e| e.0)?;
                active.compiler_frontier(None).map_err(|e| e.0)?;
                Ok("nested".into())
            })
            .unwrap();
        let setup = evaluator
            .evaluate_js(
                r#"(() => {
            globalThis.frontierEvents = [];
            globalThis.__lightEnterCompilerFrontier = next => {
                frontierEvents.push(next);
                if (next > 0) nestedFrontier(next - 1);
            };
            globalThis.__lightLeaveCompilerFrontier = () => frontierEvents.push('leave');
        })()"#,
            )
            .unwrap();
        evaluator.release(setup).unwrap();
        evaluator.compiler_frontier(Some(2)).unwrap();
        evaluator.compiler_frontier(None).unwrap();
        let result = evaluator
            .evaluate_js("JSON.stringify(frontierEvents)")
            .unwrap();
        assert_eq!(
            evaluator.string(result).unwrap(),
            "[2,1,0,\"leave\",\"leave\",\"leave\"]"
        );
        evaluator.release(result).unwrap();
        assert_eq!(Rc::strong_count(&evaluator), 1);
    }

    #[test]
    fn frontier_replay_includes_an_earlier_bundle_initialized_by_a_proxy_patch() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let setup = evaluator
            .evaluate_unit("rt => { globalThis.__lightRuntimeForCompiler = rt; }")
            .unwrap();
        evaluator.release(setup).unwrap();
        let registry = evaluator
            .evaluate_js(include_str!("compiler_roots.js"))
            .unwrap();
        evaluator.release(registry).unwrap();
        let result = evaluator.evaluate_js(r#"(() => {
            const a = {}, b = {}, rawC = {};
            let armed = false, calls = 0;
            const c = new Proxy(rawC, {deleteProperty(target, key) {
                if (armed) {
                    armed = false;
                    void __lightCompilerRoots.b;
                }
                return Reflect.deleteProperty(target, key);
            }});
            for (const [name, value, number] of [['a', a, 1], ['b', b, 2], ['c', c, 3]]) {
                __lightRegisterBundle([name], rt => {
                    calls++;
                    rt.extendInstance(value, [['ICounted', 0], ['ICounted/-count', 1, () => number]], true);
                    __lightCompilerBundleProducts[name] = value;
                }, [['probe', '_', name]], 'probe');
            }
            void __lightCompilerRoots.c;
            void __lightCompilerRoots.a;
            armed = true;
            __lightEnterCompilerFrontier(0);
            const absent = !Object.hasOwn(b, 'cljs$core$ICounted$_count$arity$1');
            __lightLeaveCompilerFrontier();
            return JSON.stringify([absent, calls, rt.core.count(a), rt.core.count(b), rt.core.count(c)]);
        })()"#).unwrap();
        assert_eq!(evaluator.string(result).unwrap(), "[true,3,1,2,3]");
        evaluator.release(result).unwrap();
    }

    #[test]
    fn engine_maps_preserve_key_semantics_across_growth_and_deletion() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let result = evaluator.evaluate_js(r#"(() => {
            const keys = Array.from({length: 4096}, (_, i) => [i, {i}, Symbol(String(i))]).flat();
            const map = new Map(keys.map((key, i) => [key, i]));
            const weak = new WeakMap(keys.filter(key => typeof key === 'object').map(key => [key, key.i]));
            for (let i = 0; i < keys.length; i++) {
                if (map.get(keys[i]) !== i) throw Error('lost key ' + i);
                if (typeof keys[i] === 'object' && weak.get(keys[i]) !== keys[i].i) throw Error('weak key');
            }
            for (let i = 0; i < keys.length; i += 3) map.delete(keys[i]);
            let position = 0;
            for (const [key, value] of map) {
                while (position % 3 === 0) position++;
                if (key !== keys[position] || value !== position++) throw Error('iteration order');
            }
            map.set(-0, 'zero'); map.set(0, 'updated');
            map.set(NaN, 'nan'); map.set(Number('nan'), 'updated nan');
            map.set(1n, 'bigint'); map.set(1, 'number'); map.set('1', 'string');
            if (map.get(-0) !== 'updated' || map.get(NaN) !== 'updated nan' ||
                map.get(BigInt('1')) !== 'bigint' || map.get(1.0) !== 'number' || map.get('1') !== 'string') throw Error('key equality');
            const set = new Set([NaN, NaN, -0, 0, 1n, 1, '1']);
            if (set.size !== 5 || !set.has(NaN) || !set.has(+0)) throw Error('set equality');
            return true;
        })()"#).unwrap();
        assert_eq!(evaluator.print(result).unwrap(), "true");
    }

    #[test]
    fn failed_macro_calls_release_their_imported_object_arguments() {
        use crate::compiler_data::{DataBuilder, ExpansionOrigin, Name, Source};
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let function = evaluator
            .evaluate_js("() => { throw new Error('expected failure'); }")
            .unwrap();
        let mut host = crate::MacroHost::new();
        let form = host
            .read_compiler_form(&Source::new("failed.cljs", "(explode [some-name])"))
            .unwrap()
            .unwrap();
        let env = DataBuilder.map(&[]).unwrap();
        let live = evaluator.bridge.borrow().live_provenance.clone();
        for _ in 0..25 {
            let error = host
                .expand_javascript(
                    &evaluator,
                    function,
                    &form,
                    &env,
                    ExpansionOrigin {
                        macro_name: Name::new(Some("cljs.user"), "explode"),
                        call: form.span(),
                        definition: None,
                        helpers: vec![],
                    },
                )
                .unwrap_err();
            assert!(error.contains("expected failure"), "{error}");
        }
        evaluator.collect_garbage();
        assert_eq!(live.get(), 0, "failed calls must not pin their input forms");
        evaluator.release(function).unwrap();
    }

    #[test]
    fn shared_macro_arguments_restore_provenance_after_environment_import() {
        use crate::compiler_data::{DataBuilder, Name, Origin, Source};
        let c = DataBuilder;
        let keyword = c.keyword(Name::new(None, "shared"));
        let cases = [
            (
                Datum::new(DatumKind::Ratio {
                    numerator: 3,
                    denominator: 2,
                }),
                Datum::new(DatumKind::Double(1.5)),
            ),
            (
                c.integer(9007199254740993),
                Datum::new(DatumKind::Double(9007199254740992.0)),
            ),
            (Datum::new(DatumKind::Character(97)), c.string(vec![97])),
            (
                keyword.clone(),
                keyword.with_metadata(
                    c.map(&[(c.keyword(Name::new(None, "environment")), c.boolean(true))])
                        .unwrap(),
                ),
            ),
        ];
        for (argument, environment) in cases {
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let source = Source::new("shared.cljs", "(identity value)");
            let data = c
                .list(&[c.symbol(Name::new(None, "identity")), argument.clone()])
                .unwrap();
            let form = Form::from_data(
                data,
                source.span(0..source.text().len()).unwrap(),
                Origin::Source,
            );
            let limits = ConversionLimits::default();
            let input = evaluator.import_macro_form(&form, limits).unwrap();
            let env = evaluator.import_environment(&environment, limits).unwrap();
            let access = evaluator
                .evaluate_js("form => rt.core.nth(form, 1)")
                .unwrap();
            let naive = evaluator.call(access, &[input]).unwrap();
            let stale = evaluator.export_data(naive, limits).unwrap();
            assert_ne!(
                stale.identity(),
                argument.identity(),
                "the naive shortcut must lose this input's provenance"
            );
            evaluator.release(naive).unwrap();
            let arguments = evaluator.macro_arguments(input, &form, limits).unwrap();
            assert_eq!(arguments.len(), 1);
            let output = evaluator.export_data(arguments[0], limits).unwrap();
            assert_eq!(
                output.identity(),
                argument.identity(),
                "shared input must recover the exact original datum"
            );
            let same = evaluator
                .evaluate_js("(form, argument) => rt.core.nth(form, 1) === argument")
                .unwrap();
            let result = evaluator.call(same, &[input, arguments[0]]).unwrap();
            assert_eq!(evaluator.print(result).unwrap(), "true");
            for value in [input, env, access, arguments[0], same, result] {
                evaluator.release(value).unwrap();
            }
        }
    }

    #[test]
    fn failed_macro_environments_release_pending_input_graphs() {
        use crate::compiler_data::{
            CompilerLiteralKind, DataBuilder, ExpansionOrigin, Name, Source,
        };
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let function = evaluator
            .evaluate_js("(_form, _env, value) => value")
            .unwrap();
        let mut host = crate::MacroHost::new();
        let form = host
            .read_compiler_form(&Source::new("pending.cljs", "(identity [some-name])"))
            .unwrap()
            .unwrap();
        let environment = Datum::new(DatumKind::CompilerLiteral {
            kind: CompilerLiteralKind::Uuid,
            payload: DataBuilder.string("not-valid".encode_utf16().collect::<Vec<_>>()),
        });
        let live = evaluator.bridge.borrow().live_provenance.clone();
        for _ in 0..25 {
            let error = host
                .expand_javascript(
                    &evaluator,
                    function,
                    &form,
                    &environment,
                    ExpansionOrigin {
                        macro_name: Name::new(Some("cljs.user"), "identity"),
                        call: form.span(),
                        definition: None,
                        helpers: vec![],
                    },
                )
                .unwrap_err();
            assert!(error.contains("invalid UUID"), "{error}");
        }
        evaluator.collect_garbage();
        // Each call's scalar table goes when its input is released.
        assert_eq!(live.get(), 0, "no scalar provenance may outlive its call");
        evaluator.release(function).unwrap();
        drop(evaluator);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn shared_macro_literals_are_constructed_once() {
        use crate::compiler_data::{DataBuilder, ExpansionOrigin, Name, Source};
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let setup = evaluator.evaluate_js("(() => { const NativeDate = Date; globalThis.dateInputs = 0; globalThis.Date = class extends NativeDate { constructor(...args) { super(...args); dateInputs++; } }; return null; })()").unwrap();
        evaluator.release(setup).unwrap();
        let function = evaluator
            .evaluate_js(
                "(form, _env, value) => rt.core.nth(form, 1) === value && dateInputs === 1",
            )
            .unwrap();
        let mut host = crate::MacroHost::new();
        let form = host
            .read_compiler_form(&Source::new(
                "literal.cljs",
                "(identity #inst \"2024-01-01T00:00:00.000Z\")",
            ))
            .unwrap()
            .unwrap();
        let result = host
            .expand_javascript(
                &evaluator,
                function,
                &form,
                &DataBuilder.nil(),
                ExpansionOrigin {
                    macro_name: Name::new(Some("cljs.user"), "identity"),
                    call: form.span(),
                    definition: None,
                    helpers: vec![],
                },
            )
            .unwrap();
        assert!(matches!(result.data().kind(), DatumKind::Boolean(true)));
        evaluator.release(function).unwrap();
    }

    #[test]
    fn value_registry_roots_are_counted_and_released() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let baseline = evaluator.value_roots().unwrap();
        let ids = (0..3)
            .map(|_| evaluator.evaluate_js("new Array(10000)").unwrap())
            .collect::<Vec<_>>();
        evaluator.collect_garbage();
        assert_eq!(evaluator.value_roots().unwrap(), baseline + 3);
        for id in ids {
            evaluator.release(id).unwrap();
        }
        assert_eq!(evaluator.value_roots().unwrap(), baseline);
    }

    #[test]
    fn failed_argument_snapshots_drop_the_pending_graph_and_partial_results() {
        use crate::compiler_data::Source;
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let mut host = crate::MacroHost::new();
        let form = host
            .read_compiler_form(&Source::new(
                "arguments.cljs",
                "(identity [first] #js [second])",
            ))
            .unwrap()
            .unwrap();
        let mutate = evaluator
            .evaluate_js("form => { rt.core.nth(form, 2).push(() => null); return null; }")
            .unwrap();
        let live = evaluator.bridge.borrow().live_provenance.clone();
        let limits = ConversionLimits::default();
        for _ in 0..25 {
            let input = evaluator.import_macro_form(&form, limits).unwrap();
            let result = evaluator.call(mutate, &[input]).unwrap();
            evaluator.release(result).unwrap();
            let error = evaluator.macro_arguments(input, &form, limits).unwrap_err();
            assert!(
                error
                    .to_string()
                    .contains("Unsupported macro result function"),
                "{error}"
            );
            evaluator.release(input).unwrap();
        }
        evaluator.collect_garbage();
        assert_eq!(
            live.get(),
            0,
            "failed preparation must not retain snapshots or descriptor graphs"
        );
        evaluator.release(mutate).unwrap();
    }

    #[test]
    fn native_bridge_strings_preserve_every_utf16_code_unit_in_both_directions() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let units: Vec<u16> = (0..=u16::MAX).collect();
        let input = evaluator
            .import_data(
                &Datum::new(DatumKind::String(units.into())),
                ConversionLimits::default(),
            )
            .unwrap();
        let check = evaluator.evaluate_js("text => { for (let i = 0; i < 65536; i++) if (text.charCodeAt(i) !== i) throw Error('code unit ' + i); return text.length; }").unwrap();
        let result = evaluator.call(check, &[input]).unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 65536.0);
        // Independently constructed ropes exercise export without borrowed
        // provenance restoring the original Rust text and hiding a bad copy.
        let output = evaluator.evaluate_js("(() => { let text = ''; for (let i = 65535; i >= 0; i--) text += String.fromCharCode(i); return text; })()").unwrap();
        let data = evaluator
            .export_data(output, ConversionLimits::default())
            .unwrap();
        let DatumKind::String(text) = data.kind() else {
            panic!("string");
        };
        assert_eq!(text.as_ref(), (0..=u16::MAX).rev().collect::<Vec<_>>());
        for id in [input, check, result, output] {
            evaluator.release(id).unwrap();
        }
    }

    #[test]
    fn native_bridge_name_components_and_empty_text_preserve_utf16() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let output = evaluator
            .evaluate_unit(
                r#"rt => rt.core.vector(
            "", rt.core.symbol("n\u0000\ud800", "k\udc00\ud83d\ude00"),
            rt.core.keyword("n\u0000\ud800", "k\udc00\ud83d\ude00"))"#,
            )
            .unwrap();
        let data = evaluator
            .export_data(output, ConversionLimits::default())
            .unwrap();
        let entries = data.elements().unwrap();
        assert!(matches!(entries[0].kind(), DatumKind::String(text) if text.is_empty()));
        let name = crate::compiler_data::Name::from_utf16(
            Some(&[110, 0, 0xd800]),
            &[107, 0xdc00, 0xd83d, 0xde00],
        );
        assert!(matches!(entries[1].kind(), DatumKind::Symbol(n) if n == &name));
        assert!(matches!(entries[2].kind(), DatumKind::Keyword(n) if n == &name));
        let imported = evaluator
            .import_data(&data, ConversionLimits::default())
            .unwrap();
        let check = evaluator.evaluate_unit(r#"rt => items => {
            const a = rt.core.nth(items, 1), b = rt.core.nth(items, 2);
            return rt.core.nth(items, 0) === "" && rt.core.namespace(a) === "n\u0000\ud800"
              && rt.core.name(a) === "k\udc00\ud83d\ude00"
              && rt.core.namespace(b) === "n\u0000\ud800" && rt.core.name(b) === rt.core.name(a) ? 1 : 0;
        }"#).unwrap();
        let result = evaluator.call(check, &[imported]).unwrap();
        assert_eq!(evaluator.number(result).unwrap(), 1.0);
        for id in [output, imported, check, result] {
            evaluator.release(id).unwrap();
        }
    }

    #[test]
    fn native_bridge_unqualified_names_keep_a_null_namespace() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let c = crate::compiler_data::DataBuilder;
        let name = crate::compiler_data::Name::new(None, "readers");
        for data in [c.symbol(name.clone()), c.keyword(name)] {
            let input = evaluator
                .import_data(&data, ConversionLimits::default())
                .unwrap();
            let check = evaluator
                .evaluate_unit("rt => value => rt.core.namespace(value) === null ? 1 : 0")
                .unwrap();
            let result = evaluator.call(check, &[input]).unwrap();
            assert_eq!(evaluator.number(result).unwrap(), 1.0);
            for id in [input, check, result] {
                evaluator.release(id).unwrap();
            }
        }
    }

    #[test]
    fn released_bridge_inputs_release_native_provenance() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let c = crate::compiler_data::DataBuilder;
        let live = evaluator.bridge.borrow().live_provenance.clone();
        for n in 0..1000 {
            let data = c
                .vector(&[c.symbol(crate::compiler_data::Name::new(None, &format!("input-{n}")))])
                .unwrap();
            let input = evaluator
                .import_data(&data, ConversionLimits::default())
                .unwrap();
            evaluator.release(input).unwrap();
        }
        evaluator.collect_garbage();
        assert_eq!(
            live.get(),
            0,
            "released input provenance must not be session-owned"
        );
    }

    #[test]
    fn scalar_provenance_retains_values_instead_of_import_history() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let c = crate::compiler_data::DataBuilder;
        let live = evaluator.bridge.borrow().live_provenance.clone();
        for _ in 0..1000 {
            let data = c
                .vector(&[c.integer(42), c.nil(), c.boolean(true)])
                .unwrap();
            let input = evaluator
                .import_data(&data, ConversionLimits::default())
                .unwrap();
            evaluator.release(input).unwrap();
        }
        evaluator.collect_garbage();
        assert_eq!(live.get(), 3);
        drop(evaluator);
        assert_eq!(live.get(), 0);
    }

    #[test]
    fn failed_import_validation_releases_temporary_roots() {
        let c = crate::compiler_data::DataBuilder;
        // The engine can construct this UUID, but canonical validation rejects
        // its payload after the value has been imported and assigned an ID.
        let invalid_cases = [
            (
                crate::compiler_data::CompilerLiteralKind::Uuid,
                "invalid UUID",
            ),
            (
                crate::compiler_data::CompilerLiteralKind::Instant,
                "invalid instant",
            ),
        ];
        for (kind, message) in invalid_cases {
            let invalid = Datum::new(DatumKind::CompilerLiteral {
                kind,
                payload: c.string("not-valid".encode_utf16().collect::<Vec<_>>()),
            });
            for mode in 0..3 {
                let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
                let live = evaluator.bridge.borrow().live_provenance.clone();
                let data = if mode == 0 {
                    c.vector(std::slice::from_ref(&invalid)).unwrap()
                } else {
                    c.map(&[(
                        c.keyword(crate::compiler_data::Name::new(None, "invalid")),
                        invalid.clone(),
                    )])
                    .unwrap()
                };
                for _ in 0..100 {
                    let result = match mode {
                        0 => evaluator.import_data(&data, ConversionLimits::default()),
                        1 => evaluator.import_environment(&data, ConversionLimits::default()),
                        _ => evaluator.with_active(|active| {
                            active.import_environment(&data, ConversionLimits::default())
                        }),
                    };
                    assert!(result.unwrap_err().to_string().contains(message));
                }
                evaluator.collect_garbage();
                assert_eq!(
                    live.get(),
                    1,
                    "only the latest string provenance survives, mode {mode}"
                );
                let valid = evaluator
                    .import_data(&c.vector(&[]).unwrap(), ConversionLimits::default())
                    .unwrap();
                assert_eq!(evaluator.print(valid).unwrap(), "[]");
                evaluator.release(valid).unwrap();
                drop(evaluator);
                assert_eq!(live.get(), 0);
            }
        }
    }

    #[test]
    fn environment_cache_preserves_lexical_data_and_copies_mutable_literals() {
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let limits = ConversionLimits::default();
        let key = |name| {
            crate::compiler_data::DataBuilder.keyword(crate::compiler_data::Name::new(None, name))
        };
        let c = crate::compiler_data::DataBuilder;
        let data = c
            .map(&[
                (
                    key("stable"),
                    c.vector(&[
                        Datum::new(DatumKind::Ratio {
                            numerator: 3,
                            denominator: 2,
                        }),
                        c.integer(9007199254740993),
                    ])
                    .unwrap(),
                ),
                (
                    key("mutable"),
                    Datum::new(DatumKind::CompilerLiteral {
                        kind: crate::compiler_data::CompilerLiteralKind::JsArray,
                        payload: c.vector(&[c.integer(1)]).unwrap(),
                    }),
                ),
            ])
            .unwrap();
        let mutate = evaluator.evaluate_js("env => { rt.core.get(env, rt.core.keyword(null, 'mutable')).push(2); return null; }").unwrap();
        for _ in 0..3 {
            let input = evaluator.import_environment(&data, limits).unwrap();
            let output = evaluator.export_data(input, limits).unwrap();
            let stable = output.get(&key("stable")).unwrap().unwrap();
            let DatumKind::Collection { children, .. } = stable.kind() else {
                panic!("vector")
            };
            assert!(matches!(
                children[0].kind(),
                DatumKind::Ratio {
                    numerator: 3,
                    denominator: 2
                }
            ));
            assert!(matches!(
                children[1].kind(),
                DatumKind::Integer(9007199254740993)
            ));
            let mutable = output.get(&key("mutable")).unwrap().unwrap();
            let DatumKind::CompilerLiteral { payload, .. } = mutable.kind() else {
                panic!("literal")
            };
            let DatumKind::Collection { children, .. } = payload.kind() else {
                panic!("payload")
            };
            assert_eq!(children.len(), 1);
            let result = evaluator.call(mutate, &[input]).unwrap();
            evaluator.release(result).unwrap();
            evaluator.release(input).unwrap();
        }
        // Cached components still count against the logical conversion limits.
        assert!(
            evaluator
                .import_environment(
                    &data,
                    ConversionLimits {
                        nodes: 3,
                        depth: 128
                    }
                )
                .is_err()
        );
    }

    #[test]
    fn environment_child_eviction_preserves_values_and_metadata_order() {
        use crate::compiler_data::{DataBuilder, Name};
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let c = DataBuilder;
        let key = |name| c.keyword(Name::new(None, name));
        let rows = (0..512)
            .map(|i| c.vector(&[c.integer(i)]).unwrap())
            .collect::<Vec<_>>();
        let inspect = evaluator
            .evaluate_js(
                r#"env => {
                  const value = rt.core.get(env, rt.core.keyword(null, 'values'));
                  const read = xs => Array.from({length: rt.core.count(xs)},
                    (_, i) => rt.core.nth(rt.core.nth(xs, i), 0));
                  return JSON.stringify([read(value),
                    read(rt.core.get(rt.core.meta(value), rt.core.keyword(null, 'rows')))]);
                }"#,
            )
            .unwrap();
        // More children than cache slots, with metadata encoded before the
        // payload but decoded after it. Later imports reuse both old and new
        // children in a different order. Observe JS values directly: restoring
        // borrowed Rust provenance alone could hide a wrongly resolved slot.
        for round in 0..4 {
            let mut values = (round * 71..round * 71 + 192).collect::<Vec<_>>();
            if round % 2 == 1 {
                values.reverse();
            }
            let metadata = (0..160)
                .map(|i| (i * 3 + round * 43) % rows.len())
                .collect::<Vec<_>>();
            let vector = |ids: &[usize]| {
                c.vector(&ids.iter().map(|&i| rows[i].clone()).collect::<Vec<_>>())
                    .unwrap()
            };
            let value =
                vector(&values).with_metadata(c.map(&[(key("rows"), vector(&metadata))]).unwrap());
            let data = c.map(&[(key("values"), value)]).unwrap();
            let input = evaluator
                .import_environment(&data, ConversionLimits::default())
                .unwrap();
            let output = evaluator.call(inspect, &[input]).unwrap();
            let numbers = |ids: &[usize]| {
                ids.iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            };
            assert_eq!(
                evaluator.string(output).unwrap(),
                format!("[[{}],[{}]]", numbers(&values), numbers(&metadata))
            );
            evaluator.release(output).unwrap();
            evaluator.release(input).unwrap();
        }
    }

    #[test]
    fn discarded_environment_children_preserve_spelling_when_reused() {
        use crate::compiler_data::{DataBuilder, Name};
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        let c = DataBuilder;
        let key = |name| c.keyword(Name::new(None, name));
        let hidden = c
            .vector(&[
                Datum::new(DatumKind::Ratio {
                    numerator: 3,
                    denominator: 2,
                }),
                c.integer(9007199254740993),
            ])
            .unwrap();
        // The first value is decoded but discarded by the duplicate map key.
        // It has no canonical snapshot and must not become a reusable child.
        let duplicates = c
            .map(&[(key("same"), hidden.clone()), (key("same"), c.nil())])
            .unwrap();
        let warm = c.map(&[(key("discarded"), duplicates)]).unwrap();
        let limits = ConversionLimits::default();
        let value = evaluator.import_environment(&warm, limits).unwrap();
        assert_eq!(evaluator.print(value).unwrap(), "{:discarded {:same nil}}");
        evaluator.release(value).unwrap();
        let data = c.map(&[(key("kept"), hidden)]).unwrap();
        let value = evaluator.import_environment(&data, limits).unwrap();
        let exported = evaluator.export_data(value, limits).unwrap();
        let kept = exported.get(&key("kept")).unwrap().unwrap();
        let DatumKind::Collection { children, .. } = kept.kind() else {
            panic!("vector")
        };
        assert!(matches!(
            children[0].kind(),
            DatumKind::Ratio {
                numerator: 3,
                denominator: 2
            }
        ));
        assert!(matches!(
            children[1].kind(),
            DatumKind::Integer(9007199254740993)
        ));
        evaluator.release(value).unwrap();
    }

    #[test]
    fn reused_environment_children_still_count_against_node_and_depth_limits() {
        use crate::compiler_data::{DataBuilder, Name};
        let c = DataBuilder;
        let key = c.keyword(Name::new(None, "values"));
        let child = c.vector(&[c.integer(1), c.integer(2)]).unwrap();
        let warm = c
            .map(&[(key.clone(), c.vector(std::slice::from_ref(&child)).unwrap())])
            .unwrap();
        let changed = c
            .map(&[(key, c.vector(&[child, c.integer(9)]).unwrap())])
            .unwrap();
        for limits in [
            ConversionLimits {
                nodes: 6,
                depth: 128,
            },
            ConversionLimits {
                nodes: 100,
                depth: 3,
            },
        ] {
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            let value = evaluator
                .import_environment(&warm, ConversionLimits::default())
                .unwrap();
            evaluator.release(value).unwrap();
            let error = evaluator.import_environment(&changed, limits).unwrap_err();
            assert!(error.to_string().contains("conversion limit"), "{error}");
        }
    }

    #[test]
    fn failed_environment_import_does_not_publish_uninstalled_cache_slots() {
        let evaluator = Evaluator::interactive(EvaluatorOptions::default()).unwrap();
        let c = crate::compiler_data::DataBuilder;
        let key = |name| c.keyword(crate::compiler_data::Name::new(None, name));
        let stable = c.vector(&[c.integer(222)]).unwrap();
        let invalid = Datum::new(DatumKind::CompilerLiteral {
            kind: crate::compiler_data::CompilerLiteralKind::BigInteger,
            payload: c.nil(),
        });
        let bad = c
            .map(&[(key("stable"), stable.clone()), (key("bad"), invalid)])
            .unwrap();
        assert!(
            evaluator
                .import_environment(&bad, ConversionLimits::default())
                .is_err()
        );
        let good = c.map(&[(key("stable"), stable)]).unwrap();
        let result = evaluator
            .import_environment(&good, ConversionLimits::default())
            .unwrap();
        assert_eq!(evaluator.print(result).unwrap(), "{:stable [222]}");
    }

    #[test]
    fn environment_import_can_reenter_before_its_cache_slots_are_installed() {
        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let c = crate::compiler_data::DataBuilder;
        let data = c
            .map(&[(
                c.keyword(crate::compiler_data::Name::new(None, "stable")),
                c.vector(&[c.integer(42)]).unwrap(),
            )])
            .unwrap();
        let nested_data = data.clone();
        let weak = Rc::downgrade(&evaluator);
        evaluator
            .install_unit_service("reenterEnvironment", move |ctx, _| {
                let evaluator = weak.upgrade().unwrap();
                let active = evaluator.active(ctx).map_err(|e| e.0)?;
                let input = active
                    .import_environment(&nested_data, ConversionLimits::default())
                    .map_err(|e| e.0)?;
                assert_eq!(active.print(input).map_err(|e| e.0)?, "{:stable [42]}");
                active.release(input).map_err(|e| e.0)?;
                Ok("nested".into())
            })
            .unwrap();
        // Force a callback during descriptor construction, as demanded user
        // hash/equality implementations can do. Restore before reentering.
        let hook = evaluator.evaluate_js("(() => { const map = Array.prototype.map; Array.prototype.map = function (...args) { Array.prototype.map = map; reenterEnvironment(0); return Reflect.apply(map, this, args); }; return null; })()").unwrap();
        evaluator.release(hook).unwrap();
        for _ in 0..2 {
            let input = evaluator
                .import_environment(&data, ConversionLimits::default())
                .unwrap();
            assert_eq!(evaluator.print(input).unwrap(), "{:stable [42]}");
            evaluator.release(input).unwrap();
        }
    }

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
    #[cfg_attr(not(light_quickjs), allow(dead_code))]
    pub(crate) fn accepts(self, n: usize) -> bool {
        match self {
            Self::Exact(a) => n == a,
            Self::AtLeast(a) => n >= a,
            Self::Range { min, max } => n >= min && n <= max,
        }
    }
}
/// One engine's active callback scope, as a [`HostCall`] uses it. IDs it
/// returns are not yet owned by the callback.
pub(crate) trait HostScope {
    fn control(&self) -> ExecutionControl;
    /// The index of `id`, an error when it belongs to another session.
    #[cfg_attr(not(light_quickjs), allow(dead_code))]
    fn check(&self, id: ValueId) -> Result<u64, EvalError>;
    fn number(&self, value: ValueId) -> Result<f64, EvalError>;
    fn string(&self, value: ValueId) -> Result<String, EvalError>;
    fn print(&self, value: ValueId) -> Result<String, EvalError>;
    fn data(&self, value: ValueId) -> Result<Datum, EvalError>;
    fn value(&self, data: &Datum) -> Result<ValueId, EvalError>;
    fn invoke(&self, function: ValueId, args: &[ValueId]) -> Result<ValueId, EvalError>;
    fn release(&self, value: ValueId) -> Result<(), EvalError>;
    /// Run a compiler unit. When it throws, the scope keeps the exception, so
    /// that a callback failing with its error rethrows the original value.
    fn unit(&self, source: &str) -> Result<ValueId, EvalError>;
}
/// Engine-neutral view of one active callback. It never reacquires the engine
/// lock; recursive invocation uses the already active context. IDs created here
/// and argument IDs expire when the callback returns.
pub struct HostCall<'a, 'js> {
    scope: &'a dyn HostScope,
    owned: RefCell<Vec<ValueId>>,
    _engine: std::marker::PhantomData<&'js ()>,
}
impl<'a> HostCall<'a, '_> {
    #[cfg_attr(not(light_quickjs), allow(dead_code))]
    pub(crate) fn new(scope: &'a dyn HostScope) -> Self {
        Self {
            scope,
            owned: RefCell::new(Vec::new()),
            _engine: std::marker::PhantomData,
        }
    }
    /// The IDs the callback created and still owns.
    #[cfg_attr(not(light_quickjs), allow(dead_code))]
    pub(crate) fn into_owned(self) -> Vec<ValueId> {
        self.owned.into_inner()
    }
    fn own(&self, value: Result<ValueId, EvalError>) -> Result<ValueId, EvalError> {
        let value = value?;
        self.owned.borrow_mut().push(value);
        Ok(value)
    }
    pub fn number(&self, value: ValueId) -> Result<f64, EvalError> {
        self.scope.number(value)
    }
    pub fn string(&self, value: ValueId) -> Result<String, EvalError> {
        self.scope.string(value)
    }
    pub fn print(&self, value: ValueId) -> Result<String, EvalError> {
        self.scope.print(value)
    }
    pub fn data(&self, value: ValueId) -> Result<Datum, EvalError> {
        self.scope.data(value)
    }
    pub fn value(&self, data: &Datum) -> Result<ValueId, EvalError> {
        self.own(self.scope.value(data))
    }
    pub fn invoke(&self, function: ValueId, args: &[ValueId]) -> Result<ValueId, EvalError> {
        self.own(self.scope.invoke(function, args))
    }
    pub fn control(&self) -> ExecutionControl {
        self.scope.control()
    }
    pub(crate) fn release_scoped(&self, value: ValueId) -> Result<(), EvalError> {
        let mut owned = self.owned.borrow_mut();
        let index = owned
            .iter()
            .position(|id| *id == value)
            .ok_or_else(|| EvalError("value is not owned by this callback".into()))?;
        self.scope.release(value)?;
        owned.swap_remove(index);
        Ok(())
    }
    pub(crate) fn unit(&self, source: &str) -> Result<ValueId, EvalError> {
        self.own(self.scope.unit(source))
    }
}
/// The QuickJS scope of a host callback.
#[cfg(light_quickjs)]
struct QuickHost<'a, 'js> {
    active: ActiveEvaluator<'a, 'js>,
    unit_exception: RefCell<Option<rquickjs::Value<'js>>>,
}
#[cfg(light_quickjs)]
impl HostScope for QuickHost<'_, '_> {
    fn control(&self) -> ExecutionControl {
        self.active.control()
    }
    fn check(&self, id: ValueId) -> Result<u64, EvalError> {
        self.active.check(id)
    }
    fn number(&self, value: ValueId) -> Result<f64, EvalError> {
        self.active.number(value)
    }
    fn string(&self, value: ValueId) -> Result<String, EvalError> {
        self.active.string(value)
    }
    fn print(&self, value: ValueId) -> Result<String, EvalError> {
        self.active.print(value)
    }
    fn data(&self, value: ValueId) -> Result<Datum, EvalError> {
        self.active.export_data(value, ConversionLimits::default())
    }
    fn value(&self, data: &Datum) -> Result<ValueId, EvalError> {
        self.active.import_data(data, ConversionLimits::default())
    }
    fn invoke(&self, function: ValueId, args: &[ValueId]) -> Result<ValueId, EvalError> {
        self.active.call(function, args)
    }
    fn release(&self, value: ValueId) -> Result<(), EvalError> {
        self.active.release(value)
    }
    fn unit(&self, source: &str) -> Result<ValueId, EvalError> {
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
        result
    }
}

/// The JavaScript engine of an interactive session ([`crate::EvalSession`],
/// `--eval`, `--repl`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EvalEngine {
    /// The embedded QuickJS heap, the only engine of a build without V8
    /// (`--no-default-features`); a WebAssembly build has none, and its
    /// sessions fail to start.
    #[cfg_attr(not(light_v8), default)]
    QuickJs,
    /// A V8 isolate, the default where the build has V8.
    #[cfg(light_v8)]
    #[default]
    V8,
}
impl EvalEngine {
    /// The engine `name` (`quickjs` or `v8`) names, when this build has it.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "quickjs" => Ok(Self::QuickJs),
            #[cfg(light_v8)]
            "v8" => Ok(Self::V8),
            #[cfg(not(light_v8))]
            "v8" => Err("this light-cljs was built without V8; use --engine quickjs".into()),
            _ => Err(format!("unknown engine {name}; use quickjs or v8")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::QuickJs => "quickjs",
            #[cfg(light_v8)]
            Self::V8 => "v8",
        }
    }
}

/// The runtime heap of an interactive session: the operations
/// [`crate::EvalSession`] performs at an outer boundary, which QuickJS and V8
/// both provide. Language exceptions are ordinary errors here, and the heap
/// stays usable after them; resource limits and cancellation stay terminal.
pub(crate) trait SessionEvaluator {
    fn control(&self) -> ExecutionControl;
    fn begin_operation(&self, options: &EvaluatorOptions) -> Result<(), EvalError> {
        self.control().begin(options)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError>;
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        self.evaluate_unit(&format!("(rt) => ({expression}\n)"))
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError>;
    fn number(&self, id: ValueId) -> Result<f64, EvalError>;
    fn string(&self, id: ValueId) -> Result<String, EvalError>;
    fn print(&self, id: ValueId) -> Result<String, EvalError>;
    fn release(&self, id: ValueId) -> Result<(), EvalError>;
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError>;
    fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError>;
    fn validate(&self, id: ValueId) -> Result<(), EvalError>;
    /// Bind an application root, or with `core` a runtime core field.
    fn bind(&self, name: &str, id: ValueId, core: bool) -> Result<(), EvalError>;
    fn lookup(&self, name: &str, core: bool) -> Result<ValueId, EvalError>;
    /// The `ex-data` of the last exception, unless nil.
    fn error_data(&self) -> Option<Datum>;
    fn last_exception(&self) -> Result<ValueId, EvalError>;
    /// Run the pending promise jobs, as [`Evaluator::run_jobs`] describes.
    fn run_jobs(&self, keep: bool) -> Result<(), EvalError>;
    /// Take the promises rejected with no handler, as
    /// [`Evaluator::unhandled_rejections`] describes.
    fn unhandled_rejections(&self, limit: u32, raise: bool) -> Result<Vec<String>, EvalError>;
    fn input_provider(
        &self,
        name: &str,
        provider: crate::eval_session::Input,
    ) -> Result<ValueId, EvalError>;
    fn input_window(&self, more: bool) -> Result<(String, usize, bool, usize), EvalError>;
    fn input_action(&self, action: &str, units: usize) -> Result<(), EvalError>;
    fn install_host_function(
        &self,
        name: &str,
        arity: HostArity,
        callback: crate::eval_session::HostCallback,
    ) -> Result<ValueId, EvalError>;
    /// Live engine heap bytes, for retention measurements.
    fn memory_usage_bytes(&self) -> i64;
}
/// A fresh interactive heap of `engine`, with promise rejection tracking on.
pub(crate) fn session_evaluator(
    engine: EvalEngine,
    options: EvaluatorOptions,
) -> Result<Box<dyn SessionEvaluator>, EvalError> {
    match engine {
        #[cfg(light_quickjs)]
        EvalEngine::QuickJs => {
            let evaluator = Evaluator::interactive(options)?;
            evaluator.track_rejections();
            Ok(Box::new(evaluator))
        }
        #[cfg(not(light_quickjs))]
        EvalEngine::QuickJs => {
            let _ = options;
            Err(no_quickjs())
        }
        #[cfg(light_v8)]
        EvalEngine::V8 => Ok(Box::new(crate::v8_evaluator::V8Evaluator::interactive(
            options,
        )?)),
    }
}
#[cfg(light_quickjs)]
impl SessionEvaluator for Evaluator {
    fn control(&self) -> ExecutionControl {
        Evaluator::control(self)
    }
    fn begin_operation(&self, options: &EvaluatorOptions) -> Result<(), EvalError> {
        Evaluator::begin_operation(self, options)
    }
    fn evaluate_unit(&self, source: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_unit(self, source)
    }
    fn evaluate_js(&self, expression: &str) -> Result<ValueId, EvalError> {
        Evaluator::evaluate_js(self, expression)
    }
    fn call(&self, function: ValueId, arguments: &[ValueId]) -> Result<ValueId, EvalError> {
        Evaluator::call(self, function, arguments)
    }
    fn number(&self, id: ValueId) -> Result<f64, EvalError> {
        Evaluator::number(self, id)
    }
    fn string(&self, id: ValueId) -> Result<String, EvalError> {
        Evaluator::string(self, id)
    }
    fn print(&self, id: ValueId) -> Result<String, EvalError> {
        Evaluator::print(self, id)
    }
    fn release(&self, id: ValueId) -> Result<(), EvalError> {
        Evaluator::release(self, id)
    }
    fn import_data(&self, data: &Datum, limits: ConversionLimits) -> Result<ValueId, EvalError> {
        Evaluator::import_data(self, data, limits)
    }
    fn export_data(&self, id: ValueId, limits: ConversionLimits) -> Result<Datum, EvalError> {
        Evaluator::export_data(self, id, limits)
    }
    fn validate(&self, id: ValueId) -> Result<(), EvalError> {
        Evaluator::validate(self, id)
    }
    fn bind(&self, name: &str, id: ValueId, core: bool) -> Result<(), EvalError> {
        Evaluator::bind(self, name, id, core)
    }
    fn lookup(&self, name: &str, core: bool) -> Result<ValueId, EvalError> {
        Evaluator::lookup(self, name, core)
    }
    fn error_data(&self) -> Option<Datum> {
        Evaluator::error_data(self)
    }
    fn last_exception(&self) -> Result<ValueId, EvalError> {
        Evaluator::last_exception(self)
    }
    fn run_jobs(&self, keep: bool) -> Result<(), EvalError> {
        Evaluator::run_jobs(self, keep)
    }
    fn unhandled_rejections(&self, limit: u32, raise: bool) -> Result<Vec<String>, EvalError> {
        Evaluator::unhandled_rejections(self, limit, raise)
    }
    fn input_provider(
        &self,
        name: &str,
        provider: crate::eval_session::Input,
    ) -> Result<ValueId, EvalError> {
        Evaluator::input_provider(self, name, move |control| provider(control))
    }
    fn input_window(&self, more: bool) -> Result<(String, usize, bool, usize), EvalError> {
        Evaluator::input_window(self, more)
    }
    fn input_action(&self, action: &str, units: usize) -> Result<(), EvalError> {
        Evaluator::input_action(self, action, units)
    }
    fn install_host_function(
        &self,
        name: &str,
        arity: HostArity,
        callback: crate::eval_session::HostCallback,
    ) -> Result<ValueId, EvalError> {
        Evaluator::install_host_function(self, name, arity, callback)
    }
    fn memory_usage_bytes(&self) -> i64 {
        Evaluator::memory_usage_bytes(self)
    }
}

// Interrupted bridge imports must release every engine value exactly once.
// Regression coverage for the double free in the engine's `Array.prototype.find`
// cleanup that corrupted the heap during an interrupted form import.
#[cfg(test)]
mod bridge_interrupt_tests {
    use super::*;
    use crate::compiler_data::{DatumKind, Source};

    /// Compiler form whose decode builds nested array maps. Its inner map
    /// literals drive `Array.prototype.findIndex` through the bridge's map
    /// builder, which is where an engine interrupt used to leave the previous
    /// element double referenced.
    const BRIDGE_FORM: &str = r#"{:locals {x {:name x :kind :let} y {:name y :kind :arg :argument 0}} :ns {:name cljs.user} :context :expr :line 3 :column 7 :data [1 2.5 "str" \c nil true false sym ns/sym :kw :ns/kw #{1 2} (a b c) ^{:m 1} [x] #"re+" #uuid "00000000-0000-0000-0000-000000000000" #inst "2020-01-01" #js [1 2] #js {:a 1}]}"#;

    /// Warm-up loop length that places the engine's 10_000-poll interrupt
    /// counter so its next expiry lands on the loop-top poll of the second
    /// `findIndex` iteration inside the bridge's array-map builder. The engine
    /// is pinned exactly (rquickjs 0.14.0, quickjs-ng 0.16.2), so this is
    /// deterministic. After an engine update, run the ignored sweep below to
    /// locate the new value.
    const INTERRUPT_WARMUP: usize = 1_184;

    fn bridge_form(host: &mut crate::MacroHost) -> Form {
        host.read_compiler_form(&Source::new("interrupted_bridge.cljs", BRIDGE_FORM))
            .unwrap()
            .unwrap()
    }

    /// Import `form` once with the engine interrupt armed to fire at the first
    /// poll after the warm-up. The session is dropped before returning, so a
    /// heap corrupted by the interrupted import faults here.
    fn import_with_engine_interrupt(
        host: &crate::MacroHost,
        form: &Form,
        warmup: usize,
    ) -> (bool, Result<ValueId, EvalError>) {
        let evaluator = host.new_evaluator().unwrap();
        let control = evaluator.control();
        let armed = Arc::new(AtomicBool::new(false));
        let fired = Arc::new(AtomicBool::new(false));
        let (arm, seen) = (armed.clone(), fired.clone());
        evaluator
            .runtime
            .set_interrupt_handler(Some(Box::new(move || {
                if arm.swap(false, Ordering::Relaxed) {
                    seen.store(true, Ordering::Relaxed);
                    control.cancel();
                    true
                } else {
                    false
                }
            })));
        let shift = evaluator
            .evaluate_js(&format!(
                "(() => {{ for (let i = 0; i < {warmup}; i++); return 0; }})()"
            ))
            .unwrap();
        evaluator.release(shift).unwrap();
        armed.store(true, Ordering::Relaxed);
        let limits = ConversionLimits::default();
        let result = evaluator.import_form(form, limits);
        let fired = fired.load(Ordering::Relaxed);
        if fired {
            // Cancellation is terminal: a retry through the poisoned session
            // cannot turn the interruption into a successful import.
            assert!(evaluator.import_form(form, limits).is_err());
        }
        (fired, result)
    }

    #[test]
    fn interrupted_bridge_array_map_build_releases_each_value_once() {
        let mut host = crate::macro_host::MacroHost::new();
        host.evaluator_options.poll_budget = u64::MAX;
        let form = bridge_form(&mut host);

        let (fired, result) = import_with_engine_interrupt(&host, &form, INTERRUPT_WARMUP);
        assert!(fired, "engine interrupt did not fire at the pinned poll");
        let error = result.expect_err("interrupted import must not succeed");
        assert!(error.0.contains("cancelled"), "{error}");

        // The interrupted session above is dropped by now. A fresh session from
        // the same host must import and export the same form cleanly.
        let evaluator = host.new_evaluator().unwrap();
        let value = evaluator
            .import_form(&form, ConversionLimits::default())
            .unwrap();
        assert!(matches!(
            evaluator
                .export_data(value, ConversionLimits::default())
                .unwrap()
                .kind(),
            DatumKind::Collection { .. }
        ));
        evaluator.release(value).unwrap();
    }

    /// Wider interrupt-point sweep over the same bridge decode. It covers every
    /// poll position in a counter cycle, so it both locates the pinned warm-up
    /// above and re-checks that no other interrupted bridge cleanup releases a
    /// value twice. Run explicitly; optionally in parallel chunks:
    ///   cargo test --lib sweep_bridge_import_interrupt_points -- --ignored --nocapture
    ///   SWEEP_START=0 SWEEP_END=1257 cargo test --lib ...   (8 chunks cover 0..=10_050)
    #[test]
    #[ignore = "stress sweep; run explicitly to cover every interrupt point"]
    fn sweep_bridge_import_interrupt_points() {
        let mut host = crate::macro_host::MacroHost::new();
        host.evaluator_options.poll_budget = u64::MAX;
        let form = bridge_form(&mut host);
        let start: usize = std::env::var("SWEEP_START").map_or(0, |v| v.parse().unwrap());
        let end: usize = std::env::var("SWEEP_END").map_or(1_257, |v| v.parse().unwrap());
        let mut interrupts = 0;
        for warmup in start..end {
            let (fired, result) = import_with_engine_interrupt(&host, &form, warmup);
            if fired {
                interrupts += 1;
                let error = result.expect_err("interrupted import must not succeed");
                assert!(error.0.contains("cancelled"), "{error}");
            }
        }
        assert!(interrupts > 0, "no interrupt fired in {start}..{end}");
        eprintln!("sweep {start}..{end}: {interrupts} interrupts");
    }
}
