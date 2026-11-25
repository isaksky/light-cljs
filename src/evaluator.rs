//! Synchronous JavaScript execution. Engine values never cross this boundary.
//!
//! A session owns one heap and canonical runtime. Reusable hosts create fresh
//! sessions; interactive consumers may deliberately retain a session instead.
use crate::compiler_data::{Datum, Form};
use crate::form_bridge::ConversionLimits;
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
pub(crate) use host_clock::Instant;
use std::cell::RefCell;
#[cfg(not(all(target_arch = "wasm32", target_os = "unknown")))]
pub(crate) use std::time::Instant;
use std::{
    fmt,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, AtomicUsize, Ordering},
    },
    time::Duration,
};

/// The session clock on wasm32-unknown-unknown, where `std::time::Instant`
/// is unavailable: a host-supplied clock, microseconds since the Unix epoch,
/// which the embedder defines as `__light_cljs_host_now_us`. Other wasm
/// timing paths use it through `crate::evaluator::Instant`.
#[cfg(all(target_arch = "wasm32", target_os = "unknown"))]
mod host_clock {
    use std::time::Duration;

    unsafe extern "C" {
        fn __light_cljs_host_now_us() -> f64;
    }

    #[derive(Clone, Copy, Debug)]
    pub struct Instant(f64);

    impl Instant {
        pub fn now() -> Self {
            // SAFETY: a pure host import with no arguments; the embedder links
            // it or the module fails to instantiate.
            Self(unsafe { __light_cljs_host_now_us() })
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
/// The default macro memory limit (`--macro-memory-limit`), which is also
/// what a build's main isolate is allowed at least when several slots share
/// a larger limit ([`MemoryPool`]).
pub(crate) const DEFAULT_MEMORY_LIMIT: usize = 64 * 1024 * 1024;
/// What a build's macro memory limit must leave beyond the main engine's
/// reservation for each worker engine, and the least base a worker engine
/// is allowed ([`MemoryPool`]). At 32 MiB on the 200k project, collections
/// ran at the limit and the build took 1.6x as long with twice the
/// isolates (light-cljs-441).
pub(crate) const WORKER_SHARE: usize = 64 << 20;

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
            memory_limit: DEFAULT_MEMORY_LIMIT,
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
    /// Run `work`, compiler work no operation is charged for, such as
    /// compiling the process's bootstrap units: this control's deadline
    /// and poll budget stand still while it runs. Cancellation and a linked
    /// build's deadline still apply, and a failure met meanwhile stays, so
    /// a build with several isolates does such work before its deadline
    /// starts ([`crate::compiler_bootstrap::prepare`]).
    pub(crate) fn exempt<T>(&self, work: impl FnOnce() -> T) -> T {
        debug_assert!(
            self.0.build.is_none(),
            "exempt work in an isolate of a build would still spend the build's deadline"
        );
        let started = self.0.clock_origin.elapsed();
        let deadline = self.0.deadline_nanos.swap(u64::MAX, Ordering::Relaxed);
        let remaining = self.0.remaining.swap(u64::MAX, Ordering::Relaxed);
        let result = work();
        let spent =
            u64::try_from((self.0.clock_origin.elapsed() - started).as_nanos()).unwrap_or(u64::MAX);
        self.0
            .deadline_nanos
            .store(deadline.saturating_add(spent), Ordering::Relaxed);
        self.0.remaining.store(remaining, Ordering::Relaxed);
        result
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
    /// Whether an engine or callback failure invalidated the session.
    pub(crate) fn invalidated(&self) -> bool {
        self.0.terminal.load(Ordering::Relaxed) == 4
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

/// The macro memory limit of one build. The coordinator and its workers
/// have base shares and can grow into the shared headroom, which they give
/// back on destruction. Worker shares stay reserved while the pool can
/// still start.
#[derive(Debug)]
#[cfg_attr(not(light_v8), allow(dead_code))]
pub(crate) struct MemoryPool {
    base: usize,
    main: usize,
    keep: usize,
    headroom: AtomicUsize,
    #[cfg(light_v8)]
    reserved_workers: AtomicUsize,
}

#[cfg_attr(not(light_v8), allow(dead_code))]
impl MemoryPool {
    #[cfg(test)]
    pub(crate) fn new(limit: usize, isolates: usize) -> Arc<Self> {
        Self::with_worker_reservation(limit, isolates, true)
    }

    /// The pool of `limit` bytes for `isolates` isolates. Each worker's is
    /// allowed a base of half the even share, at least [`WORKER_SHARE`] and
    /// at most 256 MiB, but never more than the even share; the main isolate the
    /// same, but never less than a serial build with the default limit
    /// has, or the whole limit when it is lower (light-cljs-437), as far
    /// as the workers' bases leave it. What is left of the limit is
    /// headroom to grow into, of which a worker keeps an even part between
    /// jobs.
    fn with_worker_reservation(limit: usize, isolates: usize, reserve_workers: bool) -> Arc<Self> {
        let isolates = isolates.max(1);
        let base = (limit / (2 * isolates))
            .clamp(WORKER_SHARE, 256 << 20)
            .min(limit / isolates);
        let workers = base * (isolates - 1);
        let main = base
            .max(limit.min(DEFAULT_MEMORY_LIMIT))
            .min(limit - workers);
        let headroom = limit - workers - main;
        Arc::new(Self {
            base,
            main,
            keep: headroom / isolates,
            headroom: AtomicUsize::new(if reserve_workers {
                headroom
            } else {
                limit - main
            }),
            #[cfg(light_v8)]
            reserved_workers: AtomicUsize::new(if reserve_workers { workers } else { 0 }),
        })
    }
    /// The build was fully cached, so no worker will use these reservations.
    #[cfg(light_v8)]
    pub(crate) fn release_unused_worker_shares(&self) {
        self.give(self.reserved_workers.swap(0, Ordering::Relaxed));
    }
    /// The bytes each worker isolate is allowed before it takes headroom.
    pub(crate) fn base(&self) -> usize {
        self.base
    }
    /// The bytes the main isolate is allowed before it takes headroom.
    pub(crate) fn main_base(&self) -> usize {
        self.main
    }
    /// The fraction of planned headroom a worker can keep between jobs.
    /// Workers can reuse isolates whose steady needs exceed their base
    /// shares and return larger spikes at the end of the job (light-cljs-459).
    pub(crate) fn keep(&self) -> usize {
        self.keep
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

/// A build's deadline and cancellation, a linked control for workers,
/// and the memory pool for its macro isolates.
#[derive(Clone)]
#[cfg_attr(not(light_v8), allow(dead_code))]
pub(crate) struct BuildResources {
    pub(crate) control: ExecutionControl,
    pub(crate) workers: ExecutionControl,
    pub(crate) memory: Arc<MemoryPool>,
    pub(crate) isolates: usize,
}
impl BuildResources {
    /// The resources of a build with `options`, whose deadline starts now,
    /// for `isolates` planned isolates. Worker shares are reserved while
    /// the pool can still start.
    pub(crate) fn new(
        options: &EvaluatorOptions,
        isolates: usize,
        reserve_workers: bool,
    ) -> Result<Self, EvalError> {
        let control = ExecutionControl::new(options)?;
        Ok(Self {
            workers: ExecutionControl::linked(options, &control)?,
            control,
            memory: MemoryPool::with_worker_reservation(
                options.memory_limit,
                isolates,
                reserve_workers,
            ),
            isolates,
        })
    }
}

/// Opaque identity, valid only in its originating session, until release/drop.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ValueId {
    pub(crate) session: u64,
    pub(crate) index: u64,
}

#[cfg(light_v8)]
pub use crate::v8_evaluator::V8Evaluator as Evaluator;

/// A build without an embedded engine cannot create this evaluator. Compiler
/// macros can use a [`crate::host_evaluator::ScriptHost`]. Every constructor
/// reports the unavailable engine.
#[cfg(not(light_v8))]
pub enum Evaluator {}
#[cfg(not(light_v8))]
fn no_embedded_engine() -> EvalError {
    EvalError(
        "this light-cljs build embeds no JavaScript engine; use a V8 build or supply a ScriptHost for compiler macros".into(),
    )
}
#[cfg(not(light_v8))]
fn no_interactive_engine() -> EvalError {
    EvalError("interactive sessions require a V8 build of light-cljs".into())
}
#[cfg(not(light_v8))]
impl Evaluator {
    pub fn new(_: EvaluatorOptions) -> Result<Self, EvalError> {
        Err(no_embedded_engine())
    }
    pub fn interactive(_: EvaluatorOptions) -> Result<Self, EvalError> {
        Err(no_interactive_engine())
    }
    pub(crate) fn interactive_with_control(
        _: EvaluatorOptions,
        _: ExecutionControl,
    ) -> Result<Self, EvalError> {
        Err(no_interactive_engine())
    }
    pub fn export_data(&self, _: ValueId, _: ConversionLimits) -> Result<Datum, EvalError> {
        match *self {}
    }
    pub fn evaluate_unit(&self, _: &str) -> Result<ValueId, EvalError> {
        match *self {}
    }
    pub fn evaluate(&self, _: &crate::Program) -> Result<ValueId, EvalError> {
        match *self {}
    }
    pub fn print(&self, _: ValueId) -> Result<String, EvalError> {
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
    pub fn collect_garbage(&self) {
        match *self {}
    }
}
#[cfg(not(light_v8))]
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
/// One root registration of [`MacroEvaluator::register_compiler_roots`].
pub struct CompilerRoot<'a> {
    pub name: &'a str,
    pub unit: Option<usize>,
    pub once: bool,
    pub location: &'a str,
    pub test: Option<usize>,
}
/// Whether an engine's compile-time code has installed an extension that
/// environment data could reach: one of a core runtime protocol on a type,
/// or an instance of one, other than a source `deftype` or `defrecord`
/// (light-cljs-433; `noteEnvironmentExtension` in build.rs). Converting
/// environment data could call such an extension, so a source macro is
/// given only the entries it reads while it is [`Self::None`]: unknown
/// until a compilation prepares the engine to report it, and once noted it
/// stays so for the engine's life, as the extension does.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum EnvironmentExtensions {
    #[default]
    Untracked,
    None,
    Noted,
}
/// An engine's [`EnvironmentExtensions`], which its report service sets.
pub type EnvironmentTracking = std::rc::Rc<std::cell::Cell<EnvironmentExtensions>>;
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
    /// Register the declaration bundle of `unit` with its registration
    /// `source` (`BundleRecord::source`). An engine that demands units as
    /// dependency results also registers `unit` as one (light-cljs-478).
    fn register_compiler_bundle(&self, source: &str, unit: usize) -> Result<(), EvalError> {
        let _ = unit;
        let value = self.evaluate_unit(source)?;
        self.release(value)
    }
    /// Advance the engine's view of the compiler-owned [`crate::compilation_session::RootIndex`]
    /// to `cursor` registrations, `bundles` bundles, and any `new_protocols` extended since its
    /// last sync, without replaying individual root or bundle registrations.
    fn sync_catalog(
        &self,
        cursor: usize,
        bundles: usize,
        new_protocols: &[String],
    ) -> Result<(), EvalError> {
        let mut source = format!("rt => {{ globalThis.__lightSyncCatalog?.({cursor},{bundles},[");
        for (index, protocol) in new_protocols.iter().enumerate() {
            if index > 0 {
                source.push(',');
            }
            crate::source_map::json_string(&mut source, protocol);
        }
        source.push_str("]); return null; }");
        let value = self.evaluate_unit(&source)?;
        self.release(value)
    }
    /// Register `roots` in order, as one [`Self::register_compiler_root`]
    /// each would.
    fn register_compiler_roots(&self, roots: &[CompilerRoot<'_>]) -> Result<(), EvalError> {
        for root in roots {
            self.register_compiler_root(root.name, root.unit, root.once, root.location, root.test)?;
        }
        Ok(())
    }
    /// Whether this engine uses JavaScript reader namespace tables. Native
    /// form engines receive reader context in the macro environment instead.
    fn needs_reader_registration(&self) -> bool {
        true
    }
    /// For an engine without reader tables: the namespaces of a program,
    /// `namespaces` in its order, that a legacy engine's reader registration
    /// would create here ([`crate::reader_registration::register`]), which
    /// `all-ns`, `find-ns` and `the-ns` then see.
    fn register_namespaces(
        &self,
        _namespaces: &crate::arena::Arena<String>,
    ) -> Result<(), EvalError> {
        Ok(())
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
    /// What this engine has registered of the compilation's namespaces, for
    /// an engine that keeps it, so that a registration names only what it
    /// lacks ([`crate::reader_registration`]); without, every registration
    /// names every namespace of its program.
    fn reader_namespaces(
        &self,
    ) -> Option<std::rc::Rc<std::cell::RefCell<crate::reader_registration::ReaderNamespaces>>> {
        None
    }
    /// Whether this engine's compile-time code has installed an extension
    /// that environment data could reach ([`EnvironmentExtensions`]), for
    /// an engine that keeps it; without, source macros always get their
    /// whole environment.
    fn environment_extensions(&self) -> Option<EnvironmentTracking> {
        None
    }
    /// Make the compile-time runtime's `gensym` number in the scope of
    /// generated names `serial`, appending `prefix` and the scope's count,
    /// or with its counter for serial 0; with `native`, it takes each
    /// number of that count from the compiler (`__lightGensym`) rather than
    /// counting it itself ([`crate::generated_names::Scope`]).
    fn gensym_scope(
        &self,
        serial: u64,
        prefix: &str,
        retired: &str,
        native: bool,
    ) -> Result<(), EvalError> {
        debug_assert!(prefix.bytes().all(|b| b.is_ascii_digit() || b == b'_'));
        let value = self.evaluate_unit(&format!(
            "rt => {{ rt.compilerForms.gensymScope({serial}, \"{prefix}\", \"{retired}\", {native}); return null; }}"
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
#[cfg(all(test, light_v8))]
mod engine_tests {
    use super::*;
    use crate::{
        compiler_data::{DatumKind, ExpansionOrigin},
        compiler_evaluator::CompilerEvaluator,
    };
    use std::rc::Rc;

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
    fn frontier_service_can_reenter_an_active_compilation() {
        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let heap = evaluator.heap();
        evaluator
            .clone()
            .install_compiler_service(
                "nestedFrontier",
                Rc::new(move |active, next| {
                    assert_eq!(active.heap(), heap);
                    active
                        .compiler_frontier(Some(next.try_into().unwrap()))
                        .map_err(|e| e.0)?;
                    active.compiler_frontier(None).map_err(|e| e.0)?;
                    Ok("nested".into())
                }),
            )
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
        use crate::compiler_data::{Name, Source};
        use std::cell::Cell;

        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let c = crate::compiler_data::DataBuilder;
        let data = c
            .map(&[(
                c.keyword(crate::compiler_data::Name::new(None, "stable")),
                c.vector(&[c.integer(42)]).unwrap(),
            )])
            .unwrap();
        let nested_data = data.clone();
        let call = crate::MacroHost::new()
            .read_compiler_form(&Source::new("cache.cljs", "(reenter)"))
            .unwrap()
            .unwrap();
        let origin = ExpansionOrigin {
            macro_name: Name::new(None, "reenter"),
            call: call.span(),
            definition: None,
            helpers: vec![],
        };
        let observed = Rc::new(Cell::new(0));
        let calls = observed.clone();
        evaluator
            .clone()
            .install_compiler_service(
                "reenterEnvironment",
                Rc::new(move |active, _| {
                    let run = || -> Result<String, EvalError> {
                        calls.set(calls.get() + 1);
                        let limits = ConversionLimits::default();
                        let input = active.import_environment(&nested_data, limits)?;
                        let output = active.export_form(input, &call, origin.clone(), limits)?;
                        assert!(crate::form_bridge::exact(&output.data(), &nested_data));
                        active.release(input)?;
                        Ok("nested".into())
                    };
                    run().map_err(|e| e.0)
                }),
            )
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
        assert_eq!(observed.get(), 1);
    }

    #[test]
    fn unit_service_recurses_with_active_context_and_bridge() {
        use crate::compiler_data::{Name, Source};

        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let heap = evaluator.heap();
        let call = crate::MacroHost::new()
            .read_compiler_form(&Source::new("reentry.cljs", "(compile-nested)"))
            .unwrap()
            .unwrap();
        let origin = ExpansionOrigin {
            macro_name: Name::new(None, "compile-nested"),
            call: call.span(),
            definition: None,
            helpers: vec![],
        };
        evaluator
            .clone()
            .install_compiler_service(
                "compileNested",
                Rc::new(move |active, request| {
                    let run = || -> Result<String, EvalError> {
                        assert_eq!(active.heap(), heap);
                        let limits = ConversionLimits::default();
                        let input =
                            active.import_data(&Datum::new(DatumKind::Integer(41)), limits)?;
                        let identity = active.evaluate_unit("rt => x => x")?;
                        let result = active.call(identity, &[input])?;
                        let output = active.export_form(result, &call, origin.clone(), limits)?;
                        assert!(matches!(output.data().kind(), DatumKind::Integer(41)));
                        active.release(result)?;
                        active.release(identity)?;
                        active.release(input)?;
                        if request > 0 {
                            let nested = active.evaluate_unit(&format!(
                                "rt => compileNested({}) === '(rt) => 42'",
                                request - 1
                            ))?;
                            let output =
                                active.export_form(nested, &call, origin.clone(), limits)?;
                            assert!(matches!(output.data().kind(), DatumKind::Boolean(true)));
                            active.release(nested)?;
                        }
                        Ok("(rt) => 42".into())
                    };
                    run().map_err(|e| e.0)
                }),
            )
            .unwrap();
        let result = evaluator.evaluate_js("compileNested(3)").unwrap();
        assert_eq!(evaluator.string(result).unwrap(), "(rt) => 42");
        evaluator.release(result).unwrap();
        assert_eq!(Rc::strong_count(&evaluator), 1);
    }

    #[test]
    fn unit_service_cancellation_is_sticky_even_when_js_catches() {
        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        let control = evaluator.control();
        evaluator
            .clone()
            .install_compiler_service(
                "cancelCompile",
                Rc::new(move |_, _| {
                    control.cancel();
                    Ok("(rt) => 42".into())
                }),
            )
            .unwrap();
        let error = evaluator
            .evaluate_js("(() => { try { cancelCompile(0) } catch (_) {} return 1 })()")
            .unwrap_err();
        assert!(error.0.contains("cancelled"), "{error}");
        assert!(evaluator.evaluate_js("1").is_err());
    }

    #[test]
    fn unit_service_failure_is_sticky_even_when_js_catches() {
        let evaluator = Rc::new(Evaluator::new(EvaluatorOptions::default()).unwrap());
        evaluator
            .clone()
            .install_compiler_service("failCompile", Rc::new(|_, _| Err("compile failed".into())))
            .unwrap();
        let error = evaluator
            .evaluate_js("(() => { try { failCompile(0) } catch (_) {} return 1 })()")
            .unwrap_err();
        assert_eq!(error.0, "compile failed");
        assert!(evaluator.evaluate_js("1").is_err());
    }

    #[test]
    fn backtracking_deadline_is_terminal_and_fresh_operations_recover() {
        let host = crate::MacroHost::new();
        let evaluator = host.new_evaluator().unwrap();
        let short = EvaluatorOptions {
            timeout: Duration::from_millis(150),
            ..Default::default()
        };
        // Keep runtime startup outside the intentionally short regex window.
        evaluator.begin_operation(&short).unwrap();
        // The lookahead keeps the regexp on V8's backtracking engine.
        let work =
            "(()=>{try { /^(?=a)(a+)+$/.test('a'.repeat(30)+'!'); } catch(e) {} return 42;})()";
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
    #[cfg_attr(not(light_v8), allow(dead_code))]
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
    #[cfg_attr(not(light_v8), allow(dead_code))]
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
    #[cfg_attr(not(light_v8), allow(dead_code))]
    pub(crate) fn new(scope: &'a dyn HostScope) -> Self {
        Self {
            scope,
            owned: RefCell::new(Vec::new()),
            _engine: std::marker::PhantomData,
        }
    }
    /// The IDs the callback created and still owns.
    #[cfg_attr(not(light_v8), allow(dead_code))]
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
/// The JavaScript engine of an interactive session ([`crate::EvalSession`],
/// `--eval`, `--repl`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum EvalEngine {
    /// A V8 isolate.
    #[default]
    V8,
}
impl EvalEngine {
    /// Parse the engine name for this build.
    pub fn parse(name: &str) -> Result<Self, String> {
        match name {
            "quickjs" => Err("QuickJS was removed; use a V8 build with --engine v8".into()),
            #[cfg(light_v8)]
            "v8" => Ok(Self::V8),
            #[cfg(not(light_v8))]
            "v8" => Err(no_interactive_engine().0),
            _ => Err(format!("unknown engine {name}; use v8")),
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::V8 => "v8",
        }
    }
}

/// The runtime heap of an interactive session: the operations
/// [`crate::EvalSession`] performs at an outer boundary. The heap stays usable
/// after language exceptions. Resource limits and cancellation are terminal.
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
    /// Run the pending promise jobs, under the current operation limits.
    fn run_jobs(&self, keep: bool) -> Result<(), EvalError>;
    /// Take the promises rejected with no handler, since the last call.
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
        #[cfg(light_v8)]
        EvalEngine::V8 => Ok(Box::new(crate::v8_evaluator::V8Evaluator::interactive(
            options,
        )?)),
        #[cfg(not(light_v8))]
        EvalEngine::V8 => {
            let _ = options;
            Err(no_interactive_engine())
        }
    }
}
// Interrupted bridge imports must release every engine value exactly once.
// Regression coverage for the double free in the engine's `Array.prototype.find`
// cleanup that corrupted the heap during an interrupted form import.
#[cfg(all(test, light_v8))]
mod bridge_interrupt_tests {
    use super::*;
    use crate::{MacroHost, compiler_data::Source};
    use std::sync::mpsc;

    fn cancel_during_bridge(evaluator: &Evaluator, operation: impl Fn() -> Result<(), EvalError>) {
        let control = evaluator.control();
        let (entered, wait) = mpsc::channel();
        evaluator
            .install_callback("bridgeEntered", move |_, _| {
                entered
                    .send(())
                    .map_err(|error| EvalError(error.to_string()))?;
                Ok(String::new())
            })
            .unwrap();
        std::thread::scope(|scope| {
            let canceller = scope.spawn(move || {
                wait.recv_timeout(Duration::from_secs(5))
                    .expect("the bridge must reach the JavaScript hook");
                control.cancel();
            });
            let error = operation().unwrap_err();
            canceller.join().unwrap();
            assert!(error.0.contains("cancelled"), "{error}");
        });
        let error = operation().unwrap_err();
        assert!(error.0.contains("cancelled"), "{error}");
    }

    #[test]
    fn interrupted_bridge_array_map_import_leaves_fresh_heap_usable() {
        let mut host = MacroHost::new();
        let form = host
            .read_compiler_form(&Source::new(
                "interrupted_bridge.cljs",
                r#"{:locals {x {:name x :kind :let} y {:name y :kind :arg :argument 0}} :ns {:name cljs.user} :context :expr :line 3 :column 7 :data [1 2.5 "str" \c nil true false sym ns/sym :kw :ns/kw #{1 2} (a b c) ^{:m 1} [x] #"re+" #uuid "00000000-0000-0000-0000-000000000000" #inst "2020-01-01" #js [1 2] #js {:a 1}]}"#,
            ))
            .unwrap()
            .unwrap();
        let limits = ConversionLimits::default();
        let evaluator = host.new_evaluator().unwrap();
        // The bridge maps the descriptors of an array map's entries before
        // constructing the map. Previous children may already be decoded.
        let hook = evaluator
            .evaluate_js(
                r#"(() => {
            const map = Array.prototype.map;
            Array.prototype.map = function (...args) {
                if (this[0]?.[0] === 'keyword' && this[0][4] === 'name') {
                    bridgeEntered('');
                    for (;;) {}
                }
                return Reflect.apply(map, this, args);
            };
        })()"#,
            )
            .unwrap();
        evaluator.release(hook).unwrap();
        cancel_during_bridge(&evaluator, || {
            evaluator.import_form(&form, limits).map(|_| ())
        });
        drop(evaluator);

        let fresh = host.new_evaluator().unwrap();
        let value = fresh.import_form(&form, limits).unwrap();
        let output = fresh.export_data(value, limits).unwrap();
        assert!(crate::form_bridge::exact(&form.data(), &output));
        fresh.release(value).unwrap();
        assert_eq!(fresh.value_roots().unwrap(), 0);
    }

    #[test]
    fn interrupted_bridge_export_during_lazy_realization_leaves_fresh_heap_usable() {
        let host = MacroHost::new();
        let limits = ConversionLimits::default();
        let evaluator = host.new_evaluator().unwrap();
        let lazy = evaluator
            .evaluate_unit(
                r#"rt => rt.core.mapSeq(() => {
            bridgeEntered('');
            for (;;) {}
        }, rt.core.vector(1))"#,
            )
            .unwrap();
        cancel_during_bridge(&evaluator, || {
            evaluator.export_data(lazy, limits).map(|_| ())
        });
        drop(evaluator);

        let fresh = host.new_evaluator().unwrap();
        let value = fresh
            .evaluate_unit("rt => rt.core.mapSeq(x => x + 1, rt.core.vector(1))")
            .unwrap();
        let output = fresh.export_data(value, limits).unwrap();
        let expected = crate::compiler_data::DataBuilder
            .list(&[crate::compiler_data::DataBuilder.integer(2)])
            .unwrap();
        assert!(output.value_eq(&expected), "{output:?}");
        fresh.release(value).unwrap();
        assert_eq!(fresh.value_roots().unwrap(), 0);
    }
}

#[cfg(test)]
mod control_tests {
    use super::*;
    /// Exempt work spends neither the deadline nor the poll budget, and
    /// the control goes on with what it had left.
    #[test]
    fn exempt_work_leaves_the_deadline_and_poll_budget_as_they_were() {
        let control = ExecutionControl::new(&EvaluatorOptions {
            timeout: Duration::from_secs(1),
            poll_budget: 3,
            ..Default::default()
        })
        .unwrap();
        control.exempt(|| {
            std::thread::sleep(Duration::from_millis(1200));
            for _ in 0..10 {
                control.poll().unwrap();
            }
        });
        for _ in 0..3 {
            control.poll().unwrap();
        }
        let error = control.poll().unwrap_err();
        assert!(error.0.contains("poll budget exhausted"), "{error}");
        let control = ExecutionControl::new(&EvaluatorOptions {
            timeout: Duration::from_millis(1),
            ..Default::default()
        })
        .unwrap();
        control.exempt(|| std::thread::sleep(Duration::from_millis(5)));
        std::thread::sleep(Duration::from_millis(5));
        let error = control.poll().unwrap_err();
        assert!(error.0.contains("deadline exceeded"), "{error}");
    }
}
