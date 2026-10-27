//! Owned deferred analysis. No catalog lock survives analysis or engine entry.
//! Frames retain immutable definition-site resolution/macro snapshots. JS roots
//! have their own state machine; analysis cycles are diagnosed separately.
//!
//! The catalog of units is shared by every thread of a compilation
//! ([`SharedCatalog`]): one thread analyzes a unit, and every other thread
//! that demands it meanwhile waits for that analysis and shares its result
//! or its failure. What is bound to one compile-time engine, the engine and
//! the registrations installed in it, stays with the view of the thread that
//! owns the engine: [`CompilationSession`] on the coordinator, [`WorkerView`]
//! on a worker (docs/compiler-concurrency.md, "As built (light-cljs-335)").
use crate::{
    MacroHost, Program,
    analyzer::{AnalysisOptions, AnalysisWarning, NamespaceResolution},
    compiler_bootstrap::Bootstrap,
    compiler_data::{Datum, DatumKind, Form, Name},
    evaluator::MacroEvaluator,
    macro_host::{EngineBinding, MacroEnvironment},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError},
};

pub(crate) struct DefinitionSnapshot {
    pub namespace: String,
    pub aliases: Vec<(String, String)>,
    pub requires: Vec<String>,
    pub resolution: NamespaceResolution,
    pub resolution_program: Program,
    pub global_names: crate::name_index::NameIndex,
    pub namespace_globals: (usize, crate::arena::Arena<crate::analyzer::GlobalId>),
    /// The record layouts of `resolution_program` found so far.
    pub record_layouts: crate::analyzer::RecordLayouts,
    /// In an interactive session, its namespaces at definition time.
    pub session_namespaces: crate::analyzer::SessionNamespaces,
    pub options: AnalysisOptions,
    pub unchecked_arrays: bool,
    pub source_index: usize,
    pub definition_name: Option<String>,
    pub definition_test: Option<Form>,
    pub definition_test_unit: Option<usize>,
    pub extension_frontier: usize,
    /// The scope of generated names the unit is analyzed in, numbered in
    /// the scope it was registered from ([`crate::generated_names::child`]).
    pub names: Option<std::sync::Arc<str>>,
}
pub(crate) struct BundleRegistration<'a> {
    pub products: &'a [crate::analyzer::GlobalId],
    pub extensions: &'a [(String, Option<String>, String)],
    pub once: bool,
    pub form: Form,
    pub snapshot: DefinitionSnapshot,
    pub host: MacroHost,
    pub location: String,
    pub analyzed: Option<Program>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct VarId(usize);
#[derive(Clone, Copy, Debug)]
struct UnitRequest {
    var: VarId,
    generation: usize,
}
struct Unit {
    request: UnitRequest,
    name: String,
    /// The source index of the namespace that defined it, on which
    /// `light.build` records what the unit reads.
    source: u32,
    // Consumed by the one analysis. The analyzed result keeps any resolution
    // data it needs; these inputs must not pin an obsolete second snapshot.
    // Keep large inputs out of line so completed units retain no empty space
    // for the definition snapshot or the macro host's many shared tables.
    inputs: Option<Box<UnitInputs>>,
    state: AnalysisState,
    analyses: usize,
    // Count actual engine demand separately from pure source preparation.
    // Ready or failed JavaScript roots never initialize a second time.
    emitted: bool,
    /// How many threads wait for its analysis, and while one does after
    /// the application took the analyzed program, a strong reference to
    /// it, so that every waiter gets it ([`CompilationSession::release_application_ir`]).
    waiters: u32,
    retained: Option<Arc<Program>>,
    /// Whether its form was found macro-free when it was registered, so
    /// that any thread may analyze it with no compile-time engine
    /// ([`CompilationSession::set_macro_free`]).
    macro_free: bool,
    /// Whether the drain gives it to the build's unit pool
    /// ([`CompilationSession::set_pooled`]).
    pooled: bool,
    /// Whether its namespace's dispatch keeps it for the drain point, which
    /// charges it: until then a demand takes its analysis and leaves its
    /// charges (light-cljs-394, [`CompilationSession::hold_units`]).
    held: bool,
    /// Whether its namespace's dispatch took it, so that its drain point
    /// charges it: a demand while another unit is recorded, on any thread,
    /// records it on its own, as a worker that started it first would have,
    /// rather than in that recording (light-cljs-442).
    dispatched: bool,
    /// What its macro-free analysis counted, until the coordinator
    /// charges it ([`UnitCharges`]), and how long the analysis took.
    charges: Option<Box<UnitCharges>>,
    time: std::time::Duration,
}
/// What a macro-free unit's analysis counts, on whichever thread it ran:
/// it runs with counters and marks of its own, and the coordinator charges
/// them to the compilation's when it first takes the analysis, checking the
/// limits as each native expansion would have where the unit is charged,
/// so that limits, their errors and `--stats` do not depend on `--jobs`.
pub(crate) struct UnitCharges {
    events: Vec<crate::macro_host::UnitEvent>,
    macro_depth: usize,
    analysis_depth: usize,
    /// The unit's expansion and generated-node limits.
    limits: (usize, usize),
    /// The stack depth at which a demand for the unit fails
    /// ([`SharedCatalog::claim`]).
    depth_limit: usize,
    /// `source:line:column: unit` of its form, which its failure message
    /// begins with.
    location: String,
    /// What it observed while a dependency manifest is recorded, and the
    /// names `gensym` gave in its worker's engine while artifacts are
    /// captured, which the coordinator adds to the compilation's where it
    /// charges the unit (light-cljs-374).
    observations: Option<Box<crate::dependency_manifest::ObservationLog>>,
    gensyms: Vec<Vec<u16>>,
    /// What a worker's `--stats-file` trace recorded of the unit and the
    /// units it demanded, which the coordinator records where it charges
    /// the unit (light-cljs-369, [`crate::compile_trace::replay`]).
    trace: Option<Box<crate::compile_trace::WorkerRows>>,
}
impl UnitCharges {
    /// Add the charges to `binding`'s counters and marks, and spend the
    /// unit's polls from `control`, the execution control its analysis on
    /// the charging thread would have polled; the unit was analyzed at
    /// `depth` of that thread's stack, and with no control, `start` starts
    /// the engine whose control it is at the first JavaScript expansion.
    /// The first poll, expansion or start that fails fails the unit, with
    /// the message and diagnostic its analysis there would have given.
    fn charge(
        &self,
        binding: &EngineBinding,
        depth: usize,
        mut control: Option<crate::evaluator::ExecutionControl>,
        start: &dyn Fn() -> Result<Option<crate::evaluator::ExecutionControl>, String>,
        session: &CompilationSession,
    ) -> Result<(), (String, crate::AnalysisError)> {
        use crate::macro_host::UnitEvent;
        use crate::macro_host::WorkMarks;
        WorkMarks::note(&binding.work.unit_depth, depth + 1);
        WorkMarks::note(&binding.work.macro_depth, self.macro_depth);
        WorkMarks::note(&binding.work.analysis_depth, self.analysis_depth);
        let (expansions, nodes) = (&binding.expansions, &binding.generated_nodes);
        let fail = |span: &std::ops::Range<usize>, message: String| {
            let diagnostic = crate::AnalysisError {
                span: span.clone(),
                kind: crate::AnalysisErrorKind::MacroExpansion(message),
            };
            Err((format!("{}: {diagnostic}", self.location), diagnostic))
        };
        for event in &self.events {
            match event {
                // A session an engine failure invalidated, such as a later
                // top-level pass's that the drain point comes after
                // (light-cljs-391), fails that expansion, not the polls
                // charged after it.
                UnitEvent::Poll(_) if control.as_ref().is_some_and(|c| c.invalidated()) => {}
                UnitEvent::Poll(span) => {
                    if let Some(Err(error)) = control.as_ref().map(|control| control.poll()) {
                        return fail(span, error.to_string());
                    }
                }
                UnitEvent::Expansion(expansion) => {
                    let exceeded = if expansions.get() >= self.limits.0 {
                        Some("compiler macro expansion limit exceeded")
                    } else {
                        expansions.set(expansions.get() + 1);
                        nodes.set(nodes.get().saturating_add(expansion.nodes));
                        (nodes.get() > self.limits.1)
                            .then_some("compiler macro generated-node limit exceeded")
                    };
                    if let Some(message) = exceeded {
                        return fail(
                            &expansion.span,
                            format!("while expanding {}: {message}", expansion.operator),
                        );
                    }
                }
                // The limits as `MacroHost::expand_javascript` checks them,
                // failing as the analyzer reports them at the call it
                // expanded.
                UnitEvent::Script {
                    nodes: generated,
                    span,
                    site,
                } => {
                    let fail = |message: &str| match site {
                        Some((operator, span)) => {
                            fail(span, format!("while expanding {operator}: {message}"))
                        }
                        None => fail(span, message.into()),
                    };
                    // A start that fails is the expansion's failure, and
                    // leaves no unspent control to skip later polls with.
                    if control.is_none() {
                        control = match start() {
                            Ok(control) => control,
                            Err(error) => return fail(&error),
                        };
                    }
                    if expansions.get() >= self.limits.0 {
                        return fail("compiler macro expansion limit exceeded");
                    }
                    expansions.set(expansions.get() + 1);
                    nodes.set(nodes.get().saturating_add(*generated));
                    if nodes.get() > self.limits.1 {
                        return fail("compiler macro generated-node limit exceeded");
                    }
                }
                // What capture records, where a serial build records it.
                UnitEvent::Room(extra) => {
                    WorkMarks::note(&binding.work.node_peak, nodes.get().saturating_add(*extra));
                }
                UnitEvent::Conversion(peak) => {
                    WorkMarks::note(&binding.work.conversion_peak, *peak);
                }
                UnitEvent::Unmeasured => binding.work.unmeasured.set(true),
                UnitEvent::Captured(expansion) => session.capture_expansion((**expansion).clone()),
            }
        }
        Ok(())
    }
}
/// What any thread analyzes a unit from: its form, its definition snapshot
/// and the sendable part of the host it was registered with. The thread
/// that registered it keeps the host's engine binding.
struct UnitInputs {
    form: Form,
    snapshot: DefinitionSnapshot,
    environment: MacroEnvironment,
}
#[derive(Clone)]
enum AnalysisState {
    Unstarted,
    /// Being analyzed by the thread with this slot in [`Catalog::threads`].
    Analyzing(usize),
    Analyzed(Arc<Program>, Vec<AnalysisWarning>),
    /// Released to its application owner, which holds the analyzed program
    /// while it copies it; the compile-time JavaScript stays.
    Prepared(
        std::sync::Weak<Program>,
        Vec<AnalysisWarning>,
        Arc<crate::emitter::DeferredUnit>,
    ),
    Failed(String, Option<crate::AnalysisError>),
}
#[derive(Clone)]
pub(crate) enum Registration {
    Root(String, Option<usize>, Option<usize>, bool, String),
    Bundle(Arc<BundleRecord>),
}
/// A declaration bundle's registration, kept as data: its JavaScript names
/// the bundle's unit ID, which an artifact records as a relocatable reference.
pub(crate) struct BundleRecord {
    /// The products, by `ns/name`.
    pub products: Vec<String>,
    pub unit: usize,
    pub extensions: Vec<(String, Option<String>, String)>,
    pub location: String,
    pub once: bool,
    pub group: bool,
}
/// Whether converting canonical environment data into the compile-time
/// engine, or hashing, comparing and looking up the values it builds, could
/// demand (initialize) the bundle that registers `extensions` or call one of
/// its methods (light-cljs-433). The compile-time runtime demands a bundle
/// only for a protocol it dispatches on a value whose native kind or core
/// type an index's selector names, or for any value when the selector is
/// `null` (a computed or instance target) or `_` (`default`)
/// (`setDeclarationDemand` in compiler_roots.js), and a ready bundle's
/// methods run only through those dispatches. Environment data holds only
/// nil, booleans, numbers, strings, characters, symbols, keywords, lists,
/// vectors, maps, sets, metadata maps and the compiler literals (regex,
/// instant, uuid, queue, JavaScript arrays and objects), and building or
/// hashing them dispatches only core protocols on those and on other core
/// types (seqs, map entries, nodes). So an index cannot be reached when it
/// is a multimethod's (core data operations call no multimethod), when its
/// protocol is a source `defprotocol` outside `cljs.core` (core code knows
/// no such protocol), or when its selector names a type outside `cljs.core`,
/// a user `deftype` or `defrecord`, of which environment data holds no
/// instance. Every other index counts: a `null`, `_`, `object` or other
/// native selector, or any `cljs.core` type.
pub(crate) fn reaches_environment_data(extensions: &[(String, Option<String>, String)]) -> bool {
    extensions.iter().any(|(protocol, selector, _)| {
        if protocol.starts_with("multifn:") {
            return false;
        }
        // Builtin protocols are keyed without a namespace (`IEquiv`,
        // `Object`); a source protocol by its `ns/Name`.
        if protocol
            .split_once('/')
            .is_some_and(|(namespace, _)| namespace != "cljs.core")
        {
            return false;
        }
        match selector.as_deref().map(|s| s.strip_prefix('@')) {
            None | Some(None) => true,
            Some(Some(name)) => name.starts_with("cljs.core/"),
        }
    })
}
impl BundleRecord {
    /// The unit that registers the bundle in the compile-time session.
    pub fn source(&self) -> String {
        fn quoted(s: &str) -> String {
            let mut out = String::from("\"");
            for c in s.encode_utf16() {
                out.push_str(&format!("\\u{c:04x}"));
            }
            out.push('"');
            out
        }
        let names = self
            .products
            .iter()
            .map(|name| quoted(name))
            .collect::<Vec<_>>()
            .join(",");
        let indices = self
            .extensions
            .iter()
            .map(|(p, t, identity)| {
                format!(
                    "[{},{},{}]",
                    quoted(p),
                    t.as_deref().map_or("null".into(), quoted),
                    quoted(identity)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let unit = self.unit;
        let code = format!(
            "(() => {{ let code; return rt => {{ code ??= (0,eval)(globalThis.__lightCompileUnit({unit})); return code(rt); }}; }})()"
        );
        format!(
            "rt => globalThis.__lightRegisterBundle([{names}], {code}, [{indices}], {}, {}, {}, {unit})",
            quoted(&self.location),
            self.once,
            self.group
        )
    }
}
#[derive(Default)]
struct Catalog {
    units: Vec<Unit>,
    active: HashMap<String, bool>,
    // Only active defmulti products have stable named demand keys.
    multifn_targets: HashMap<String, String>,
    bindings: HashMap<String, (VarId, usize)>,
    /// Every registration in order; each engine installs them in this order
    /// and records how far it got in its own view.
    registrations: Vec<Registration>,
    bundle_count: usize,
    /// How many of those bundles could be demanded or called by converting
    /// or looking up environment data ([`reaches_environment_data`]).
    environment_bundles: usize,
    /// By the slot each view joined with ([`SharedCatalog::join`]).
    threads: Vec<AnalysisThread>,
    /// How many threads wait for a unit another thread analyzes.
    waiting: usize,
    /// Why the unit pool stopped, which ends every wait.
    abandoned: Option<String>,
    /// `light.build` requests workers forwarded to the coordinator, and
    /// its answers, by ticket ([`SharedCatalog::forward_build`]).
    requests: std::collections::VecDeque<BuildRequest>,
    replies: Vec<(u64, String, Option<Box<BuildLog>>)>,
    next_ticket: u64,
    /// With `--stats`, the analyzed unit IR waiting to merge
    /// ([`CompilationSession::measure_queued_ir`]).
    queued_ir: QueuedIr,
}
/// The analyzed unit IR of a compilation measured with `--stats` that
/// waits to merge into its program: from when a unit's analysis settles,
/// on whichever thread, while the catalog holds it for the drain to take
/// and while its namespace holds the drained unit for
/// [`crate::analyzer::NamespaceAnalysis::merge_front`], until it merges.
#[derive(Default)]
struct QueuedIr {
    /// Each unit's estimate while it waits, by unit ID
    /// ([`crate::program_fragment::SharedPrefix::added_bytes`]).
    held: Vec<usize>,
    bytes: usize,
    units: usize,
    peak: crate::compile_stats::QueuedIrPeak,
}
impl QueuedIr {
    fn hold(&mut self, id: usize, bytes: usize) {
        if self.held.len() <= id {
            self.held.resize(id + 1, 0);
        }
        if self.held[id] == 0 && bytes > 0 {
            self.held[id] = bytes;
            self.bytes += bytes;
            self.units += 1;
            self.peak.bytes = self.peak.bytes.max(self.bytes);
            self.peak.units = self.peak.units.max(self.units);
        }
    }
    fn merged(&mut self, id: usize) {
        if let Some(held) = self.held.get_mut(id)
            && *held > 0
        {
            self.bytes -= std::mem::take(held);
            self.units -= 1;
        }
    }
}
struct BuildRequest {
    ticket: u64,
    text: String,
    analysis: Vec<u32>,
}
/// How a thread demands a unit ([`analyze_unit`]): whether the pool was
/// given it, so that this thread waits for it while it is queued; whether
/// this thread is a pool worker, whose analyses record their charges; and
/// how it answers the workers' `light.build` requests while it waits.
#[derive(Clone, Copy)]
struct Demand<'a> {
    queued: bool,
    on_worker: bool,
    /// Whether the coordinator analyzes the unit at its namespace's
    /// dispatch, recording its charges for the drain point
    /// ([`CompilationSession::analyze_held`]).
    held: bool,
    server: Option<BuildServer<'a>>,
    help: Option<&'a dyn Fn() -> Result<bool, String>>,
}
/// What the coordinator does with a unit's analysis once it has it
/// ([`CompilationSession::analyze_in`]).
#[derive(Clone, Copy, PartialEq, Eq)]
enum Take {
    /// Charge what it recorded now, where the coordinator demands it, unless
    /// its namespace's dispatch holds it for the drain point
    /// ([`Take::Defer`]).
    Charge,
    /// Keep what it recorded for its drain point (light-cljs-391).
    Hold,
    /// A demand of a unit held for its drain point: keep what it recorded,
    /// recording it if no thread has analyzed it, as [`Take::Hold`] does
    /// but for its `gensym` names, which the running macro's engine cannot
    /// be asked for (light-cljs-394).
    Defer,
    /// Charge what it recorded now, at its namespace's drain point.
    Drain,
    /// Take only what it observed now, at its namespace's settle point,
    /// and keep its charges for its drain point (light-cljs-394,
    /// [`CompilationSession::observe_pooled`]).
    Observe,
}
/// How the coordinator answers a forwarded `light.build` request: its text
/// and the sources the worker's analyses are of.
type BuildServer<'a> = &'a dyn Fn(&str, Vec<u32>) -> (String, Option<Box<BuildLog>>);
/// What answering a forwarded `light.build` request recorded.
type BuildLog = crate::dependency_manifest::ObservationLog;
/// A thread's analyses: the units it analyzes, the innermost last, each
/// demanded by the one below it, and the unit it waits for.
#[derive(Default)]
struct AnalysisThread {
    stack: Vec<usize>,
    waiting: Option<usize>,
}
/// The units of one compilation, shared by the threads that analyze them.
/// One thread analyzes each unit, and any other that demands it meanwhile
/// waits for that analysis. The lock is held for bookkeeping only: no
/// analysis, engine entry or large destructor runs under it.
#[derive(Default)]
pub(crate) struct SharedCatalog {
    #[cfg(all(test, feature = "bbp-macros"))]
    on_native_submit: Mutex<Option<NativeSubmitHook>>,
    #[cfg(all(test, feature = "bbp-macros"))]
    on_native_run: Mutex<Option<NativeHook>>,
    catalog: Mutex<Catalog>,
    /// Notified when a unit stops being analyzed.
    settled: Condvar,
    /// What the build's unit pool did, for `--stats`.
    counters: PoolCounters,
    /// The compilation's expansion and generated-node counts as the
    /// coordinator last charged them, in canonical order: a lower bound of
    /// what precedes any unit it has not charged yet. A unit recorded off
    /// the coordinator counts from there, so a limit its own checks reach
    /// is one the serial build reaches too, at or before that point; this
    /// soft cap stops runaway work early, and the charge decides the
    /// diagnostic ([`UnitCharges`]).
    charged: [std::sync::atomic::AtomicUsize; 2],
    /// What the build's isolates share when its pool runs engines: its
    /// deadline, cancellation and memory ([`crate::evaluator::BuildResources`]).
    resources: std::sync::OnceLock<crate::evaluator::BuildResources>,
    /// Source-order effect lanes for compile-time atoms on the native
    /// backend ([`crate::effect_lanes`], light-cljs-480).
    lanes: Arc<crate::effect_lanes::Lanes>,
    #[cfg(feature = "bbp-macros")]
    native_units: std::sync::atomic::AtomicBool,
    /// Source-wide core writes, indexed before macro factories are emitted.
    /// Deferred helper bodies must affect their callers' cache eligibility.
    #[cfg(feature = "bbp-macros")]
    native_replaced_core: Mutex<std::collections::HashSet<&'static str>>,
    #[cfg(feature = "bbp-macros")]
    native_runtime:
        std::sync::OnceLock<Result<crate::bbp_evaluator::BbpRuntime, crate::evaluator::EvalError>>,
    /// One more than the lowest unit ID of a pool job whose analysis
    /// failed, or 0: the drain stops at that unit, so no job after it in
    /// unit order is needed and none starts ([`Self::admits`]).
    failed_job: std::sync::atomic::AtomicUsize,
    /// What the compilation records of each analysis beyond its charges,
    /// which a unit recorded off the coordinator hands it
    /// ([`crate::macro_host::UnitRecording`]); set when the pool starts.
    recording: std::sync::OnceLock<crate::macro_host::UnitRecording>,
    /// Whether [`Catalog::queued_ir`] is measured: only with `--stats`, so
    /// that a build without reads one relaxed load per unit.
    measuring_ir: std::sync::atomic::AtomicBool,
    /// The build's test-only perturbation ([`crate::scheduler::Perturbation`]):
    /// the unit pool's scheduling, and the unit whose analysis panics.
    #[cfg(debug_assertions)]
    perturbation: std::sync::OnceLock<crate::scheduler::Perturbation>,
    #[cfg(test)]
    on_claim: Mutex<Option<ClaimHook>>,
    /// A test's unbounded channel: report each new waiter under the catalog
    /// lock, before waiting or helping releases it.
    #[cfg(test)]
    wait_registered: Mutex<Option<std::sync::mpsc::Sender<usize>>>,
    /// Runs once on a waiting thread before it consumes a completed analysis,
    /// without either the catalog lock or the hook's mutex.
    #[cfg(test)]
    on_wake: Mutex<Option<ClaimHook>>,
}
#[cfg(all(test, feature = "bbp-macros"))]
type NativeHook = Arc<dyn Fn(big_boy_pants_js::TaskId, usize) + Send + Sync>;
#[cfg(all(test, feature = "bbp-macros"))]
type NativeSubmitHook = Arc<dyn Fn(big_boy_pants_js::TaskId, usize, bool) + Send + Sync>;
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<SharedCatalog>();
};
/// The unit pool's counts ([`crate::compile_stats::DeferredUnitCounts`]).
#[derive(Default)]
struct PoolCounters {
    pooled: std::sync::atomic::AtomicUsize,
    isolates: std::sync::atomic::AtomicUsize,
    startup_ns: std::sync::atomic::AtomicU64,
    unit_ns: std::sync::atomic::AtomicU64,
    initializations: std::sync::atomic::AtomicUsize,
    /// The pool's worker threads and how long it ran.
    workers: std::sync::atomic::AtomicUsize,
    /// Its workers with a compile-time engine, and the units the ones
    /// without analyzed (light-cljs-437).
    engine_workers: std::sync::atomic::AtomicUsize,
    engine_less_units: std::sync::atomic::AtomicUsize,
    pool_ns: std::sync::atomic::AtomicU64,
    /// What the pool's scheduler did, once it has stopped.
    scheduler: Mutex<crate::scheduler::PoolCounts>,
    /// Acquisitions of the catalog's lock that found it held.
    catalog_contended: std::sync::atomic::AtomicU64,
}
impl PoolCounters {
    fn add(counter: &std::sync::atomic::AtomicU64, since: std::time::Instant) {
        let elapsed = u64::try_from(since.elapsed().as_nanos()).unwrap_or(u64::MAX);
        counter.fetch_add(elapsed, std::sync::atomic::Ordering::Relaxed);
    }
}
/// What a demand for a unit gets from the catalog.
enum Claim {
    /// The demanding thread analyzes the unit, at this depth of its stack;
    /// with its name and whether it is macro-free.
    Analyze(Box<UnitInputs>, usize, String, bool),
    /// The unit was too deep in the thread's stack and has failed.
    TooDeep(Box<UnitInputs>, usize),
    /// Another analysis's result.
    Reuse(Arc<Program>, Vec<AnalysisWarning>),
}
/// Runs on the thread that claimed a unit, before it analyzes the unit.
#[cfg(test)]
type ClaimHook = Arc<dyn Fn(usize) + Send + Sync>;
const DEPTH_LIMIT: &str = "compiler macro expansion depth limit exceeded in deferred units";
/// Why a wait for a pooled unit after a failed one ends ([`SharedCatalog::claim`]).
const SKIPPED_AFTER_FAILURE: &str = "an earlier unit in unit order failed";
impl SharedCatalog {
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn native_counts(&self) -> crate::compile_stats::NativeHelperCounts {
        self.native_runtime
            .get()
            .and_then(|r| r.as_ref().ok())
            .map_or_default(|r| r.helper_counts())
    }
    /// The compilation's effect lanes.
    pub(crate) fn lanes(&self) -> &Arc<crate::effect_lanes::Lanes> {
        &self.lanes
    }
    /// Give `units` their lane positions ([`crate::effect_lanes::Lanes::schedule`]);
    /// with `forced` (a cache blocker's consumer), each waits for its lane
    /// before it starts.
    pub(crate) fn schedule_lanes(&self, units: &[usize], forced: bool) {
        self.lanes.schedule(units.iter().copied(), forced);
        let finished: Vec<(usize, bool)> = {
            let c = self.lock();
            (units.iter())
                .filter_map(|&id| match c.units.get(id).map(|u| &u.state) {
                    Some(AnalysisState::Failed(..)) => Some((id, true)),
                    Some(AnalysisState::Analyzed(..) | AnalysisState::Prepared(..)) => {
                        Some((id, false))
                    }
                    _ => None,
                })
                .collect()
        };
        for (id, failed) in finished {
            self.lanes.finish(id, failed);
        }
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn select_native(&self) {
        self.native_units
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn native_replaced_core(&self) -> Vec<&'static str> {
        self.native_replaced_core
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .copied()
            .collect()
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn has_native_core_function_writes(&self) -> bool {
        let replaced = self
            .native_replaced_core
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // Match native emission's cache eligibility. Dynamic Vars have value
        // type `any` and are always read at their use sites, so rebinding them
        // cannot invalidate a prepared factory's cached core functions.
        crate::CoreFnId::ALL.into_iter().any(|core| {
            replaced.contains(core.runtime_name())
                && core.operation().value == crate::core_operations::Type::FN
        })
    }

    /// Conservatively recognize core targets by spelling, including those in
    /// quoted/generated forms and bodies whose analysis is still deferred.
    /// A shadowed bare name may disable an optimization, never enable one.
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn index_native_core_writes(&self, source: &str, syntax: &crate::SyntaxTree) {
        use crate::parser::{CollectionKind, NodeId, NodeKind, PrefixKind};
        if !self.native_units.load(std::sync::atomic::Ordering::Relaxed) {
            return;
        }
        let spelling = |id: NodeId| &source[syntax.nodes[id.0].span.clone()];
        let mutator = |name: &str| {
            matches!(
                name.rsplit('/').next(),
                Some("set!" | "binding" | "with-redefs")
            )
        };
        // Qualified aliases and bare renamed refers can name a core target.
        // Reading symbol pairs conservatively also accepts ordinary data maps;
        // a false positive merely keeps another core read dynamic.
        let mut renamed = HashMap::<&str, Vec<&'static str>>::new();
        for node in &syntax.nodes {
            if let NodeKind::Collection {
                kind: CollectionKind::Map,
                children,
            } = &node.kind
            {
                for pair in children.as_chunks::<2>().0 {
                    if pair
                        .iter()
                        .all(|id| matches!(syntax.nodes[id.0].kind, NodeKind::Symbol))
                        && let Some(core) = crate::CoreFnId::resolve(spelling(pair[0]))
                    {
                        renamed
                            .entry(spelling(pair[1]))
                            .or_default()
                            .push(core.runtime_name());
                    }
                }
            }
        }
        let mut replaced = self
            .native_replaced_core
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        // A first-class mutation operator can assemble a helper's write with
        // a computed target. Its eventual IR does not exist when earlier
        // macro factories are emitted, so no core binding is proven stable.
        let mutation_heads: std::collections::HashSet<usize> = syntax
            .nodes
            .iter()
            .filter_map(|node| {
                if let NodeKind::Collection {
                    kind: CollectionKind::List,
                    children,
                } = &node.kind
                {
                    children
                        .first()
                        .filter(|&&head| mutator(spelling(head)))
                        .map(|head| head.0)
                } else {
                    None
                }
            })
            .collect();
        if syntax.nodes.iter().enumerate().any(|(index, node)| {
            matches!(node.kind, NodeKind::Symbol)
                && mutator(spelling(NodeId(index)))
                && !mutation_heads.contains(&index)
        }) {
            replaced.extend(
                crate::CoreFnId::ALL
                    .into_iter()
                    .map(crate::CoreFnId::runtime_name),
            );
            return;
        }
        let mut target = |mut id: NodeId| {
            loop {
                if let NodeKind::Collection {
                    kind: CollectionKind::List,
                    children,
                } = &syntax.nodes[id.0].kind
                    && children.len() > 1
                    && (spelling(children[0]).starts_with(".-")
                        || matches!(spelling(children[0]), "." | "aget" | "cljs.core/aget"))
                {
                    id = children[1];
                    continue;
                }
                match syntax.nodes[id.0].kind {
                    NodeKind::Prefix {
                        kind: PrefixKind::Unquote | PrefixKind::UnquoteSplicing,
                        ..
                    } => {
                        replaced.extend(
                            crate::CoreFnId::ALL
                                .into_iter()
                                .map(crate::CoreFnId::runtime_name),
                        );
                        return;
                    }
                    NodeKind::Metadata { target, .. } | NodeKind::Prefix { form: target, .. } => {
                        id = target
                    }
                    _ => break,
                }
            }
            let name = spelling(id);
            let name = name
                .split_once('/')
                .filter(|(namespace, _)| !namespace.is_empty())
                .map_or(name, |(_, member)| member);
            let name = name.strip_prefix("cljs.core.").unwrap_or(name);
            let name = name.split_once('.').map_or(name, |(binding, _)| binding);
            if let Some(core) = crate::CoreFnId::resolve(name) {
                replaced.insert(core.runtime_name());
            }
            if let Some(names) = renamed.get(name) {
                replaced.extend(names.iter().copied());
            }
        };
        for node in &syntax.nodes {
            let NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } = &node.kind
            else {
                continue;
            };
            let Some((&head, args)) = children.split_first() else {
                continue;
            };
            let head = spelling(head);
            let head = head.rsplit('/').next().unwrap_or(head);
            match (head, args) {
                ("set!", [place, ..]) => target(*place),
                ("binding" | "with-redefs", [bindings, ..]) => {
                    if let NodeKind::Collection {
                        kind: CollectionKind::Vector,
                        children,
                    } = &syntax.nodes[bindings.0].kind
                    {
                        for &place in children.iter().step_by(2) {
                            target(place);
                        }
                    }
                }
                // Raw JavaScript can assign any core value passed as a hole.
                ("js*", [_, holes @ ..]) => holes.iter().copied().for_each(&mut target),
                _ => (),
            }
        }
    }
    #[cfg(feature = "bbp-macros")]
    pub(crate) fn native_runtime(
        &self,
        options: &crate::evaluator::EvaluatorOptions,
    ) -> Result<crate::bbp_evaluator::BbpRuntime, crate::evaluator::EvalError> {
        let runtime = self
            .native_runtime
            .get_or_init(|| {
                let runtime = crate::bbp_evaluator::BbpRuntime::new(options)?;
                runtime.set_lanes(self.lanes.clone());
                Ok(runtime)
            })
            .clone();
        // One runtime per compilation: a later demand cannot change or reset
        // the aggregate budget the first one built it with.
        if let Ok(runtime) = &runtime {
            debug_assert_eq!(runtime.memory_limit(), options.memory_limit);
        }
        runtime
    }

    fn lock(&self) -> MutexGuard<'_, Catalog> {
        match self.catalog.try_lock() {
            Ok(catalog) => catalog,
            Err(std::sync::TryLockError::Poisoned(poisoned)) => poisoned.into_inner(),
            Err(std::sync::TryLockError::WouldBlock) => {
                let contended = &self.counters.catalog_contended;
                contended.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                self.catalog.lock().unwrap_or_else(PoisonError::into_inner)
            }
        }
    }
    /// Note that the coordinator has charged `expansions` and `nodes`
    /// ([`Self::charged`]).
    pub(crate) fn raise_charged(&self, expansions: usize, nodes: usize) {
        use std::sync::atomic::Ordering::Relaxed;
        self.charged[0].fetch_max(expansions, Relaxed);
        self.charged[1].fetch_max(nodes, Relaxed);
    }
    /// Whether the pool may start job `id`: not when a job earlier in
    /// unit order has failed, since the coordinator's drain, which takes
    /// the pool's units in that order, stops there and reports it
    /// (docs/compiler-concurrency.md, "As built (light-cljs-341)").
    fn admits(&self, id: usize) -> bool {
        let failed = self.failed_job.load(std::sync::atomic::Ordering::Relaxed);
        failed == 0 || id < failed
    }
    /// Wake the threads waiting for a pool job that will not start
    /// ([`Self::admits`]), which [`Self::claim`] then fails.
    fn skipped(&self) {
        let _c = self.lock();
        self.settled.notify_all();
    }
    /// The pool jobs of `units`: with `mixed`, in a pool whose workers
    /// past the first ones have no engine, a unit not found macro-free
    /// is for the ones with an engine only ([`ENGINE_PHASE`]).
    fn unit_jobs(&self, units: Vec<usize>, mixed: bool) -> Vec<UnitJob> {
        let catalog = self.lock();
        (units.into_iter())
            .map(|id| {
                let macro_free = catalog.units[id].macro_free;
                UnitJob::Analyze(
                    id,
                    if mixed && !macro_free {
                        ENGINE_PHASE
                    } else {
                        0
                    },
                    macro_free,
                )
            })
            .collect()
    }
    /// Note that pool job `id` ended with its unit failed, if it did.
    #[allow(deprecated)] // fetch_update supports the stable compiler used in CI.
    fn note_job(&self, id: usize) {
        if matches!(self.lock().units[id].state, AnalysisState::Failed(..)) {
            use std::sync::atomic::Ordering::Relaxed;
            let _ = self.failed_job.fetch_update(Relaxed, Relaxed, |failed| {
                (failed == 0 || id + 1 < failed).then_some(id + 1)
            });
        }
    }
    /// Whether the catalog measures the IR waiting to merge (`--stats`).
    fn measuring_ir(&self) -> bool {
        self.measuring_ir.load(std::sync::atomic::Ordering::Relaxed)
    }
    /// [`CompilationSession::merged_ir`] on any thread.
    fn merged_ir(&self, id: usize) {
        if self.measuring_ir() {
            self.lock().queued_ir.merged(id);
        }
    }
    /// What the build's isolates share, once its pool runs engines.
    pub(crate) fn resources(&self) -> Option<&crate::evaluator::BuildResources> {
        self.resources.get()
    }
    /// What the compilation records of each analysis beyond its charges.
    fn recording(&self) -> crate::macro_host::UnitRecording {
        self.recording.get().copied().unwrap_or_default()
    }
    /// The expansions and generated nodes the coordinator has charged.
    fn charged(&self) -> (usize, usize) {
        use std::sync::atomic::Ordering::Relaxed;
        (self.charged[0].load(Relaxed), self.charged[1].load(Relaxed))
    }
    /// A slot for a new view's thread.
    fn join(&self) -> usize {
        let mut c = self.lock();
        c.threads.push(AnalysisThread::default());
        c.threads.len() - 1
    }
    /// Demand unit `id` for the thread in slot `thread`: claim it if no
    /// thread has analyzed it, wait if another analyzes it, and reuse what
    /// the analysis gave if it is done.
    ///
    /// With `queued`, the unit was given to the pool and this thread waits
    /// for a worker to analyze it even while it is unstarted: the
    /// coordinator never analyzes a pooled unit in its own isolate, whose
    /// compile-time state would then depend on scheduling.
    ///
    /// While it waits, the coordinator serves its `light.build` requests
    /// that workers forward with `server` ([`Self::forward_build`]).
    #[cfg(test)]
    fn claim(
        &self,
        thread: usize,
        id: usize,
        queued: bool,
        server: Option<BuildServer<'_>>,
    ) -> Result<Claim, String> {
        self.claim_help(thread, id, queued, server, None)
    }
    fn claim_help(
        &self,
        thread: usize,
        id: usize,
        queued: bool,
        server: Option<BuildServer<'_>>,
        help: Option<&dyn Fn() -> Result<bool, String>>,
    ) -> Result<Claim, String> {
        // Dropped after the lock is released.
        let mut _released = None;
        // The trace's frame for the time this thread blocks here.
        let mut _blocked = None;
        // This thread's wait for the unit, which its analysis inherits.
        let mut _lane_wait: Option<crate::effect_lanes::WaitGuard> = None;
        let mut guard = self.lock();
        loop {
            if let Some(serve) = server
                && let Some(request) = guard.requests.pop_front()
            {
                drop(guard);
                let (reply, log) = serve(&request.text, request.analysis);
                guard = self.lock();
                guard.replies.push((request.ticket, reply, log));
                self.settled.notify_all();
                continue;
            }
            let c = &mut *guard;
            let state = &c.units.get(id).ok_or("unknown compilation unit")?.state;
            let waits = match *state {
                AnalysisState::Analyzing(owner) => owner != thread,
                AnalysisState::Unstarted => queued,
                _ => false,
            };
            // A pooled unit after one that failed in unit order never
            // starts ([`Self::admits`]): its namespace's analysis stops,
            // and the earlier failure, whose drain point comes first, is
            // the build's (light-cljs-395).
            let skipped = matches!(*state, AnalysisState::Unstarted) && !self.admits(id);
            if waits && (skipped || c.abandoned.is_some()) {
                if c.threads[thread].waiting.take().is_some() {
                    c.waiting -= 1;
                    c.units[id].waiters -= 1;
                }
                return Err(c
                    .abandoned
                    .clone()
                    .unwrap_or_else(|| SKIPPED_AFTER_FAILURE.to_owned()));
            }
            if waits {
                // The unit's analysis inherits this thread's lane position
                // while it waits (light-cljs-480).
                if _lane_wait.is_none() {
                    drop(guard);
                    _lane_wait = Some(self.lanes.wait_for(crate::effect_lanes::Target::Unit(id)));
                    guard = self.lock();
                    continue;
                }
                if c.threads[thread].waiting.is_none() {
                    if let Some(path) = c.wait_cycle(thread, id) {
                        return Err(format!("compile dependency cycle: {path}"));
                    }
                    c.threads[thread].waiting = Some(id);
                    c.waiting += 1;
                    c.units[id].waiters += 1;
                    #[cfg(test)]
                    if let Some(waiting) = &*self.wait_registered.lock().unwrap() {
                        waiting.send(id).unwrap();
                    }
                }
                if _blocked.is_none() {
                    _blocked = Some(crate::compile_trace::wait(id));
                }
                if let Some(help) = help {
                    drop(guard);
                    let helped = help();
                    guard = self.lock();
                    match helped {
                        Ok(true) => continue,
                        Err(error) => {
                            if guard.threads[thread].waiting.take().is_some() {
                                guard.waiting -= 1;
                                guard.units[id].waiters -= 1;
                            }
                            return Err(error);
                        }
                        Ok(false) => {}
                    }
                    // Recheck after releasing the lock to help: a producer
                    // may have settled while this worker ran another job.
                    if !matches!(guard.units[id].state, AnalysisState::Analyzing(_)) {
                        continue;
                    }
                    if let Some(wait) = &_lane_wait {
                        drop(guard);
                        wait.refresh();
                        guard = self.lock();
                    }
                    guard = self
                        .settled
                        .wait_timeout(guard, std::time::Duration::from_millis(20))
                        .unwrap_or_else(PoisonError::into_inner)
                        .0;
                    continue;
                }
                guard = self
                    .settled
                    .wait(guard)
                    .unwrap_or_else(PoisonError::into_inner);
                continue;
            }
            #[cfg(test)]
            if c.threads[thread].waiting.is_some() {
                drop(guard);
                // Both condition-variable waits and helping can finish an
                // analysis wait. Gate consumption on either path, and never
                // reacquire the catalog while holding the hook's mutex.
                let hook = self.on_wake.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(id);
                }
                guard = self.lock();
            }
            #[cfg(test)]
            let c = &mut *guard;
            if c.threads[thread].waiting.take().is_some() {
                c.waiting -= 1;
                let unit = &mut c.units[id];
                unit.waiters -= 1;
                if unit.waiters == 0 {
                    _released = unit.retained.take();
                }
            }
            match &c.units[id].state {
                AnalysisState::Analyzed(program, warnings) => {
                    return Ok(Claim::Reuse(program.clone(), warnings.clone()));
                }
                AnalysisState::Prepared(program, warnings, _) => {
                    return program
                        .upgrade()
                        .map(|program| Claim::Reuse(program, warnings.clone()))
                        .ok_or_else(|| "application IR was already transferred".to_owned());
                }
                AnalysisState::Failed(error, _) => return Err(error.clone()),
                // Met again on this thread's own stack.
                AnalysisState::Analyzing(_) => {
                    let mut path = c.threads[thread]
                        .stack
                        .iter()
                        .map(|&i| c.units[i].name.clone())
                        .collect::<Vec<_>>();
                    path.push(c.units[id].name.clone());
                    return Err(format!("compile dependency cycle: {}", path.join(" -> ")));
                }
                AnalysisState::Unstarted => (),
            }
            let depth = c.threads[thread].stack.len();
            let unit = &mut c.units[id];
            let inputs = unit.inputs.take().expect("unstarted unit inputs");
            if depth >= inputs.environment.limits().expansion_depth.min(64) {
                unit.state = AnalysisState::Failed(DEPTH_LIMIT.to_owned(), None);
                // The coordinator may wait for a pooled unit a worker fails.
                if c.waiting > 0 {
                    self.settled.notify_all();
                }
                return Ok(Claim::TooDeep(inputs, depth));
            }
            unit.state = AnalysisState::Analyzing(thread);
            unit.analyses += 1;
            let (name, macro_free) = (unit.name.clone(), unit.macro_free);
            // The request has stable identity even if a newer same-kind source
            // generation is activated while this frame is suspended.
            debug_assert!(unit.request.generation > 0);
            debug_assert_eq!(c.bindings[&unit.name].0, unit.request.var);
            c.threads[thread].stack.push(id);
            return Ok(Claim::Analyze(inputs, depth, name, macro_free));
        }
    }
    /// End the analysis of unit `id` that the thread in slot `thread` claimed
    /// with `state`, or as interrupted, and wake the threads waiting for it;
    /// `held` is the analyzed IR's estimated bytes when
    /// [`Self::measuring_ir`], or 0.
    fn settle(&self, thread: usize, id: usize, state: Option<AnalysisState>, held: usize) {
        let (previous, waiting) = {
            let mut c = self.lock();
            let c = &mut *c;
            if held > 0 && matches!(state, Some(AnalysisState::Analyzed(..))) {
                c.queued_ir.hold(id, held);
            }
            let popped = c.threads[thread].stack.pop();
            debug_assert_eq!(popped, Some(id));
            let unit = &mut c.units[id];
            let previous = matches!(unit.state, AnalysisState::Analyzing(_)).then(|| {
                std::mem::replace(
                    &mut unit.state,
                    state.unwrap_or_else(|| {
                        AnalysisState::Failed("analysis interrupted".into(), None)
                    }),
                )
            });
            (previous, c.waiting > 0)
        };
        let failed = previous.is_some() && self.unit_failed_now(id);
        drop(previous);
        self.lanes.finish(id, failed);
        if waiting {
            self.settled.notify_all();
        }
    }
    fn unit_failed_now(&self, id: usize) -> bool {
        matches!(self.lock().units[id].state, AnalysisState::Failed(..))
    }
    /// Serve `light.build` request `text` of a worker whose analyses are
    /// of the sources `analysis` on the coordinator, which alone has the
    /// build's resources and log, and wait for the answer; the coordinator
    /// answers while it waits for units ([`Self::claim`]).
    fn forward_build(&self, text: &str, analysis: Vec<u32>) -> (String, Option<Box<BuildLog>>) {
        let ticket = {
            let mut c = self.lock();
            let ticket = c.next_ticket;
            c.next_ticket += 1;
            c.requests.push_back(BuildRequest {
                ticket,
                text: text.to_owned(),
                analysis,
            });
            ticket
        };
        self.settled.notify_all();
        let _blocked = crate::compile_trace::wait(0);
        let mut c = self.lock();
        loop {
            if let Some(at) = c.replies.iter().position(|(t, _, _)| *t == ticket) {
                let (_, reply, log) = c.replies.swap_remove(at);
                return (reply, log);
            }
            if let Some(message) = &c.abandoned {
                return (format!("e{message}"), None);
            }
            c = self.settled.wait(c).unwrap_or_else(PoisonError::into_inner);
        }
    }
    /// Fail unit `id` with `message` if no thread has started it: a worker
    /// could not make its engine.
    fn fail_unstarted(&self, id: usize, message: String) {
        let waiting = {
            let mut c = self.lock();
            let unit = &mut c.units[id];
            if !matches!(unit.state, AnalysisState::Unstarted) {
                return;
            }
            unit.inputs = None;
            unit.state = AnalysisState::Failed(message, None);
            c.waiting > 0
        };
        self.lanes.finish(id, true);
        if waiting {
            self.settled.notify_all();
        }
    }
    /// End every wait with `message`: the pool stopped, so units it was
    /// given may never be analyzed.
    pub(crate) fn abandon(&self, message: String) {
        self.lock().abandoned.get_or_insert(message);
        self.settled.notify_all();
        self.lanes.abandon();
    }
    /// How many extension bundles the compilation registered.
    pub(crate) fn extension_frontier(&self) -> usize {
        self.lock().bundle_count
    }
    /// How many of them [`reaches_environment_data`].
    pub(crate) fn environment_bundles(&self) -> usize {
        self.lock().environment_bundles
    }
    /// The unit `id`'s initializer, prepared, unless it is not yet.
    fn prepared(&self, id: usize) -> Result<Option<Arc<crate::emitter::DeferredUnit>>, String> {
        let c = self.lock();
        match &c.units.get(id).ok_or("unknown compilation unit")?.state {
            AnalysisState::Prepared(_, _, prepared) => Ok(Some(prepared.clone())),
            _ => Ok(None),
        }
    }
    fn set_emitted(&self, id: usize) {
        self.lock().units[id].emitted = true;
    }
    fn record_charges(&self, id: usize, charges: UnitCharges, time: std::time::Duration) {
        let mut c = self.lock();
        c.units[id].charges = Some(Box::new(charges));
        c.units[id].time = time;
    }
    /// Unit `id`'s charges, once ([`UnitCharges`]).
    fn take_charges(&self, id: usize) -> Option<Box<UnitCharges>> {
        self.lock().units.get_mut(id)?.charges.take()
    }
    /// What unit `id` observed, once, leaving its other charges.
    fn take_observations(
        &self,
        id: usize,
    ) -> Option<Box<crate::dependency_manifest::ObservationLog>> {
        self.lock()
            .units
            .get_mut(id)?
            .charges
            .as_mut()?
            .observations
            .take()
    }
    /// Whether unit `id` waits for its namespace's drain point to be
    /// charged ([`CompilationSession::hold_units`]).
    fn held(&self, id: usize) -> bool {
        self.lock().units.get(id).is_some_and(|unit| unit.held)
    }
    /// Whether unit `id`'s namespace's dispatch took it
    /// ([`CompilationSession::hold_units`]), even if its drain point has
    /// begun.
    fn dispatched(&self, id: usize) -> bool {
        self.lock()
            .units
            .get(id)
            .is_some_and(|unit| unit.dispatched)
    }
    fn release_hold(&self, id: usize) {
        if let Some(unit) = self.lock().units.get_mut(id) {
            unit.held = false;
        }
    }
    /// Fail unit `id`, analyzed, as its charges did.
    fn fail(&self, id: usize, message: String, diagnostic: Option<crate::AnalysisError>) {
        let previous = std::mem::replace(
            &mut self.lock().units[id].state,
            AnalysisState::Failed(message, diagnostic),
        );
        drop(previous);
        self.lanes.finish(id, true);
    }
    fn unit_name(&self, id: usize) -> String {
        self.lock().units[id].name.clone()
    }
    /// The source indexes of the units the thread in slot `thread`
    /// analyzes, the innermost last.
    fn analysis_sources(&self, thread: usize) -> Vec<u32> {
        let c = self.lock();
        c.threads[thread]
            .stack
            .iter()
            .filter_map(|&unit| Some(c.units.get(unit)?.source))
            .collect()
    }
    fn unit_source(&self, id: usize) -> Option<u32> {
        Some(self.lock().units.get(id)?.source)
    }
}
impl Catalog {
    /// If the thread in slot `thread` waiting for unit `id` would close a
    /// cycle of waits, the cycle's units in demand order from the one with
    /// the lowest source index, and then that one again. Each analysis
    /// thread waits for one unit at most, so the walk follows one chain:
    /// the owner of `id`, the unit its thread waits for, and so on.
    fn wait_cycle(&self, thread: usize, id: usize) -> Option<String> {
        let mut cycle = Vec::new();
        let mut unit = id;
        for _ in 0..self.threads.len() {
            let AnalysisState::Analyzing(owner) = self.units[unit].state else {
                return None;
            };
            // Each unit from the waited one up its owner's stack demanded the
            // next; the innermost demanded what the owner waits for.
            let stack = &self.threads[owner].stack;
            let at = stack.iter().position(|&u| u == unit)?;
            cycle.extend_from_slice(&stack[at..]);
            if owner == thread {
                let first = (0..cycle.len())
                    .min_by_key(|&i| (self.units[cycle[i]].source, cycle[i]))
                    .unwrap_or(0);
                cycle.rotate_left(first);
                cycle.push(cycle[0]);
                let names = cycle
                    .iter()
                    .map(|&u| self.units[u].name.as_str())
                    .collect::<Vec<_>>();
                return Some(names.join(" -> "));
            }
            unit = self.threads[owner].waiting?;
        }
        None
    }
}
/// The registrations a compile-time engine has installed: per engine, and
/// so per thread.
#[derive(Default)]
struct InstalledRegistrations {
    bootstrap: Option<Bootstrap>,
    /// How many of the catalog's registrations `bootstrap` holds.
    installed: usize,
}
/// The coordinator's view of the catalog: the shared units, with the engine
/// of the coordinator's thread and the engine bindings of the hosts it
/// registered units with.
pub(crate) struct CompilationSession {
    shared: Arc<SharedCatalog>,
    /// This view's slot in the catalog's analysis threads.
    thread: usize,
    engine: RefCell<InstalledRegistrations>,
    /// By unit ID, the engine binding of the host the unit was registered
    /// with, until its analysis takes a frame of it.
    unit_bindings: RefCell<Vec<Option<Box<EngineBinding>>>>,
    /// While namespace artifacts are captured ([`crate::artifact_capture`]),
    /// what the catalog does not otherwise keep about its units and macros.
    capture: RefCell<Option<SessionCapture>>,
    /// `gensym` names reserved for installed namespaces before the session
    /// started ([`crate::generated_names`]), which it reserves when it starts.
    gensyms_pending: RefCell<Vec<Vec<u16>>>,
    /// The `gensym` counter reserved before the session started.
    gensym_floor: Cell<u64>,
    /// Whether capture reads what `gensym` returns.
    gensyms_logged: Cell<bool>,
    /// What `gensym` returned since the last [`Self::take_gensyms`], in a
    /// serial build's order, once a pool worker's unit added its names
    /// ([`Self::absorb_gensyms`]), and every name taken so far, which a
    /// worker's helper initializer, run again in its isolate, gives again.
    gensym_buffer: RefCell<Vec<Vec<u16>>>,
    gensyms_seen: RefCell<std::collections::HashSet<Vec<u16>>>,
    /// The build's unit pool, with `--jobs N` ([`UnitPool`]).
    pool: RefCell<Option<UnitPool>>,
}
/// What capture needs beyond the catalog, recorded only while it is on.
#[derive(Default)]
struct SessionCapture {
    /// By unit ID from `first_unit` on.
    units: Vec<UnitCapture>,
    first_unit: usize,
    macros: Vec<CapturedMacro>,
    expansions: Vec<CapturedExpansion>,
    /// By unit ID, in the order drain points charged them, the expansions
    /// each unit charged there ([`CompilationSession::take_unit_charges`]).
    charges: Vec<(usize, Vec<crate::macro_host::UnitEvent>)>,
}
/// A source macro call analysis expanded, for an artifact's provenance.
#[derive(Clone, Debug)]
pub(crate) struct CapturedExpansion {
    /// The macro, `ns/name`, and its generation when it ran.
    pub key: String,
    pub generation: usize,
    pub definition_source: String,
    pub definition: std::ops::Range<usize>,
    pub call: std::ops::Range<usize>,
    pub source_index: usize,
}
#[derive(Default)]
struct UnitCapture {
    /// The globals of the unit's definition snapshot.
    view_globals: usize,
    extension_frontier: usize,
    /// The program's globals the unit's own analysis added.
    added: Vec<usize>,
}
/// A `defmacro`'s compile-time function, as the macro host evaluated it.
pub(crate) struct CapturedMacro {
    pub key: String,
    pub code: String,
    pub view_globals: usize,
    /// Its namespace's reader context as its registration names it.
    pub reader_context: Option<crate::analyzer::RuntimeReaderNamespace>,
}
/// A unit as capture reads it from the catalog.
pub(crate) struct UnitRecord {
    pub name: String,
    pub generation: usize,
    pub analyses: usize,
    pub emitted: bool,
    /// The prepared compile-time JavaScript, or why there is none.
    pub code: Result<String, &'static str>,
    pub view_globals: usize,
    pub extension_frontier: usize,
    pub added: Vec<usize>,
    /// The reader contexts its prepared registration names.
    pub reader_contexts: Option<
        crate::shared::Shared<
            crate::layered::Layered<
                String,
                crate::shared::Shared<crate::analyzer::RuntimeReaderNamespace>,
            >,
        >,
    >,
}
/// A registration of an installed namespace, with this compilation's unit IDs
/// ([`CompilationSession::install`]).
pub(crate) enum InstalledRegistration {
    Root {
        name: String,
        unit: Option<usize>,
        test: Option<usize>,
        once: bool,
        location: String,
    },
    Bundle(Arc<BundleRecord>),
}
/// A registration as capture reads it from the catalog.
pub(crate) enum RegistrationRecord {
    Root {
        name: String,
        unit: Option<usize>,
        test: Option<usize>,
        once: bool,
        location: String,
    },
    Bundle(Arc<BundleRecord>),
}
impl Default for CompilationSession {
    fn default() -> Self {
        let shared = Arc::new(SharedCatalog::default());
        let thread = shared.join();
        Self {
            shared,
            thread,
            engine: Default::default(),
            unit_bindings: Default::default(),
            capture: Default::default(),
            gensyms_pending: Default::default(),
            gensym_floor: Default::default(),
            gensyms_logged: Default::default(),
            gensym_buffer: Default::default(),
            gensyms_seen: Default::default(),
            pool: Default::default(),
        }
    }
}
impl Drop for CompilationSession {
    fn drop(&mut self) {
        // What this thread logged for the session is not read again.
        if self.gensyms_logged.get() {
            crate::generated_names::stop_log();
        }
    }
}
impl CompilationSession {
    /// The catalog this view shares with the views of other threads.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn shared(&self) -> &Arc<SharedCatalog> {
        &self.shared
    }
    /// Record what namespace artifacts need from now on.
    pub fn begin_capture(&self) {
        let first_unit = self.shared.lock().units.len();
        *self.capture.borrow_mut() = Some(SessionCapture {
            first_unit,
            ..Default::default()
        });
    }
    pub fn capturing(&self) -> bool {
        self.capture.borrow().is_some()
    }
    /// How many units, registrations and bundles the catalog holds.
    pub fn capture_marks(&self) -> (usize, usize, usize) {
        let c = self.shared.lock();
        (c.units.len(), c.registrations.len(), c.bundle_count)
    }
    pub fn capture_macro(
        &self,
        key: String,
        code: String,
        view_globals: usize,
        reader_context: Option<crate::analyzer::RuntimeReaderNamespace>,
    ) {
        if let Some(capture) = self.capture.borrow_mut().as_mut() {
            capture.macros.push(CapturedMacro {
                key,
                code,
                view_globals,
                reader_context,
            });
        }
    }
    pub fn capture_expansion(&self, expansion: CapturedExpansion) {
        if let Some(capture) = self.capture.borrow_mut().as_mut() {
            capture.expansions.push(expansion);
        }
    }
    /// The macros declared and the source macro calls expanded since the
    /// last call.
    pub fn take_captured(&self) -> (Vec<CapturedMacro>, Vec<CapturedExpansion>) {
        self.capture
            .borrow_mut()
            .as_mut()
            .map(|capture| {
                (
                    std::mem::take(&mut capture.macros),
                    std::mem::take(&mut capture.expansions),
                )
            })
            .unwrap_or_default()
    }
    /// Keep, while capturing, the expansions and scripts of `events` that
    /// a drain point charged for unit `id`.
    fn capture_charges(&self, id: usize, events: Vec<crate::macro_host::UnitEvent>) {
        use crate::macro_host::UnitEvent;
        if let Some(capture) = self.capture.borrow_mut().as_mut() {
            let events = events
                .into_iter()
                .filter(|event| matches!(event, UnitEvent::Expansion(_) | UnitEvent::Script { .. }))
                .collect::<Vec<_>>();
            if !events.is_empty() {
                capture.charges.push((id, events));
            }
        }
    }
    /// The expansions drain points charged for each unit since the last
    /// call, by unit ID in charge order.
    pub(crate) fn take_unit_charges(&self) -> Vec<(usize, Vec<crate::macro_host::UnitEvent>)> {
        self.capture
            .borrow_mut()
            .as_mut()
            .map(|capture| std::mem::take(&mut capture.charges))
            .unwrap_or_default()
    }
    /// Note the program globals the analysis of `unit` appended.
    pub fn capture_added_globals(&self, unit: usize, added: Vec<usize>) {
        if let Some(capture) = self.capture.borrow_mut().as_mut()
            && let Some(entry) = unit
                .checked_sub(capture.first_unit)
                .and_then(|index| capture.units.get_mut(index))
        {
            entry.added = added;
        }
    }
    /// The units with IDs in `units`.
    pub fn captured_units(&self, units: std::ops::Range<usize>) -> Result<Vec<UnitRecord>, String> {
        let from = units.start;
        let units = self
            .shared
            .lock()
            .units
            .iter()
            .take(units.end)
            .skip(from)
            .map(|unit| {
                let prepared = match &unit.state {
                    AnalysisState::Prepared(_, _, prepared) => Ok(prepared.clone()),
                    AnalysisState::Unstarted => Err("a deferred unit was never analyzed"),
                    AnalysisState::Analyzing(_) => Err("a deferred unit is still being analyzed"),
                    AnalysisState::Analyzed(..) => {
                        Err("a deferred unit was analyzed but not prepared")
                    }
                    AnalysisState::Failed(..) => Err("a deferred unit failed"),
                };
                let request = unit.request.generation;
                (
                    unit.name.clone(),
                    request,
                    unit.analyses,
                    unit.emitted,
                    prepared,
                )
            })
            .collect::<Vec<_>>();
        let mut capture = self.capture.borrow_mut();
        let capture = capture.as_mut().ok_or("capture is off")?;
        let mut records = Vec::new();
        for (id, (name, generation, analyses, emitted, prepared)) in (from..).zip(units) {
            let entry = id
                .checked_sub(capture.first_unit)
                .and_then(|index| capture.units.get_mut(index))
                .ok_or("a unit registered before capture began")?;
            let reader_contexts = prepared
                .as_ref()
                .ok()
                .map(|prepared| prepared.reader_contexts().clone());
            let code = prepared.map(|prepared| prepared.code().to_owned());
            records.push(UnitRecord {
                name,
                generation,
                analyses,
                emitted,
                code,
                view_globals: entry.view_globals,
                extension_frontier: entry.extension_frontier,
                added: std::mem::take(&mut entry.added),
                reader_contexts,
            });
        }
        Ok(records)
    }
    /// The registrations at positions in `registrations`.
    pub fn captured_registrations(
        &self,
        registrations: std::ops::Range<usize>,
    ) -> Vec<RegistrationRecord> {
        self.shared
            .lock()
            .registrations
            .iter()
            .take(registrations.end)
            .skip(registrations.start)
            .map(|registration| match registration {
                Registration::Root(name, unit, test, once, location) => RegistrationRecord::Root {
                    name: name.clone(),
                    unit: *unit,
                    test: *test,
                    once: *once,
                    location: location.clone(),
                },
                Registration::Bundle(bundle) => RegistrationRecord::Bundle(bundle.clone()),
            })
            .collect()
    }
    /// The ID the next registered unit gets.
    pub fn next_unit(&self) -> usize {
        self.shared.lock().units.len()
    }
    /// Register again, in order, the units and registrations of a namespace
    /// installed from its artifact (`crate::artifact_install`), as
    /// [`Self::register`] and [`Self::register_bundle`] would have: each
    /// root with a unit adds that unit, prepared, at the next ID, which
    /// `registrations` already name. Nothing is analyzed or executed; a
    /// later demand evaluates a unit's prepared JavaScript as it would a
    /// unit this compilation prepared, and the counts are this
    /// compilation's: no analysis and no execution yet.
    pub fn install(
        &self,
        units: Vec<crate::emitter::DeferredUnit>,
        registrations: Vec<InstalledRegistration>,
        source: u32,
    ) -> Result<(), String> {
        let mut units = units.into_iter();
        for registration in registrations {
            let end = {
                let mut c = self.shared.lock();
                let c = &mut *c;
                match &registration {
                    InstalledRegistration::Root {
                        name,
                        unit,
                        test,
                        once,
                        location,
                    } => {
                        let next_var = VarId(c.bindings.len());
                        let binding = c.bindings.entry(name.clone()).or_insert((next_var, 0));
                        if let Some(id) = *unit {
                            if id != c.units.len() {
                                return Err(format!(
                                    "installed unit {id} is not the next unit {}",
                                    c.units.len()
                                ));
                            }
                            binding.1 += 1;
                            let request = UnitRequest {
                                var: binding.0,
                                generation: binding.1,
                            };
                            let prepared = units
                                .next()
                                .ok_or("an installed root names a unit the artifact lacks")?;
                            if let Some(capture) = self.capture.borrow_mut().as_mut() {
                                capture.units.push(UnitCapture::default());
                            }
                            c.units.push(Unit {
                                request,
                                name: name.clone(),
                                source,
                                inputs: None,
                                state: AnalysisState::Prepared(
                                    std::sync::Weak::new(),
                                    Vec::new(),
                                    Arc::new(prepared),
                                ),
                                analyses: 0,
                                emitted: false,
                                waiters: 0,
                                retained: None,
                                macro_free: false,
                                pooled: false,
                                held: false,
                                dispatched: false,
                                charges: None,
                                time: Default::default(),
                            });
                        }
                        if !(unit.is_none() && c.active.contains_key(name)
                            || *once && c.active.get(name).is_some_and(|defined| *defined))
                        {
                            c.multifn_targets.remove(name);
                            c.active.insert(name.clone(), unit.is_some());
                        }
                        c.registrations.push(Registration::Root(
                            name.clone(),
                            *unit,
                            *test,
                            *once,
                            location.clone(),
                        ));
                    }
                    InstalledRegistration::Bundle(bundle) => {
                        c.registrations.push(Registration::Bundle(bundle.clone()));
                        c.bundle_count += 1;
                        c.environment_bundles +=
                            usize::from(reaches_environment_data(&bundle.extensions));
                        for name in &bundle.products {
                            if bundle.once && c.active.get(name) == Some(&true) {
                                continue;
                            }
                            if bundle.once {
                                c.multifn_targets.insert(name.clone(), name.clone());
                            } else {
                                c.multifn_targets.remove(name);
                            }
                            c.active.insert(name.clone(), true);
                        }
                    }
                }
                c.registrations.len()
            };
            self.install_registrations(end, |_| Ok(()))?;
        }
        if units.next().is_some() {
            return Err("an installed unit has no root".into());
        }
        Ok(())
    }
    pub fn bootstrap(&self) -> Option<Bootstrap> {
        self.engine.borrow().bootstrap.clone()
    }
    /// Install in this view's engine, if it has one, the catalog's
    /// registrations before position `end` that it lacks, in order;
    /// `before_last` runs in the engine before the last of them.
    fn install_registrations(
        &self,
        end: usize,
        before_last: impl FnOnce(&dyn MacroEvaluator) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut installed = self.engine.borrow_mut();
        let Some(bootstrap) = installed.bootstrap.clone() else {
            return Ok(());
        };
        let start = installed.installed;
        if start >= end {
            return Ok(());
        }
        installed.installed = end;
        drop(installed);
        let pending = self.shared.lock().registrations[start..end].to_vec();
        install_registrations(bootstrap.evaluator.as_ref(), start, &pending, before_last)
    }
    /// Keep the compile-time runtime's `gensym` from returning any of
    /// `names` in this compilation: now if the session has started, else
    /// once it starts ([`crate::generated_names`]).
    pub fn reserve_gensyms(&self, names: &[Vec<u16>], counter: u64) -> Result<(), String> {
        match self.bootstrap() {
            Some(bootstrap) => {
                let engine = bootstrap.evaluator.as_ref();
                Self::raise_gensyms(engine, counter)?;
                Self::import_gensyms(engine, names)
            }
            None => {
                self.gensym_floor.set(self.gensym_floor.get().max(counter));
                self.gensyms_pending
                    .borrow_mut()
                    .extend(names.iter().cloned());
                Ok(())
            }
        }
    }
    /// Keep `gensym` from appending a number at or below `counter`.
    fn raise_gensyms(engine: &dyn MacroEvaluator, counter: u64) -> Result<(), String> {
        if counter == 0 {
            return Ok(());
        }
        let value = engine
            .evaluate_unit(&format!(
                "rt => {{ rt.compilerForms.raiseGensyms({counter}); return null; }}"
            ))
            .map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())
    }
    /// Start logging what the compile-time runtime's `gensym` returns, for
    /// [`Self::take_gensyms`].
    pub fn log_gensyms(&self) -> Result<(), String> {
        self.gensyms_logged.set(true);
        // Names a core macro expanded natively allocates are logged on this
        // thread, as are those the runtime returns (light-cljs-432).
        crate::generated_names::start_log();
        match self.bootstrap() {
            Some(bootstrap) => Self::take_gensyms_from(bootstrap.evaluator.as_ref()).map(drop),
            None => Ok(()),
        }
    }
    /// Where `gensym`'s counter stands, and what it returned since the last
    /// call or since [`Self::log_gensyms`]; before the session starts, the
    /// counter it will start from and nothing.
    pub fn take_gensyms(&self) -> Result<(u64, Vec<Vec<u16>>), String> {
        let (counter, names) = match self.bootstrap() {
            Some(bootstrap) => Self::take_gensyms_from(bootstrap.evaluator.as_ref())?,
            None => (self.gensym_floor.get(), Vec::new()),
        };
        let mut logged = crate::generated_names::take_log();
        logged.extend(names);
        let names = logged;
        // With a pool, a worker's unit may give these names again.
        if self.pool.borrow().is_some() {
            self.gensyms_seen.borrow_mut().extend(names.iter().cloned());
        }
        let mut buffer = self.gensym_buffer.take();
        if buffer.is_empty() {
            return Ok((counter, names));
        }
        buffer.extend(names);
        Ok((counter, buffer))
    }
    /// Add `names`, what `gensym` gave in a pool worker's engine while it
    /// analyzed a unit the coordinator charges now, after what this
    /// engine's `gensym` gave before, as a serial build logs them; a name
    /// already taken is not added again (docs/compiler-concurrency.md, "As
    /// built (light-cljs-374)").
    fn absorb_gensyms(&self, names: Vec<Vec<u16>>) -> Result<(), String> {
        if names.is_empty() {
            return Ok(());
        }
        self.flush_gensyms()?;
        let mut seen = self.gensyms_seen.borrow_mut();
        let mut buffer = self.gensym_buffer.borrow_mut();
        for name in names {
            if seen.insert(name.clone()) {
                buffer.push(name);
            }
        }
        Ok(())
    }
    /// Move what this engine's `gensym` has given since it was last read
    /// into the log [`Self::take_gensyms`] returns, ahead of what a unit
    /// charged later gives.
    fn flush_gensyms(&self) -> Result<(), String> {
        if let Some(earlier) = self.engine_gensyms()? {
            self.gensyms_seen
                .borrow_mut()
                .extend(earlier.iter().cloned());
            self.gensym_buffer.borrow_mut().extend(earlier);
        }
        Ok(())
    }
    /// What `gensym` has given on this thread since it was last read, while
    /// they are logged: its runtime logs what it returns here, as natively
    /// expanded core macros do ([`crate::generated_names::take_log`]).
    fn engine_gensyms(&self) -> Result<Option<Vec<Vec<u16>>>, String> {
        Ok(self
            .gensyms_logged
            .get()
            .then(crate::generated_names::take_log))
    }
    /// The runtime's `gensym` skips every unqualified symbol name the form
    /// bridge decodes, so importing the names as symbols reserves them.
    fn import_gensyms(engine: &dyn MacroEvaluator, names: &[Vec<u16>]) -> Result<(), String> {
        const CHUNK: usize = 10_000;
        for chunk in names.chunks(CHUNK) {
            let symbols = chunk
                .iter()
                .map(|name| Datum::new(DatumKind::Symbol(Name::from_utf16(None, name))))
                .collect();
            let vector = Datum::new(DatumKind::Collection {
                kind: crate::parser::CollectionKind::Vector,
                children: symbols,
            });
            let limits = crate::form_bridge::ConversionLimits {
                nodes: CHUNK + 1,
                depth: 2,
            };
            let value = engine
                .import_data(&vector, limits)
                .map_err(|e| e.to_string())?;
            engine.release(value).map_err(|e| e.to_string())?;
        }
        Ok(())
    }
    fn take_gensyms_from(engine: &dyn MacroEvaluator) -> Result<(u64, Vec<Vec<u16>>), String> {
        let value = engine
            .evaluate_unit("rt => rt.compilerForms.takeGensyms()")
            .map_err(|e| e.to_string())?;
        let source = crate::compiler_data::Source::new("<gensyms>", "");
        let span = source.span(0..0).expect("an empty span");
        let call = crate::compiler_data::Form::from_data(
            Datum::new(DatumKind::Nil),
            span.clone(),
            crate::compiler_data::Origin::SuppliedData,
        );
        let origin = crate::compiler_data::ExpansionOrigin {
            macro_name: Name::new(None, "gensym"),
            call: span,
            definition: None,
            helpers: Vec::new(),
        };
        // A bound engines pass as a 32-bit integer.
        let limits = crate::form_bridge::ConversionLimits {
            nodes: i32::MAX as usize,
            depth: 2,
        };
        let exported = engine.export_form(value, &call, origin, limits);
        engine.release(value).map_err(|e| e.to_string())?;
        let data = exported.map_err(|e| e.to_string())?.data();
        let DatumKind::Collection { children, .. } = data.kind() else {
            return Err("gensym names are not a vector".into());
        };
        let counter = match children.first().map(Datum::kind) {
            Some(DatumKind::Integer(counter)) if *counter >= 0 => *counter as u64,
            Some(DatumKind::Double(counter)) if *counter >= 0.0 => *counter as u64,
            _ => return Err("the gensym counter is not a count".into()),
        };
        let names = children[1..]
            .iter()
            .map(|name| match name.kind() {
                DatumKind::String(text) => Ok(text.to_vec()),
                _ => Err("a gensym name is not a string".to_owned()),
            })
            .collect::<Result<_, String>>()?;
        Ok((counter, names))
    }
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }
    pub fn attach(self: &Rc<Self>, bootstrap: &Bootstrap, host: &MacroHost) -> Result<(), String> {
        if self.engine.borrow().bootstrap.is_some() {
            return Ok(());
        }
        let evaluator = &bootstrap.evaluator;
        let view: std::rc::Weak<dyn CatalogView> = Rc::downgrade(self) as std::rc::Weak<Self>;
        prepare_engine(evaluator, view, &host.engine)?;
        // What installed namespaces reserved before the session started, and
        // the log capture reads.
        let pending = std::mem::take(&mut *self.gensyms_pending.borrow_mut());
        Self::raise_gensyms(evaluator.as_ref(), self.gensym_floor.get())?;
        Self::import_gensyms(evaluator.as_ref(), &pending)?;
        if self.gensyms_logged.get() {
            Self::take_gensyms_from(evaluator.as_ref())?;
        }
        self.engine.borrow_mut().bootstrap = Some(bootstrap.clone());
        let end = self.shared.lock().registrations.len();
        self.install_registrations(end, |_| Ok(()))
    }
    pub fn compilation_uses(&self) -> Vec<crate::analyzer::CompilationUse> {
        self.shared
            .lock()
            .units
            .iter()
            .map(|unit| crate::analyzer::CompilationUse {
                name: unit.name.clone(),
                generation: unit.request.generation,
                analyses: unit.analyses,
                emissions: usize::from(unit.emitted),
            })
            .collect()
    }
    /// Measure, from now on until [`Self::queued_ir`], the analyzed unit IR
    /// waiting to merge (`--stats`).
    pub(crate) fn measure_queued_ir(&self) {
        self.shared.lock().queued_ir = QueuedIr::default();
        self.shared
            .measuring_ir
            .store(true, std::sync::atomic::Ordering::Relaxed);
    }
    /// The most analyzed unit IR that waited to merge since
    /// [`Self::measure_queued_ir`], which stops measuring.
    pub(crate) fn queued_ir(&self) -> crate::compile_stats::QueuedIrPeak {
        self.shared
            .measuring_ir
            .store(false, std::sync::atomic::Ordering::Relaxed);
        std::mem::take(&mut self.shared.lock().queued_ir).peak
    }
    /// Note that unit `id`'s analyzed IR merged into its program.
    pub(crate) fn merged_ir(&self, id: usize) {
        self.shared.merged_ir(id);
    }
    /// How many units this compilation registered, analyzed, prepared and
    /// executed (`--stats`).
    pub fn unit_counts(&self) -> crate::compile_stats::DeferredUnitCounts {
        let catalog = self.shared.lock();
        let mut counts = crate::compile_stats::DeferredUnitCounts {
            registered: catalog.units.len(),
            ..Default::default()
        };
        for unit in &catalog.units {
            counts.macro_free += usize::from(unit.macro_free);
            if unit.macro_free {
                counts.macro_free_time += unit.time;
            }
            counts.analyzed += usize::from(unit.analyses > 0);
            counts.prepared += usize::from(matches!(unit.state, AnalysisState::Prepared(..)));
            counts.executed += usize::from(unit.emitted);
        }
        use std::sync::atomic::Ordering::Relaxed;
        let pool = &self.shared.counters;
        counts.pooled = pool.pooled.load(Relaxed);
        counts.worker_isolates = pool.isolates.load(Relaxed);
        counts.worker_isolate_startup =
            std::time::Duration::from_nanos(pool.startup_ns.load(Relaxed));
        counts.worker_unit_time = std::time::Duration::from_nanos(pool.unit_ns.load(Relaxed));
        counts.worker_initializations = pool.initializations.load(Relaxed);
        counts.pool_workers = pool.workers.load(Relaxed);
        counts.pool_engine_workers = pool.engine_workers.load(Relaxed);
        counts.engine_less_units = pool.engine_less_units.load(Relaxed);
        counts.pool_time = std::time::Duration::from_nanos(pool.pool_ns.load(Relaxed));
        counts.pool = *pool
            .scheduler
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        counts.catalog_contended = pool.catalog_contended.load(Relaxed);
        counts
    }
    pub fn extension_frontier(&self) -> usize {
        self.shared.lock().bundle_count
    }
    /// How many registered bundles [`reaches_environment_data`].
    pub fn environment_bundles(&self) -> usize {
        self.shared.lock().environment_bundles
    }
    pub fn multifn_target(&self, name: &str) -> Option<String> {
        self.shared.lock().multifn_targets.get(name).cloned()
    }
    pub fn register_bundle(
        &self,
        program: &Program,
        registration: BundleRegistration<'_>,
    ) -> Result<usize, String> {
        let BundleRegistration {
            products,
            extensions,
            once,
            form,
            snapshot,
            host,
            location,
            analyzed,
        } = registration;
        let names = products
            .iter()
            .map(|g| {
                let b = &program.globals[g.0];
                format!("{}/{}", b.namespace, b.name)
            })
            .collect::<Vec<_>>();
        let name = format!("declaration#{}", self.shared.lock().units.len());
        let unit = self
            .register(name, Some(form), snapshot, host, false, location.clone())?
            .unwrap();
        let group = analyzed.is_some();
        if let Some(mut program) = analyzed {
            program.shrink_to_fit();
            let inputs = {
                let mut c = self.shared.lock();
                let entry = &mut c.units[unit];
                entry.state = AnalysisState::Analyzed(Arc::new(program), Vec::new());
                entry.analyses = 1;
                entry.inputs.take()
            };
            let binding = self.take_unit_binding(unit);
            drop((inputs, binding));
        }
        let reaches = reaches_environment_data(extensions);
        let bundle = Arc::new(BundleRecord {
            products: names,
            unit,
            extensions: extensions.to_vec(),
            location,
            once,
            group,
        });
        let end = {
            let mut c = self.shared.lock();
            c.registrations.push(Registration::Bundle(bundle));
            c.bundle_count += 1;
            c.environment_bundles += usize::from(reaches);
            for g in products {
                let b = &program.globals[g.0];
                let name = format!("{}/{}", b.namespace, b.name);
                if once && c.active.get(&name) == Some(&true) {
                    continue;
                }
                if once {
                    c.multifn_targets.insert(name.clone(), name.clone());
                } else {
                    c.multifn_targets.remove(&name);
                }
                c.active.insert(name, true);
            }
            c.registrations.len()
        };
        // Initializers that call `gensym` number in the scope that
        // registers them.
        self.install_registrations(end, |engine| {
            crate::generated_names::sync_gensym_scope(engine).map_err(|e| e.to_string())
        })?;
        if group {
            self.release_application_ir(unit);
        }
        Ok(unit)
    }
    pub fn register(
        &self,
        name: String,
        form: Option<Form>,
        mut snapshot: DefinitionSnapshot,
        host: MacroHost,
        once: bool,
        location: String,
    ) -> Result<Option<usize>, String> {
        // Executable metadata has its own source-site unit and root state.
        // Reflecting it cannot analyze or execute the ordinary initializer.
        if let Some(test) = snapshot.definition_test.take().filter(|_| form.is_some()) {
            let test_name = format!("{name} :test#{}", self.shared.lock().units.len());
            snapshot.definition_test_unit = self.register(
                test_name,
                Some(test),
                snapshot.clone(),
                host.macro_analysis_frame(),
                false,
                location.clone(),
            )?;
        }
        let test = snapshot.definition_test_unit;
        #[cfg(test)]
        if let Some(form) = &form {
            worker_tests::retain(self.next_unit(), form, &snapshot, &host);
        }
        let (environment, binding) = host.into_parts();
        let (id, end) = {
            let mut c = self.shared.lock();
            let c = &mut *c;
            let next_var = VarId(c.bindings.len());
            let binding = c.bindings.entry(name.clone()).or_insert((next_var, 0));
            if form.is_some() {
                binding.1 += 1;
            }
            let request = UnitRequest {
                var: binding.0,
                generation: binding.1,
            };
            let id = form.map(|form| {
                let id = c.units.len();
                if let Some(capture) = self.capture.borrow_mut().as_mut() {
                    capture.units.push(UnitCapture {
                        view_globals: snapshot.resolution_program.globals.len(),
                        extension_frontier: snapshot.extension_frontier,
                        added: Vec::new(),
                    });
                }
                c.units.push(Unit {
                    request,
                    name: name.clone(),
                    source: snapshot.source_index as u32,
                    inputs: Some(Box::new(UnitInputs {
                        form,
                        snapshot,
                        environment,
                    })),
                    state: AnalysisState::Unstarted,
                    analyses: 0,
                    emitted: false,
                    waiters: 0,
                    retained: None,
                    macro_free: false,
                    pooled: false,
                    held: false,
                    dispatched: false,
                    charges: None,
                    time: Default::default(),
                });
                id
            });
            if !(id.is_none() && c.active.contains_key(&name)
                || once && c.active.get(&name).is_some_and(|defined| *defined))
            {
                c.multifn_targets.remove(&name);
                c.active.insert(name.clone(), id.is_some());
            }
            c.registrations
                .push(Registration::Root(name, id, test, once, location));
            (id, c.registrations.len())
        };
        if let Some(id) = id {
            let mut bindings = self.unit_bindings.borrow_mut();
            if bindings.len() <= id {
                bindings.resize_with(id + 1, || None);
            }
            bindings[id] = Some(Box::new(binding));
        }
        self.install_registrations(end, |_| Ok(()))?;
        Ok(id)
    }
    /// Mark unit `id`, just registered, as macro-free: its form reaches no
    /// compile-time engine, so a pool worker may analyze it, and every
    /// analysis of it panics if it does reach one
    /// ([`crate::macro_host::MacroFreeUnit`]).
    pub fn set_macro_free(&self, id: usize) {
        self.shared.lock().units[id].macro_free = true;
    }
    /// Let the build's unit pool analyze unit `id` ([`Self::submit_units`]).
    pub(crate) fn set_pooled(&self, id: usize) {
        self.shared.lock().units[id].pooled = true;
    }
    /// Of `units`, those for the pool that no thread has started, in order.
    pub(crate) fn poolable(&self, units: impl Iterator<Item = usize>) -> Vec<usize> {
        let c = self.shared.lock();
        units
            .filter(|&id| {
                c.units
                    .get(id)
                    .is_some_and(|u| u.pooled && matches!(u.state, AnalysisState::Unstarted))
            })
            .collect()
    }
    /// Perturb this build's unit pool and analyses, for a stress test.
    #[cfg(debug_assertions)]
    pub(crate) fn perturb(&self, perturbation: crate::scheduler::Perturbation) {
        let _ = self.shared.perturbation.set(perturbation);
    }
    /// Note what the compilation records of each recorded analysis beyond
    /// its charges, with a pool or without: the units the coordinator keeps
    /// at a dispatch are recorded too (light-cljs-391).
    pub(crate) fn set_recording(&self, recording: crate::macro_host::UnitRecording) {
        let _ = self.shared.recording.set(recording);
    }
    /// Start the build's unit pool of `slots`, whose workers are bound to
    /// `environment` and of which `isolates`, the coordinator's included,
    /// have `engines` ([`UnitPool::start`]).
    pub(crate) fn start_pool(
        &self,
        slots: crate::scheduler::Slots,
        engines: PoolEngines,
        isolates: usize,
        environment: MacroEnvironment,
        resources: Option<crate::evaluator::BuildResources>,
        recording: crate::macro_host::UnitRecording,
    ) -> Result<(), String> {
        if let Some(resources) = resources {
            let _ = self.shared.resources.set(resources);
        }
        let _ = self.shared.recording.set(recording);
        let pool = UnitPool::start(self.shared.clone(), slots, engines, isolates, environment)?;
        *self.pool.borrow_mut() = Some(pool);
        Ok(())
    }
    /// Whether the build's unit pool takes a unit: with `macro_free`, one
    /// found macro-free; with `test`, one of a definition with `:test`
    /// metadata, analyzed with its definition through this session. Its
    /// workers without engines take only macro-free units. A cache
    /// blocker's consumer never asks (`NamespaceAnalysis::main_isolate`).
    pub(crate) fn pools(&self, macro_free: bool, test: bool) -> bool {
        match &*self.pool.borrow() {
            None => false,
            Some(pool) if pool.engines == PoolEngines::None => macro_free,
            Some(_) => macro_free || !test,
        }
    }
    /// Whether the build's unit pool takes a declaration bundle
    /// (light-cljs-428): one [`Self::pools`] takes, when more than one of
    /// the pool's workers may analyze it, any worker for a macro-free one
    /// and one with an engine for another (light-cljs-437). With one, the
    /// coordinator keeps its bundles, as before: it never analyzes a
    /// queued unit, so giving the one worker the bundles as well as the
    /// definitions would leave the coordinator waiting for it at the drain
    /// points (200k generated project at `--jobs 8` with one worker
    /// isolate: 8.79 s against 6.62 s).
    pub(crate) fn pools_bundle(&self, macro_free: bool) -> bool {
        self.pools(macro_free, false)
            && self
                .pool
                .borrow()
                .as_ref()
                .is_some_and(|pool| match macro_free {
                    true => pool.workers > 1,
                    false => pool.engine_workers > 1,
                })
    }
    /// Give `units` to the build's pool: whether it has one.
    /// The compilation has charged `charged` expansions and generated
    /// nodes before them ([`SharedCatalog::raise_charged`]).
    pub(crate) fn submit_units(&self, units: Vec<usize>, charged: (usize, usize)) -> bool {
        match &*self.pool.borrow() {
            Some(pool) => {
                self.shared.raise_charged(charged.0, charged.1);
                pool.submit(units);
                true
            }
            None => false,
        }
    }
    /// Note that the compilation has charged `charged` expansions and
    /// generated nodes, the soft cap that units recorded from here on start
    /// from ([`SharedCatalog::raise_charged`]).
    pub(crate) fn note_charged(&self, charged: (usize, usize)) {
        self.shared.raise_charged(charged.0, charged.1);
    }
    /// Start the coordinator's engine for `host` at a namespace's dispatch,
    /// if it has none ([`MacroHost::start_session_engine`]).
    pub(crate) fn start_dispatch_engine(self: &Rc<Self>, host: &MacroHost) -> Result<(), String> {
        if self.bootstrap().is_some() {
            return Ok(());
        }
        #[cfg(debug_assertions)]
        if let Some(delay) = self
            .shared
            .perturbation
            .get()
            .and_then(|p| p.main_engine_delay)
        {
            std::thread::sleep(delay);
        }
        let bootstrap = match &host.engine.bootstrap {
            Some(bootstrap) => bootstrap.clone(),
            None => crate::compiler_bootstrap::build(host, true)?,
        };
        #[cfg(debug_assertions)]
        if let Some(delay) = self
            .shared
            .perturbation
            .get()
            .and_then(|p| p.main_attach_delay)
        {
            std::thread::sleep(delay);
        }
        // An engine that failed to attach is not the session's: the
        // expansion that needs one starts it again, and fails as it would
        // have without this start.
        let attached = self.attach(&bootstrap, host);
        if attached.is_err() {
            self.engine.borrow_mut().bootstrap = None;
        }
        attached
    }
    /// The coordinator's engine, started now if it has none, as the first
    /// JavaScript macro a serial build expands starts it; `binding` is a
    /// frame of the compilation's, which reaches this session. None without
    /// a pool or a session; a start that fails, its engine's construction or
    /// its attachment, gives the error that expansion would have failed with
    /// (light-cljs-386).
    fn start_main_engine(&self, binding: &EngineBinding) -> Result<Option<Bootstrap>, String> {
        if let Some(bootstrap) = self.bootstrap() {
            return Ok(Some(bootstrap));
        }
        let Some(environment) = self.pool.borrow().as_ref().map(|pool| pool.main.clone()) else {
            return Ok(None);
        };
        let Some(session) = binding.session_handle() else {
            return Ok(None);
        };
        let host = MacroHost::from_parts(environment, binding.frame());
        let bootstrap = crate::compiler_bootstrap::build(&host, true)?;
        #[cfg(debug_assertions)]
        if let Some(delay) = self
            .shared
            .perturbation
            .get()
            .and_then(|p| p.main_attach_delay)
        {
            std::thread::sleep(delay);
        }
        session.attach(&bootstrap, &host)?;
        Ok(Some(bootstrap))
    }
    /// The build failed with an error the coordinator has chosen: what
    /// the pool has not started is not needed, and what its workers run is
    /// cancelled (docs/compiler-concurrency.md, "As built (light-cljs-341)").
    pub(crate) fn cancel_pool(&self) {
        if let Some(pool) = &*self.pool.borrow() {
            let mut inbox = pool.inbox.lock();
            inbox.batches.clear();
            inbox.closed = true;
            drop(inbox);
            pool.inbox.ready.notify_all();
        }
        if let Some(resources) = self.shared.resources() {
            resources.workers.cancel();
        }
        // Nothing the pool still runs is ordered against an earlier unit.
        self.shared.lanes.abandon();
        #[cfg(feature = "bbp-macros")]
        if let Some(Ok(runtime)) = self.shared.native_runtime.get() {
            runtime.cancel();
        }
    }
    /// Stop the build's unit pool and join its workers ([`UnitPool::finish`]).
    pub(crate) fn finish_pool(&self) {
        let pool = self.pool.borrow_mut().take();
        if let Some(pool) = pool {
            pool.finish();
        }
        self.shared.lanes.set_rescuer(None);
        self.shared.lanes.join_rescues();
    }
    /// Whether unit `id`'s analysis failed.
    pub(crate) fn unit_failed(&self, id: usize) -> bool {
        matches!(
            self.shared.lock().units.get(id).map(|unit| &unit.state),
            Some(AnalysisState::Failed(..))
        )
    }
    pub fn diagnostic(&self, id: usize) -> Option<crate::AnalysisError> {
        match &self.shared.lock().units.get(id)?.state {
            AnalysisState::Failed(_, diagnostic) => diagnostic.clone(),
            _ => None,
        }
    }
    /// Called after the analyzed body has been copied into its application
    /// owner. Preparing source is pure: execution and registration stay lazy.
    pub fn release_application_ir(&self, id: usize) {
        self.shared.release_application_ir(id);
    }
}
impl SharedCatalog {
    /// [`CompilationSession::release_application_ir`] on any thread.
    fn release_application_ir(&self, id: usize) {
        let program = match &self.lock().units[id].state {
            AnalysisState::Analyzed(program, _) => program.clone(),
            _ => return,
        };
        // Prepared outside the lock; the analyzed program is dropped by its
        // last holder, outside it too.
        let prepared = Arc::new({
            #[cfg(feature = "bbp-macros")]
            if self.native_units.load(std::sync::atomic::Ordering::Relaxed) {
                crate::emitter::DeferredUnit::prepare_native(&program, &self.native_replaced_core())
            } else {
                crate::emitter::DeferredUnit::prepare(&program)
            }
            #[cfg(not(feature = "bbp-macros"))]
            crate::emitter::DeferredUnit::prepare(&program)
        });
        let previous = {
            let mut c = self.lock();
            let state = &mut c.units[id].state;
            match std::mem::replace(state, AnalysisState::Unstarted) {
                AnalysisState::Analyzed(program, warnings) => {
                    *state = AnalysisState::Prepared(Arc::downgrade(&program), warnings, prepared);
                    // Threads waiting for the analysis have not taken it yet.
                    let unit = &mut c.units[id];
                    if unit.waiters > 0 {
                        unit.retained = Some(program);
                        None
                    } else {
                        Some(program)
                    }
                }
                other => {
                    *state = other;
                    None
                }
            }
        };
        drop((previous, program));
    }
}
impl CompilationSession {
    /// The engine binding unit `id` was registered with, if its analysis
    /// has not taken it.
    fn take_unit_binding(&self, id: usize) -> Option<Box<EngineBinding>> {
        self.unit_bindings
            .borrow_mut()
            .get_mut(id)
            .and_then(Option::take)
    }
    pub fn analyze(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        self.analyze_in(id, engine, false, Take::Charge)
    }
    /// Analyze unit `id`, which its namespace's dispatch keeps on this
    /// thread, recording what it charges for its drain point, where
    /// [`Self::analyze_pooled`] charges it in unit order as it charges a
    /// worker's unit (docs/compiler-concurrency.md, "As built
    /// (light-cljs-391)"). An error is the unit's failure, which its drain
    /// point reports.
    pub(crate) fn analyze_held(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
    ) -> Result<(), String> {
        // A kept unit's analysis starts its own activity at its lane.
        let _root = self.shared.lanes.enter_root();
        let analysis = self.analyze_in(id, engine, false, Take::Hold).map(drop);
        // No pool job after it in unit order starts, as after a worker's.
        self.shared.note_job(id);
        analysis
    }
    /// Unit `id`'s analysis, which the drain gave to the unit pool: a
    /// worker analyzes it, and this thread waits for it even while it is
    /// queued ([`SharedCatalog::claim`]).
    pub(crate) fn analyze_pooled(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        self.analyze_in(id, engine, true, Take::Charge)
    }
    /// Unit `id`'s analysis at its namespace's drain point, which charges
    /// what it recorded ([`UnitCharges`]): a worker's if the pool was given
    /// it (`queued`), waiting for it, or this thread's.
    pub(crate) fn analyze_drained(
        &self,
        id: usize,
        queued: bool,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        self.analyze_in(id, None, queued, Take::Drain)
    }
    /// Wait for unit `id`'s analysis, a worker's if the pool was given it
    /// (`queued`), and add what it observed to the dependency log now, at
    /// its namespace's settle point; its drain point charges the rest
    /// (light-cljs-394). An error is the unit's failure.
    pub(crate) fn observe_pooled(&self, id: usize, queued: bool) -> Result<(), String> {
        self.analyze_in(id, None, queued, Take::Observe).map(drop)
    }
    /// Note that the dispatch of their namespace keeps `units` for its
    /// drain point, which charges them; until then a demand of one takes
    /// its analysis and leaves its charges (light-cljs-394).
    pub(crate) fn hold_units(&self, units: impl Iterator<Item = usize>, forced: bool) {
        let units: Vec<usize> = units.collect();
        {
            let mut c = self.shared.lock();
            for &id in &units {
                if let Some(unit) = c.units.get_mut(id) {
                    unit.held = true;
                    unit.dispatched = true;
                }
            }
        }
        self.shared.schedule_lanes(&units, forced);
    }
    /// Give `units`, drained here in order, their lane positions
    /// ([`SharedCatalog::schedule_lanes`]).
    pub(crate) fn schedule_units(&self, units: &[usize], forced: bool) {
        self.shared.schedule_lanes(units, forced);
    }
    /// Start a namespace's top-level pass segment on its lane
    /// ([`crate::effect_lanes::Lanes::begin_pass`]).
    pub(crate) fn begin_pass(&self, forced: bool) -> crate::effect_lanes::PassGuard {
        self.shared.lanes.begin_pass(forced)
    }
    fn analyze_in(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
        queued: bool,
        take: Take,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        // A unit held for its drain point is charged there, not where a
        // later top-level pass demands it first, so that an earlier
        // namespace's units are charged at one point in every build, with
        // or without the cache (light-cljs-394); one no thread has
        // analyzed is recorded for it, as the dispatch records a kept
        // unit. So is one demanded while another unit is recorded, rather
        // than in that recording, which it joined only when no worker had
        // started it first (light-cljs-442).
        let take = match take {
            Take::Charge if self.shared.held(id) => Take::Defer,
            Take::Drain => {
                self.shared.release_hold(id);
                Take::Drain
            }
            take => take,
        };
        let bootstrap = self.bootstrap();
        // A unit analyzed on a worker, or a macro-free one anywhere, is
        // charged to the counters of the binding it was registered with, a
        // frame of the compilation's ([`UnitCharges`]).
        let counters = self
            .unit_bindings
            .borrow()
            .get(id)
            .and_then(Option::as_ref)
            .map(|binding| binding.frame());
        // Workers forward their `light.build` requests here. What a
        // request records goes back to the worker's unit, which is charged
        // in unit order (light-cljs-374), not in the order requests come.
        let serve = |text: &str, analysis: Vec<u32>| {
            let binding = counters.as_ref().expect("a unit binding");
            let log = binding.dependency_log.borrow().as_ref().map(|log| {
                crate::dependency_manifest::ObservationLog {
                    // The worker's unit's source, whatever namespace this
                    // thread analyzes meanwhile (light-cljs-391).
                    window: analysis.first().copied().unwrap_or(log.window),
                    ..Default::default()
                }
            });
            let log = Rc::new(RefCell::new(log));
            let reply = crate::build_services::serve_request(
                &binding.build,
                &log,
                text,
                |unit| self.shared.unit_source(unit),
                analysis,
            );
            (reply, log.take().map(Box::new))
        };
        let server = counters.is_some().then_some(&serve as BuildServer<'_>);
        // What `gensym` gives in this engine during a kept unit's analysis
        // is the unit's, which its drain point adds, as a worker's.
        if take == Take::Hold {
            self.flush_gensyms()?;
        }
        let analysis = analyze_unit(
            &self.shared,
            self.thread,
            id,
            engine,
            Demand {
                help: None,
                queued,
                on_worker: false,
                held: matches!(take, Take::Hold | Take::Defer),
                server,
            },
            || {
                let binding = self.take_unit_binding(id).expect("unstarted unit host");
                (*binding, bootstrap)
            },
        );
        match take {
            Take::Charge | Take::Drain => {}
            // What it observed goes to the log now; the rest waits.
            Take::Observe => {
                if let Some(binding) = &counters
                    && let Some(observations) = self.shared.take_observations(id)
                    && let Some(log) = binding.dependency_log.borrow_mut().as_mut()
                {
                    log.absorb(*observations);
                }
                return analysis;
            }
            // Its drain point charges it to the binding it was registered
            // with.
            Take::Hold | Take::Defer => {
                if take == Take::Hold
                    && let Some(names) = self.engine_gensyms()?
                {
                    let mut c = self.shared.lock();
                    match c.units[id].charges.as_mut() {
                        Some(charges) => charges.gensyms.extend(names),
                        None => {
                            drop(c);
                            self.gensym_buffer.borrow_mut().extend(names);
                        }
                    }
                }
                if let Some(binding) = counters
                    && let Some(slot) = self.unit_bindings.borrow_mut().get_mut(id)
                    && slot.is_none()
                {
                    *slot = Some(Box::new(binding));
                }
                if take == Take::Defer {
                    // No pool job after it in unit order starts if it failed.
                    self.shared.note_job(id);
                }
                return analysis;
            }
        }
        // A unit that this thread demands while it records another's
        // analysis is charged at its own drain point, or where it is
        // demanded outside a recording, as a unit a worker demands is.
        if crate::macro_host::MacroFreeUnit::active() {
            return analysis;
        }
        // Analyzed on another thread, the unit's binding is no longer needed.
        drop(self.take_unit_binding(id));
        // A pooled unit a worker found too deep fails here too, and needed
        // the depth its claim here would have.
        if queued
            && let (Some(binding), Err(error)) = (&counters, &analysis)
            && error == DEPTH_LIMIT
        {
            let depth = self.shared.lock().threads[self.thread].stack.len();
            crate::macro_host::WorkMarks::note(&binding.work.unit_depth, depth + 1);
        }
        if let Some(binding) = counters
            && let Some(mut charges) = self.shared.take_charges(id)
        {
            let depth = self.shared.lock().threads[self.thread].stack.len();
            // What its analysis here would have polled ([`analyze_unit`]).
            let control = engine.map(|engine| engine.control()).or_else(|| {
                binding
                    .bootstrap
                    .as_ref()
                    .or(self.bootstrap().as_ref())
                    .map(|b| b.evaluator.control())
            });
            // A demand this deep fails before the unit is analyzed, as
            // its claim would have failed it with one slot.
            if depth >= charges.depth_limit {
                crate::macro_host::WorkMarks::note(&binding.work.unit_depth, depth + 1);
                self.shared.fail(id, DEPTH_LIMIT.to_owned(), None);
                return Err(DEPTH_LIMIT.to_owned());
            }
            // A worker's trace rows go where the serial build would have
            // recorded them, as the charges do (light-cljs-369).
            if let Some(rows) = charges.trace.take() {
                crate::compile_trace::replay(*rows, depth);
            }
            let before = (binding.expansions.get(), binding.generated_nodes.get());
            // A worker's unit that expanded a JavaScript macro would have
            // started the coordinator's engine with one slot, at that
            // expansion; it starts it there, so later polls are spent.
            let start = || {
                #[cfg(debug_assertions)]
                if let Some(delay) = self
                    .shared
                    .perturbation
                    .get()
                    .and_then(|p| p.main_engine_delay)
                    && self.bootstrap().is_none()
                {
                    std::thread::sleep(delay);
                }
                self.start_main_engine(&binding)
                    .map(|b| b.map(|b| b.evaluator.control()))
            };
            if let Err((message, diagnostic)) =
                charges.charge(&binding, depth, control, &start, self)
            {
                self.shared.fail(id, message.clone(), Some(diagnostic));
                return Err(message);
            }
            // The unit's trace frame counted these; the frame it is
            // charged in does not count them as its own.
            crate::compile_trace::charged(
                &binding.expansions,
                binding.expansions.get().saturating_sub(before.0),
                binding.generated_nodes.get().saturating_sub(before.1),
            );
            let UnitCharges {
                observations,
                gensyms,
                events,
                ..
            } = *charges;
            // What the drain point charged, for its namespace's artifact,
            // whose installation charges it there again.
            if take == Take::Drain {
                self.capture_charges(id, events);
            }
            if let (Some(observations), Some(log)) =
                (observations, binding.dependency_log.borrow_mut().as_mut())
            {
                log.absorb(*observations);
            }
            if let Err(error) = self.absorb_gensyms(gensyms) {
                self.shared.fail(id, error.clone(), None);
                return Err(error);
            }
            (self.shared).raise_charged(binding.expansions.get(), binding.generated_nodes.get());
        }
        analysis
    }
}
/// Analyze unit `id` on the thread in slot `thread`, or wait for the thread
/// that analyzes it, or reuse what an analysis gave. `binding` gives the
/// engine binding the unit's analysis frame is taken from, and the bootstrap
/// engine the frame gets if the binding has none.
fn analyze_unit(
    shared: &SharedCatalog,
    thread: usize,
    id: usize,
    engine: Option<&dyn MacroEvaluator>,
    demand: Demand<'_>,
    binding: impl FnOnce() -> (EngineBinding, Option<Bootstrap>),
) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
    let Demand {
        queued,
        on_worker,
        held,
        server,
        help,
    } = demand;
    let (inputs, depth, name, macro_free) =
        match shared.claim_help(thread, id, queued, server, help)? {
            Claim::Reuse(program, warnings) => return Ok((program, warnings)),
            Claim::TooDeep(inputs, depth) => {
                shared.lanes.finish(id, true);
                let (binding, _) = binding();
                crate::macro_host::WorkMarks::note(&binding.work.unit_depth, depth + 1);
                drop((inputs, binding));
                return Err(DEPTH_LIMIT.to_owned());
            }
            Claim::Analyze(inputs, depth, name, macro_free) => (inputs, depth, name, macro_free),
        };
    // Its atom effects keep its `--jobs 1` place (light-cljs-480); declared
    // before the guard, so it ends after the unit settles.
    let _lane = shared.lanes.enter_unit(id);
    // From here on, however the analysis ends, even by unwinding, the unit
    // settles and its waiters wake.
    let mut guard = AnalysisGuard {
        shared,
        thread,
        id,
        state: None,
        held: 0,
    };
    #[cfg(test)]
    let on_claim = shared
        .on_claim
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    #[cfg(test)]
    if let Some(hook) = on_claim {
        hook(id);
    }
    let UnitInputs {
        form,
        snapshot,
        environment,
    } = *inputs;
    let (binding, bootstrap) = binding();
    crate::macro_host::WorkMarks::note(&binding.work.unit_depth, depth + 1);
    // A unit is analyzed once; its registration-time host is not needed
    // after its analysis frame is taken.
    let mut host = MacroHost::from_parts(environment, binding).macro_analysis_frame();
    if host.engine.bootstrap.is_none() {
        host.engine.bootstrap = bootstrap;
    }
    // A macro-free unit, on any thread, and any unit a worker analyzes
    // record what they would charge to the compilation, which the
    // coordinator charges in its order ([`UnitCharges`]); a unit demanded
    // while one is recorded is recorded with it, in its place, as the
    // serial build charged it.
    // So does a unit the coordinator keeps at its namespace's dispatch,
    // whose drain point charges it (light-cljs-391).
    // A unit a dispatch took is recorded on its own even inside another's
    // recording, on any thread: its drain point charges it, as it charges
    // it when a worker started it before the demand (light-cljs-442).
    let nested = crate::macro_host::MacroFreeUnit::active();
    let own = nested && (held || (on_worker && shared.dispatched(id)));
    let records = own || (!nested && (macro_free || on_worker || held));
    if records || (nested && !on_worker) {
        // Counted where the unit is charged ([`UnitCharges`]), from what
        // the coordinator charged before it; the unit's trace frame counts
        // these. A unit the coordinator demands while it records another
        // counts on its own cells too, as a worker's does on the worker's,
        // so that only the recording charges it.
        let (expansions, nodes) = shared.charged();
        host.engine.expansions = std::rc::Rc::new(std::cell::Cell::new(expansions));
        host.engine.generated_nodes = std::rc::Rc::new(std::cell::Cell::new(nodes));
        if !records {
            host.engine.work = Default::default();
        }
    }
    let (analysis, charges) = {
        // Declared before the frontier guard, so the frontier's transitions
        // count in the unit's frame (`--stats`, [`crate::compile_trace`]).
        let _traced = crate::compile_trace::enter_unit(
            || crate::compile_trace::UnitInfo {
                id,
                source: shared.unit_source(id).unwrap_or_default(),
                name: shared.unit_name(id),
                span: form.span(),
                depth: depth + HELP_DEPTH.get(),
            },
            &host,
        );
        let evaluator = host.engine.bootstrap.as_ref().map(|b| b.evaluator.clone());
        let engine = engine.or_else(|| {
            evaluator
                .as_ref()
                .map(|e| e.as_ref() as &dyn MacroEvaluator)
        });
        let _frontier = if let Some(engine) = engine {
            engine
                .compiler_frontier(Some(snapshot.extension_frontier))
                .map_err(|e| e.to_string())?;
            Some(FrontierGuard { engine })
        } else {
            None
        };
        // The unit's macros number in its scope; whatever demanded it, such
        // as a running macro calling it, numbers in its own again after.
        let _gensyms = crate::generated_names::RestoreGensymScope(engine);
        if records {
            let started = crate::evaluator::Instant::now();
            host.engine.work = Default::default();
            let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
            let location = format!("{}:{}:{}: {name}", loc.source, loc.line, loc.column);
            let recording = shared.recording();
            // While artifacts are captured, a worker's engine logs what
            // `gensym` gives from here on, which the unit's analysis hands
            // the coordinator; what the engine gave before, setting itself
            // up or installing the catalog's registrations, is its own.
            // The log is this thread's: the engine logs into it, and so do
            // the core macros the unit expands natively, with or without an
            // engine (light-cljs-432).
            let logs_gensyms = on_worker && recording.capture && !nested;
            let started_log = if logs_gensyms {
                crate::generated_names::start_log();
                match engine {
                    Some(engine) => CompilationSession::take_gensyms_from(engine).map(drop),
                    None => Ok(()),
                }
            } else {
                Ok(())
            };
            // A unit recorded on its own inside another's recording, on any
            // thread, logs into a log of its own, if the thread logs, and
            // the other's goes on after it, so that its names are its own
            // charges' wherever it is analyzed; the engine logs into this
            // thread's log already (light-cljs-442).
            let nested_log = own.then(crate::generated_names::nest_log);
            let free = crate::macro_host::MacroFreeUnit::enter(name, macro_free, recording, !held);
            let analysis =
                crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);
            let recorded = free.finish();
            // What `gensym` gave on this worker, for the artifact
            // ([`CompilationSession::take_gensyms`]).
            let gensyms = if let Some(log) = nested_log {
                Ok(log.finish().unwrap_or_default())
            } else if logs_gensyms {
                let logged = crate::generated_names::stop_log();
                started_log.map(|()| logged)
            } else {
                Ok(Vec::new())
            };
            let mut events = recorded.events;
            // Names it cannot account for keep its namespace's artifact from
            // being installed, as an unmeasured conversion does.
            let gensyms = gensyms.unwrap_or_else(|_| {
                events.push(crate::macro_host::UnitEvent::Unmeasured);
                Vec::new()
            });
            let charges = UnitCharges {
                events,
                macro_depth: host.engine.work.macro_depth.get(),
                analysis_depth: host.engine.work.analysis_depth.get(),
                limits: (host.limits.expansions, host.limits.generated_nodes),
                depth_limit: host.limits.expansion_depth.min(64),
                location,
                observations: recorded.observations,
                gensyms,
                trace: None,
            };
            (analysis, Some((charges, started.elapsed())))
        } else {
            let analysis =
                crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);
            (analysis, None)
        }
    };
    if let Some((mut charges, time)) = charges {
        // On a worker, the trace of the unit, whose frame has closed, and
        // of those it demanded, which the coordinator records where it
        // charges the unit. A unit recorded inside another's recording
        // leaves its rows in that unit's, whose frame it is in.
        if !nested {
            charges.trace = crate::compile_trace::take_worker_rows();
        }
        shared.record_charges(id, charges, time);
    }
    let diagnostic = analysis.as_ref().err().cloned();
    let result = analysis
        .map(|(mut p, w)| {
            p.shrink_to_fit();
            (Arc::new(p), w)
        })
        .map_err(|e| {
            let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
            format!(
                "{}:{}:{}: {}: {e}",
                loc.source,
                loc.line,
                loc.column,
                shared.unit_name(id)
            )
        });
    if let Ok((p, _)) = &result
        && shared.measuring_ir()
    {
        guard.held = crate::program_fragment::SharedPrefix::same(&snapshot.resolution_program)
            .added_bytes(p);
    }
    guard.state = Some(match &result {
        Ok((p, w)) => AnalysisState::Analyzed(p.clone(), w.clone()),
        Err(e) => AnalysisState::Failed(e.clone(), diagnostic),
    });
    drop(guard);
    result
}
/// How many consecutive root registrations [`install_registrations`] gives
/// an engine at once, in blocks aligned to the catalog: an engine catching
/// up, such as a pool worker's, then installs a full block with the same
/// text as every other engine ([`MacroEvaluator::register_compiler_roots`]).
const ROOT_BLOCK: usize = 64;
/// Install `registrations`, the catalog's from position `first`, in
/// `engine` in order; `before_last` runs in it before the last of them.
fn install_registrations(
    engine: &dyn MacroEvaluator,
    first: usize,
    registrations: &[Registration],
    before_last: impl FnOnce(&dyn MacroEvaluator) -> Result<(), String>,
) -> Result<(), String> {
    #[cfg(feature = "bbp-macros")]
    if let Some(native) = engine.native_shell() {
        if let Some((last, preceding)) = registrations.split_last() {
            native
                .install_catalog(first, preceding)
                .map_err(|e| e.to_string())?;
            before_last(engine)?;
            native
                .install_catalog(first + preceding.len(), std::slice::from_ref(last))
                .map_err(|e| e.to_string())?;
        }
        return Ok(());
    }
    let mut before_last = Some(before_last);
    let mut roots = Vec::new();
    for (index, registration) in registrations.iter().enumerate() {
        let last = index + 1 == registrations.len();
        if let Registration::Root(name, unit, test, once, location) = registration
            && !last
        {
            roots.push(crate::evaluator::CompilerRoot {
                name,
                unit: *unit,
                once: *once,
                location,
                test: *test,
            });
            if (first + index + 1).is_multiple_of(ROOT_BLOCK) {
                engine
                    .register_compiler_roots(&std::mem::take(&mut roots))
                    .map_err(|e| e.to_string())?;
            }
            continue;
        }
        if !roots.is_empty() {
            engine
                .register_compiler_roots(&std::mem::take(&mut roots))
                .map_err(|e| e.to_string())?;
        }
        if last && let Some(before_last) = before_last.take() {
            before_last(engine)?;
        }
        match registration {
            Registration::Root(name, unit, test, once, location) => engine
                .register_compiler_root(name, *unit, *once, location, *test)
                .map_err(|e| e.to_string())?,
            Registration::Bundle(bundle) => engine
                .register_compiler_bundle(&bundle.source(), bundle.unit)
                .map_err(|e| e.to_string())?,
        }
    }
    Ok(())
}
/// Make `evaluator` a compile-time engine of the compilation `view` sees:
/// its services, the registry of roots and `light.build`.
fn prepare_engine(
    evaluator: &Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
    view: std::rc::Weak<dyn CatalogView>,
    binding: &EngineBinding,
) -> Result<(), String> {
    if evaluator.native_forms() {
        return install_catalog_services(
            evaluator,
            view,
            binding.build.clone(),
            binding.dependency_log.clone(),
        );
    }
    install_catalog_services(
        evaluator,
        view,
        binding.build.clone(),
        binding.dependency_log.clone(),
    )?;
    // From now on the runtime reports the first extension environment data
    // could reach through the build service (light-cljs-433), including one
    // it noted before, which the registry reports when it loads.
    if let Some(tracking) = evaluator.environment_extensions()
        && tracking.get() == crate::evaluator::EnvironmentExtensions::Untracked
    {
        tracking.set(crate::evaluator::EnvironmentExtensions::None);
    }
    // Registry and root states stay in JS, so ready reads never cross Rust.
    let value = evaluator
        .evaluate_js(include_str!("compiler_roots.js"))
        .map_err(|e| e.to_string())?;
    evaluator.release(value).map_err(|e| e.to_string())?;
    // A new compilation, whose first registration names every namespace.
    if let Some(registered) = evaluator.reader_namespaces() {
        registered.borrow_mut().forget();
    }
    // A recording trace also takes the reads of ready roots, which the
    // registry notes and batches per engine entry (light-cljs-359).
    if crate::compile_trace::recording() {
        evaluator
            .install_string_service(
                "__lightRootReadService",
                Rc::new(|note| {
                    crate::compile_trace::root_reads(note);
                    String::new()
                }),
            )
            .map_err(|e| e.to_string())?;
        let value = evaluator
            .evaluate_js("__lightTraceRootReads()")
            .map_err(|e| e.to_string())?;
        evaluator.release(value).map_err(|e| e.to_string())?;
    }
    let value = evaluator
        .evaluate_js(&format!(
            "(globalThis.lightCljsBuild = ({})(globalThis.__lightBuildService, globalThis.__lightBuildCaller), null)",
            crate::build_services::BUILD_OBJECT
        ))
        .map_err(|e| e.to_string())?;
    evaluator.release(value).map_err(|e| e.to_string())?;
    // Capture the canonical runtime without a second runtime or native adapter.
    let value = evaluator
        .evaluate_unit("rt => { globalThis.__lightRuntimeForCompiler = rt; return null; }")
        .map_err(|e| e.to_string())?;
    evaluator.release(value).map_err(|e| e.to_string())
}
/// What the services installed in a compile-time engine reach: the units of
/// the compilation as the engine's thread sees them. The coordinator's
/// engine reaches its [`CompilationSession`]; a worker's engine reaches its
/// [`WorkerView`] of the shared catalog (docs/compiler-concurrency.md, "As
/// built (light-cljs-334)" and "As built (light-cljs-335)").
pub(crate) trait CatalogView {
    fn help(&self) -> Result<bool, String> {
        Ok(false)
    }
    #[cfg(feature = "bbp-macros")]
    fn run_native(
        &self,
        step: crate::bbp_evaluator::NativeStep,
    ) -> Result<big_boy_pants_js::RunOutcome, String> {
        step.run()
    }
    /// Where a native task this view parks is queued once woken: the
    /// pool of the job it runs in, if any.
    #[cfg(feature = "bbp-macros")]
    fn native_submit(&self) -> Option<crate::compiler_evaluator::NativeSubmit> {
        None
    }
    /// `__lightCompileUnit`: unit `id`'s initializer, analyzed first if it
    /// is not yet, with the registration it needs evaluated in `active`.
    fn compile_unit(&self, active: &dyn MacroEvaluator, id: usize) -> Result<String, String>;
    /// `light.build`: the source index of unit `id`, if the catalog can tell.
    fn unit_source(&self, id: usize) -> Option<u32>;
    /// `light.build`: the source indexes of the units being analyzed, the
    /// innermost last.
    fn analysis_sources(&self) -> Vec<u32>;
    /// `light.build`: the answer to `request` from the thread that has the
    /// build's resources, if this view's thread has not: a pool worker's.
    fn forward_build(&self, _request: &str) -> Option<String> {
        None
    }
}
/// `__lightCompileUnit` in `active` for a view whose analyses `analyze` runs:
/// analysis is shared by the threads, and the unit's registration is
/// evaluated in the engine of the thread that asks.
fn compile_unit_in(
    shared: &SharedCatalog,
    active: &dyn MacroEvaluator,
    id: usize,
    analyze: impl FnOnce() -> Result<(Arc<Program>, Vec<AnalysisWarning>), String>,
) -> Result<String, String> {
    #[cfg(debug_assertions)]
    if let Some(perturbation) = shared.perturbation.get()
        && let Some(name) = &perturbation.panic_in
        && shared.unit_name(id) == *name
    {
        panic!("injected panic in the compile-unit callback for {name}");
    }
    let prepared = shared.prepared(id)?;
    crate::compile_trace::executed(id, prepared.is_some());
    let register = |tables: &crate::reader_registration::ReaderTables| {
        crate::reader_registration::register(active, tables).map_err(|e| e.to_string())
    };
    // JavaScript is running: a macro's reader view may be active.
    crate::reader_registration::in_callback(active, || {
        if let Some(prepared) = prepared {
            shared.set_emitted(id);
            register(prepared.tables())?;
            return Ok(prepared.code().to_owned());
        }
        match analyze() {
            Ok((program, _)) => {
                let code = {
                    #[cfg(feature = "bbp-macros")]
                    if active.native_bridge_stats().is_some() {
                        crate::emitter::emit_native_macro_unit(
                            &program,
                            true,
                            &shared.native_replaced_core(),
                        )
                    } else {
                        crate::emitter::emit_deferred_unit(&program)
                    }
                    #[cfg(not(feature = "bbp-macros"))]
                    crate::emitter::emit_deferred_unit(&program)
                };
                shared.set_emitted(id);
                register(&crate::reader_registration::ReaderTables::of(&program))?;
                Ok(code)
            }
            // Another thread analyzed it and its application took the
            // program after this one looked; its JavaScript is prepared.
            Err(error) => match shared.prepared(id)? {
                Some(prepared) => {
                    shared.set_emitted(id);
                    register(prepared.tables())?;
                    Ok(prepared.code().to_owned())
                }
                None => Err(error),
            },
        }
    })
}
impl CatalogView for CompilationSession {
    fn compile_unit(&self, active: &dyn MacroEvaluator, id: usize) -> Result<String, String> {
        compile_unit_in(&self.shared, active, id, || self.analyze(id, Some(active)))
    }
    fn unit_source(&self, id: usize) -> Option<u32> {
        self.shared.unit_source(id)
    }
    fn analysis_sources(&self) -> Vec<u32> {
        self.shared.analysis_sources(self.thread)
    }
}
/// A worker thread's view of a [`SharedCatalog`]: the units it analyzes and
/// waits for are the compilation's, and the registrations it installs and
/// the roots it evaluates are its own engine's. Made, used and dropped on
/// the worker's thread.
pub(crate) struct WorkerView {
    help: RefCell<Option<crate::compiler_evaluator::WaitService>>,
    #[cfg(feature = "bbp-macros")]
    runner: RefCell<Option<crate::compiler_evaluator::NativeRunner>>,
    #[cfg(feature = "bbp-macros")]
    submit: RefCell<Option<crate::compiler_evaluator::NativeSubmit>>,
    shared: Arc<SharedCatalog>,
    thread: usize,
    /// The worker's host ([`MacroHost::bind`]), whose binding each unit's
    /// analysis frame is taken from.
    host: MacroHost,
    /// How many of the catalog's registrations the host's engine holds.
    installed: Cell<usize>,
    /// Whether its `light.build` requests go to the coordinator: a pool
    /// worker's ([`SharedCatalog::forward_build`]).
    forwards: Cell<bool>,
}
#[cfg_attr(not(test), allow(dead_code))]
impl WorkerView {
    /// A view of `shared` for this thread, which analyzes in frames of
    /// `host`. If the host has an engine, the engine reaches the view
    /// through its services.
    pub(crate) fn new(shared: Arc<SharedCatalog>, mut host: MacroHost) -> Result<Rc<Self>, String> {
        let thread = shared.join();
        host.engine.catalog = Some(shared.clone());
        let view = Rc::new_cyclic(|view| {
            host.engine.worker = Some(view.clone());
            Self {
                help: RefCell::new(None),
                #[cfg(feature = "bbp-macros")]
                runner: RefCell::new(None),
                #[cfg(feature = "bbp-macros")]
                submit: RefCell::new(None),
                shared,
                thread,
                host,
                installed: Cell::new(0),
                forwards: Cell::new(false),
            }
        });
        if let Some(bootstrap) = &view.host.engine.bootstrap {
            let weak: std::rc::Weak<dyn CatalogView> = Rc::downgrade(&view) as std::rc::Weak<Self>;
            prepare_engine(&bootstrap.evaluator, weak, &view.host.engine)?;
        }
        Ok(view)
    }
    /// Install in this thread's engine the registrations made since it last
    /// looked, through `active` while the engine runs code that demands.
    ///
    /// A native producer task (big-boy-pants-js) is not this thread's
    /// engine but a task of its own beside it, and is dropped once its
    /// helper is compiled: it gets the registrations, and the engine still
    /// lacks them and takes them at its next analysis. (A legacy engine's
    /// nested demand runs in the engine itself.)
    fn install(&self, active: Option<&dyn MacroEvaluator>) -> Result<(), String> {
        let Some(bootstrap) = &self.host.engine.bootstrap else {
            return Ok(());
        };
        let producer = active.is_some_and(|active| {
            active.native_bridge_stats().is_some() && active.heap() != bootstrap.evaluator.heap()
        });
        let engine = active.unwrap_or(bootstrap.evaluator.as_ref());
        // A producer imported its parent's catalog frontier, which is the
        // engine's (`installed`), and keeps its own cursor from there: a
        // registration is installed in it once, however often it demands.
        let first = match producer {
            true => engine.catalog_cursor().unwrap_or(self.installed.get()),
            false => self.installed.get(),
        };
        let pending = {
            let c = self.shared.lock();
            if c.registrations.len() == first {
                return Ok(());
            }
            let pending = c.registrations[first..].to_vec();
            match producer {
                true => engine.set_catalog_cursor(c.registrations.len()),
                false => self.installed.set(c.registrations.len()),
            }
            pending
        };
        install_registrations(engine, first, &pending, |_| Ok(()))
    }
    /// The view's native evaluator, whose task context it parks between
    /// jobs and borrows.
    #[cfg(feature = "bbp-macros")]
    fn native_shell(&self) -> Option<&crate::bbp_evaluator::BbpEvaluator> {
        self.host
            .engine
            .bootstrap
            .as_ref()?
            .evaluator
            .native_shell()
    }
    /// Take what the view that parked the task context it now runs on knew
    /// about that task: the registrations installed in it and the reader
    /// declarations sent to it. Its macro functions stay in the context.
    #[cfg(feature = "bbp-macros")]
    fn load_context(&self, state: crate::bbp_evaluator::LentState) {
        self.installed.set(state.installed);
        *self.host.engine.reader_declared.borrow_mut() = state.reader_declared;
        // Another thread may have put this heap's `gensym` in a scope since.
        crate::generated_names::forget_installed_scopes();
    }
    /// Give the view's native task context back to the runtime with what the
    /// view knows about it, for whichever slot runs the next macro unit
    /// (light-cljs-481): a slot holds no task between jobs, so a pool runs
    /// as many contexts as it runs macro units at once, each installing the
    /// catalog's registrations and evaluating source macros once.
    #[cfg(feature = "bbp-macros")]
    fn park_context(&self) {
        let Some(shell) = self.native_shell() else {
            return;
        };
        let state = crate::bbp_evaluator::LentState {
            installed: self.installed.get(),
            reader_declared: std::mem::take(&mut *self.host.engine.reader_declared.borrow_mut()),
        };
        shell.park(state);
    }
    /// Make sure the view has a native task context for a job: its own, or
    /// an idle one it borrows. False if it has none and none is idle, so the
    /// slot makes a new view, which boots one.
    #[cfg(feature = "bbp-macros")]
    fn adopt_context(&self) -> bool {
        let Some(shell) = self.native_shell() else {
            return true;
        };
        if shell.has_context() {
            return true;
        }
        match shell.adopt() {
            Some(state) => {
                self.load_context(state);
                true
            }
            None => false,
        }
    }
    /// Whether the view's engine took more headroom from the build's
    /// memory budget than a worker keeps between jobs
    /// ([`crate::evaluator::MemoryPool::keep`]), which only destroying it
    /// gives back: its worker retires it after the job, so what one unit
    /// needed does not stay taken for the rest of the build and decide
    /// which later units fit. What it holds up to that is its steady need,
    /// such as the build's macros, which a replacement would take again at
    /// once (light-cljs-459).
    fn holds_excess_headroom(&self) -> bool {
        let keep = self.shared.resources().map_or(0, |r| r.memory.keep());
        self.host
            .engine
            .bootstrap
            .as_ref()
            .is_some_and(|b| b.evaluator.headroom_held() > keep)
    }
    /// Whether the view's engine failed for good, such as by running out of
    /// memory or time: its worker retires it.
    fn terminal(&self) -> bool {
        self.host
            .engine
            .bootstrap
            .as_ref()
            .is_some_and(|b| b.evaluator.control().check().is_err())
    }
    /// Unit `id`'s analysis, by this thread or another.
    pub(crate) fn analyze(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        self.install(engine)?;
        let help = || self.help();
        analyze_unit(
            &self.shared,
            self.thread,
            id,
            engine,
            Demand {
                help: self
                    .host
                    .native_macro_backend()
                    .then_some(&help as &dyn Fn() -> Result<bool, String>),
                queued: false,
                on_worker: true,
                held: false,
                server: None,
            },
            || (self.host.engine.frame(), None),
        )
    }
}
/// A deferred unit for a pool worker to analyze, in its phase: 0 for one
/// any worker takes, 1 for one only the workers with an engine take, in a
/// pool that has workers of both kinds ([`ENGINE_PHASE`]).
enum UnitJob {
    Analyze(usize, usize, bool),
    #[cfg(feature = "bbp-macros")]
    Native(
        crate::bbp_evaluator::NativeStep,
        std::sync::mpsc::SyncSender<Result<big_boy_pants_js::RunOutcome, String>>,
    ),
}
/// The phase of a unit that may enter a compile-time engine, in a pool
/// whose first workers have engines and the rest none (light-cljs-437).
const ENGINE_PHASE: usize = 1;
impl crate::scheduler::Job for UnitJob {
    fn phase(&self) -> usize {
        match self {
            Self::Analyze(_, phase, _) => *phase,
            #[cfg(feature = "bbp-macros")]
            Self::Native(..) => 0,
        }
    }
    fn help_ready(&self) -> bool {
        match self {
            // This is the snapshot-based eligibility proof used for engine-free
            // workers, not a syntactic macro search. It excludes nested defs,
            // deferred drains, lazy references, readers and engine services;
            // MacroFreeUnit enforces it during analysis. Ordinary application
            // Var reads resolve from the snapshot and never demand an initializer.
            Self::Analyze(_, _, macro_free) => *macro_free,
            #[cfg(feature = "bbp-macros")]
            Self::Native(..) => true,
        }
    }
    #[cfg(debug_assertions)]
    fn key(&self) -> u64 {
        match self {
            Self::Analyze(id, _, _) => *id as u64,
            #[cfg(feature = "bbp-macros")]
            Self::Native(..) => u64::MAX,
        }
    }
}
/// How a build's unit pool analyzes (docs/compiler-concurrency.md, "As
/// built (light-cljs-339)").
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PoolEngines {
    /// Workers have no compile-time engine and take only macro-free units
    /// (light-cljs-338): when the macro memory limit leaves room for no
    /// worker isolate ([`isolate_count`]), macros run in a script host, or the
    /// compilation is interactive.
    None,
    /// Each worker with an engine owns one V8 isolate, or in a QuickJS-only
    /// build one QuickJS runtime (light-cljs-436), for the whole build; the
    /// workers beyond the isolates the limit allows have none and take only
    /// macro-free units (light-cljs-437).
    Persistent,
    /// Each unit is analyzed in a fresh isolate, the correctness reference
    /// of persistent ones (`LIGHT_CLJS_FRESH_UNIT_ISOLATES=1`).
    Fresh,
}
/// The pool that analyzes a build's deferred units with `--jobs N`, kept
/// for the whole build: N-1 worker threads, the first ones each owning one
/// compile-time engine made on it (after the V8 platform), and destroyed on
/// it when the pool finishes, the others none, which take only macro-free
/// units: the scheduler gives the units that may enter an engine to the
/// workers with one alone ([`ENGINE_PHASE`]). Nothing engine-related
/// enters a queue: a job is a unit ID, and a worker analyzes the unit from
/// the catalog's sendable inputs in a host bound to its own engine
/// ([`WorkerView`]).
///
/// The scheduler is driven from a thread of its own, which only submits
/// the batches the drains hand it and consumes outcomes, so that the
/// coordinator's analysis need not run inside the scheduler's closure; the
/// coordinator waits for its units in the catalog ([`SharedCatalog::claim`]).
pub(crate) struct UnitPool {
    inbox: Arc<Inbox>,
    shared: Arc<SharedCatalog>,
    thread: Option<std::thread::JoinHandle<Result<(), String>>>,
    pub(crate) engines: PoolEngines,
    /// Its worker threads: the slots but the coordinator's.
    workers: usize,
    /// Those of them with an engine: all of them, or with
    /// [`PoolEngines::None`] none, or as many as the macro memory limit
    /// leaves isolates for ([`isolate_count`]).
    engine_workers: usize,
    /// The coordinator's settings, from which it starts its own engine
    /// where a serial build would have ([`CompilationSession::start_main_engine`]).
    main: MacroEnvironment,
}
#[derive(Default)]
struct Inbox {
    state: Mutex<InboxState>,
    ready: Condvar,
}
#[derive(Default)]
struct InboxState {
    batches: std::collections::VecDeque<Vec<usize>>,
    closed: bool,
}
impl Inbox {
    fn lock(&self) -> MutexGuard<'_, InboxState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    /// The next batch, waiting for one; None once the pool closes.
    fn next(&self) -> Option<Vec<usize>> {
        let mut state = self.lock();
        loop {
            if let Some(batch) = state.batches.pop_front() {
                return Some(batch);
            }
            if state.closed {
                return None;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn try_next(&self) -> Option<Vec<usize>> {
        self.lock().batches.pop_front()
    }
    fn closed(&self) -> bool {
        self.lock().closed
    }
}
/// The analysis of the `:test` unit `unit` of a definition that `host`'s
/// frame analyzes: through the session on the coordinator, and through the
/// catalog view of a worker, whose demand runs inline on it.
pub(crate) fn analyze_definition_test(
    host: &MacroHost,
    unit: usize,
    engine: Option<&dyn MacroEvaluator>,
) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
    if let Some(session) = host.session_handle() {
        return session.analyze(unit, engine);
    }
    let worker = host.engine.worker.as_ref().and_then(std::rc::Weak::upgrade);
    worker.expect("compilation session").analyze(unit, engine)
}
/// Give the analyzed `:test` unit `unit`, whose IR its definition's
/// appended, to its application owner
/// ([`CompilationSession::release_application_ir`]) from either thread.
pub(crate) fn release_definition_test(host: &MacroHost, unit: usize) {
    match (host.session_handle(), &host.engine.catalog) {
        (Some(session), _) => {
            session.merged_ir(unit);
            session.release_application_ir(unit);
        }
        (None, Some(catalog)) => {
            catalog.merged_ir(unit);
            catalog.release_application_ir(unit);
        }
        (None, None) => {}
    }
}
/// How many compile-time isolates, the main one included, `slots` work
/// slots may run under the macro memory limit `limit`: the main one, and
/// as many workers' as what the limit leaves beyond the main one's
/// reservation gives [`crate::evaluator::WORKER_SHARE`] each (64 MiB for a
/// V8 isolate or a QuickJS runtime). The main isolate reserves what
/// a serial build with the default limit has, or the whole limit when it
/// is lower, so that a build the default limit fits at `--jobs 1` fits
/// at every `--jobs` (light-cljs-437): the default 64 MiB leaves no room
/// for a worker isolate. They share the limit as one budget
/// ([`crate::evaluator::MemoryPool`], docs/compiler-concurrency.md, "As
/// built (light-cljs-341)"); the slots beyond them run workers without an
/// engine.
pub(crate) fn isolate_count(limit: usize, slots: crate::scheduler::Slots) -> usize {
    let reserved = limit.min(crate::evaluator::DEFAULT_MEMORY_LIMIT);
    1 + ((limit - reserved) / crate::evaluator::WORKER_SHARE).min(slots.workers())
}

#[cfg(test)]
mod share_tests {
    use crate::evaluator::MemoryPool;
    use crate::scheduler::Slots;
    /// `--stats`' `catalog.lock-contended` counts the acquisitions of the
    /// catalog's lock that found it held, and no others (light-cljs-343).
    #[test]
    fn a_contended_acquisition_of_the_catalog_lock_is_counted_once() {
        use std::sync::atomic::Ordering::Relaxed;
        let catalog = super::SharedCatalog::default();
        let contended = || catalog.counters.catalog_contended.load(Relaxed);
        drop(catalog.lock());
        assert_eq!(contended(), 0);
        let held = catalog.lock();
        std::thread::scope(|scope| {
            let waiter = scope.spawn(|| drop(catalog.lock()));
            while contended() == 0 {
                std::thread::yield_now();
            }
            drop(held);
            waiter.join().unwrap();
        });
        assert_eq!(contended(), 1);
        drop(catalog.lock());
        assert_eq!(contended(), 1);
    }
    #[test]
    fn isolates_share_the_memory_limit_without_exceeding_it() {
        let slots = |n| Slots::new(n).unwrap();
        // The default 64 MiB is the main isolate's alone, whatever N
        // (light-cljs-437): the other slots run workers without an engine.
        assert_eq!(super::isolate_count(64 << 20, slots(8)), 1);
        assert_eq!(super::isolate_count(64 << 20, slots(2)), 1);
        // Beyond the main isolate's 64 MiB, a worker isolate, or a QuickJS
        // runtime in QuickJS-only builds, per 64 MiB (light-cljs-436 and
        // light-cljs-441).
        let share = crate::evaluator::WORKER_SHARE;
        assert_eq!(share, 64 << 20);
        assert_eq!(super::isolate_count(96 << 20, slots(8)), 1);
        assert_eq!(super::isolate_count(128 << 20, slots(8)), 2);
        assert_eq!(super::isolate_count(256 << 20, slots(4)), 4);
        assert_eq!(super::isolate_count(256 << 20, slots(8)), 4);
        assert_eq!(super::isolate_count(4_000_000_000, slots(8)), 8);
        // Less than the reservation: one isolate with the whole limit.
        assert_eq!(super::isolate_count(40 << 20, slots(4)), 1);
        assert_eq!(super::isolate_count(16 << 20, slots(4)), 1);
        // Each worker isolate's base, the main one's and the headroom they
        // share never sum to more than the limit, and the main one has at
        // least what a serial build with the default limit has.
        for limit in [
            64 << 20,
            96 << 20,
            100 << 20,
            128 << 20,
            160 << 20,
            256 << 20,
            1 << 30,
            4_000_000_000,
        ] {
            for n in 2..=12 {
                let isolates = super::isolate_count(limit, slots(n));
                let pool = MemoryPool::new(limit, isolates);
                assert!(pool.base() >= share.min(limit / isolates), "{limit} {n}");
                assert!(pool.main_base() >= 64 << 20, "{limit} {n}");
                assert_eq!(
                    pool.main_base() + (isolates - 1) * pool.base() + pool.headroom(),
                    limit,
                    "{limit} {n}"
                );
                // What the workers keep between jobs leaves at least a
                // worker's part of the headroom (light-cljs-459).
                assert!(
                    pool.headroom() - (isolates - 1) * pool.keep() >= pool.keep(),
                    "{limit} {n}"
                );
            }
        }
        // The 800k generated project's 2 GiB at 8, 12 and 16 slots: its
        // workers' isolates hold about 90 to 100 MiB once they have its
        // macros, within what they keep.
        for (n, base, keep) in [(8, 128, 128), (12, 85, 85), (16, 64, 64)] {
            let pool = MemoryPool::new(2 << 30, super::isolate_count(2 << 30, slots(n)));
            assert_eq!((pool.base() >> 20, pool.keep() >> 20), (base, keep), "{n}");
        }
        // 64 MiB and a share for two: the main isolate's 64 MiB and a
        // worker's share.
        let pool = MemoryPool::new((64 << 20) + share, 2);
        assert_eq!(
            (pool.main_base(), pool.base(), pool.headroom()),
            (64 << 20, share, 0)
        );
        // 64 MiB, three shares and 96 MiB for four: 64 MiB for the main
        // isolate, a share for each worker's and 96 MiB to grow into, taken
        // whole or in part and given back.
        let pool = MemoryPool::new((64 << 20) + 3 * share + (96 << 20), 4);
        assert_eq!(
            (pool.main_base(), pool.base(), pool.headroom()),
            (64 << 20, share, 96 << 20)
        );
        assert_eq!(pool.keep(), 24 << 20);
        assert_eq!(pool.take(68 << 20, 1), 68 << 20);
        assert_eq!(pool.take(100 << 20, 1), 28 << 20);
        assert_eq!(pool.take(1, 1), 0);
        pool.give(50 << 20);
        assert_eq!(pool.take(60 << 20, 60 << 20), 0);
        assert_eq!(pool.take(60 << 20, 40 << 20), 50 << 20);
    }
}
impl UnitPool {
    /// Start the pool of `slots` for the units of `shared`, of which
    /// `isolates`, the coordinator's included, have `engines`, the others
    /// none. A worker's host is bound to `environment`, the compilation
    /// host's settings.
    pub(crate) fn start(
        shared: Arc<SharedCatalog>,
        slots: crate::scheduler::Slots,
        engines: PoolEngines,
        isolates: usize,
        environment: MacroEnvironment,
    ) -> Result<Self, String> {
        let engine_workers = match engines {
            PoolEngines::None => 0,
            _ => isolates.saturating_sub(1).min(slots.workers()),
        };
        let engines = match engine_workers {
            0 => PoolEngines::None,
            _ => engines,
        };
        let inbox = Arc::new(Inbox::default());
        // Started on the coordinator: its workers record their units'
        // traces while it records its own.
        let traced = crate::compile_trace::recording();
        let thread = {
            let inbox = inbox.clone();
            let shared = shared.clone();
            let environment = environment.clone();
            std::thread::Builder::new()
                .name("light-cljs-units".into())
                .spawn(move || {
                    let engines = (engines, engine_workers);
                    serve_pool(&inbox, &shared, slots, engines, &environment, traced)
                })
                .map_err(|e| format!("cannot start the compiler's unit pool: {e}"))?
        };
        Ok(Self {
            inbox,
            shared,
            thread: Some(thread),
            engines,
            workers: slots.workers(),
            engine_workers,
            main: environment,
        })
    }
    /// Give the pool `units`, one namespace's, in unit order: one batch,
    /// which the scheduler's workers claim and steal in that order.
    pub(crate) fn submit(&self, units: Vec<usize>) {
        if units.is_empty() {
            return;
        }
        (self.shared.counters.pooled).fetch_add(units.len(), std::sync::atomic::Ordering::Relaxed);
        self.inbox.lock().batches.push_back(units);
        self.inbox.ready.notify_one();
    }
    /// Stop the pool once its jobs have run and join its threads, which
    /// destroy their engines. A worker's panic panics the build here, as
    /// it does when the pool is dropped.
    pub(crate) fn finish(mut self) {
        if let Some(message) = self.join() {
            panic!("{message}");
        }
    }
    fn join(&mut self) -> Option<String> {
        // Nothing waits for a unit from here on; a worker waiting for the
        // coordinator to answer its request stops waiting.
        self.shared.abandon("the build ended".into());
        self.inbox.lock().closed = true;
        self.inbox.ready.notify_all();
        let thread = self.thread.take()?;
        match thread.join() {
            Ok(Ok(())) => None,
            Ok(Err(message)) => Some(message),
            Err(_) => Some("the compiler's unit pool panicked".into()),
        }
    }
}
impl Drop for UnitPool {
    fn drop(&mut self) {
        if let Some(message) = self.join() {
            self.shared.abandon(message.clone());
            if !std::thread::panicking() {
                panic!("{message}");
            }
        }
    }
}
/// A worker's view of `shared`, bound to an engine made on this thread.
fn worker_view(
    shared: &Arc<SharedCatalog>,
    engines: PoolEngines,
    environment: &MacroEnvironment,
) -> Result<Rc<WorkerView>, String> {
    let started = std::time::Instant::now();
    #[cfg(feature = "bbp-macros")]
    let mut lent = None;
    let engine: Option<Rc<dyn crate::compiler_evaluator::CompilerEvaluator>> = match engines {
        PoolEngines::None => None,
        // A persistent slot borrows a task context another slot parked, if
        // one is idle (`WorkerView::park_context`).
        #[cfg(feature = "bbp-macros")]
        PoolEngines::Persistent if environment.native_macro_backend() => {
            let (evaluator, state) = shared
                .native_runtime(environment.evaluator_options())
                .and_then(|runtime| runtime.lent_evaluator())
                .map_err(|e| e.to_string())?;
            lent = state;
            Some(Rc::new(evaluator))
        }
        #[cfg(feature = "bbp-macros")]
        PoolEngines::Fresh if environment.native_macro_backend() => Some(Rc::new(
            shared
                .native_runtime(environment.evaluator_options())
                .and_then(|runtime| runtime.evaluator())
                .map_err(|e| e.to_string())?,
        )),
        #[cfg(light_v8)]
        PoolEngines::Persistent | PoolEngines::Fresh => Some(Rc::new(
            match shared.resources() {
                Some(resources) => crate::v8_evaluator::V8Evaluator::new_for_build(
                    environment.evaluator_options().clone(),
                    resources,
                    false,
                ),
                None => {
                    crate::v8_evaluator::V8Evaluator::new(environment.evaluator_options().clone())
                }
            }
            .map_err(|e| e.to_string())?,
        )),
        // QuickJS-only builds: one QuickJS runtime per worker, made here on
        // the worker's thread and freed on it, which no runtime leaves (the
        // evaluator is neither `Send` nor `Sync`; light-cljs-436).
        #[cfg(all(light_quickjs, not(light_v8)))]
        PoolEngines::Persistent | PoolEngines::Fresh => Some(Rc::new(
            match shared.resources() {
                Some(resources) => crate::evaluator::Evaluator::new_for_build(
                    environment.evaluator_options().clone(),
                    resources,
                    false,
                ),
                None => crate::evaluator::Evaluator::new(environment.evaluator_options().clone()),
            }
            .map_err(|e| e.to_string())?,
        )),
        #[cfg(not(any(light_v8, light_quickjs)))]
        PoolEngines::Persistent | PoolEngines::Fresh => {
            return Err("compiler workers need an embedded engine".into());
        }
    };
    let counted = engine.is_some();
    let view = WorkerView::new(
        shared.clone(),
        MacroHost::bind(environment.clone(), engine)?,
    )?;
    view.forwards.set(true);
    #[cfg(feature = "bbp-macros")]
    if let Some(state) = lent {
        view.load_context(state);
    }
    if counted {
        let counters = &shared.counters;
        counters
            .isolates
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        PoolCounters::add(&counters.startup_ns, started);
        #[cfg(debug_assertions)]
        if let Some(perturbation) = shared.perturbation.get() {
            (perturbation.isolates).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
    }
    Ok(view)
}
/// A pool slot's state, made and dropped on its thread: its catalog view,
/// and its trace recording, made before the view so that the view's engine
/// reports root reads.
struct WorkerSlot {
    view: Option<Rc<WorkerView>>,
    /// On the native backend, the slot's view without an engine, in which
    /// it analyzes the units proven macro-free ([`UnitJob::help_ready`]):
    /// such a unit never enters an engine, so a worker, or a waiting
    /// worker's nested help, boots no runtime task and installs no
    /// registrations for it.
    plain: Option<Rc<WorkerView>>,
    /// The slot's engines: the pool's for the first workers, none for the
    /// ones beyond the isolates the macro memory limit allows.
    engines: PoolEngines,
    _trace: Option<crate::compile_trace::Recording>,
}
thread_local! {
    static HELP_DEPTH: Cell<usize> = const { Cell::new(0) };
}
struct HelpDepth(usize);
impl Drop for HelpDepth {
    fn drop(&mut self) {
        HELP_DEPTH.set(self.0);
    }
}
/// The pool's thread: run the scheduler, submitting each batch the inbox
/// gets and consuming outcomes, until the inbox closes and every job has
/// run. A worker's panic stops the pool and ends every wait in the catalog.
fn help_pool(
    helper: &crate::scheduler::HelpHandle<UnitJob, ()>,
    engines: PoolEngines,
    shared: &Arc<SharedCatalog>,
    environment: &MacroEnvironment,
) -> Result<bool, String> {
    match helper
        .help(
            |_| WorkerSlot {
                view: None,
                plain: None,
                engines,
                _trace: None,
            },
            |slot, job, worker| execute_pool_job(slot, job, worker, shared, environment),
        )
        .map_err(|e| e.to_string())?
    {
        crate::scheduler::Help::Ran => Ok(true),
        crate::scheduler::Help::Idle => Ok(false),
        crate::scheduler::Help::DepthLimit => Err("compiler help nesting limit exceeded".into()),
    }
}

fn execute_pool_job(
    slot: &mut WorkerSlot,
    job: UnitJob,
    worker: &crate::scheduler::Worker<'_, UnitJob, ()>,
    shared: &Arc<SharedCatalog>,
    environment: &MacroEnvironment,
) {
    let _depth = HelpDepth(HELP_DEPTH.replace(worker.help_depth()));
    let (id, macro_free) = match job {
        UnitJob::Analyze(id, _, macro_free) => (id, macro_free),
        #[cfg(feature = "bbp-macros")]
        UnitJob::Native(step, reply) => {
            #[cfg(test)]
            {
                let hook = shared.on_native_run.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(step.task_id(), worker.slot());
                }
            }
            let _ = reply.send(step.run());
            return;
        }
    };
    if !shared.admits(id) {
        return shared.skipped();
    }
    #[cfg(feature = "bbp-macros")]
    let plain =
        macro_free && slot.engines != PoolEngines::None && environment.native_macro_backend();
    #[cfg(not(feature = "bbp-macros"))]
    let plain = {
        let _ = macro_free;
        false
    };
    #[cfg_attr(not(feature = "bbp-macros"), allow(unused_mut))]
    let (mut kept, engines) = if plain {
        (slot.plain.take(), PoolEngines::None)
    } else {
        (slot.view.take(), slot.engines)
    };
    // A native view parked its task context after its last job: it borrows
    // an idle one, or makes way for a view that boots one.
    #[cfg(feature = "bbp-macros")]
    if kept.as_ref().is_some_and(|view| !view.adopt_context()) {
        kept = None;
    }
    let view = match kept {
        Some(view) => view,
        None => match worker_view(shared, engines, environment) {
            Ok(view) => view,
            Err(error) => {
                shared.fail_unstarted(id, error);
                return shared.note_job(id);
            }
        },
    };
    let engines = slot.engines;
    let control = view
        .host
        .engine
        .bootstrap
        .as_ref()
        .filter(|b| b.evaluator.native_forms())
        .map(|b| b.evaluator.control());
    let wait_control = control.clone();
    let helper = worker.help_handle();
    let catalog = shared.clone();
    let settings = environment.clone();
    *view.help.borrow_mut() = Some(Rc::new(move || {
        if let Some(control) = &wait_control {
            control.poll().map_err(|e| e.to_string())?;
        }
        help_pool(&helper, engines, &catalog, &settings)
    }));
    #[cfg(feature = "bbp-macros")]
    {
        let submitter = worker.help_handle().submitter();
        *view.submit.borrow_mut() = Some(std::sync::Arc::new(move |step, reply| {
            submitter.push_urgent(UnitJob::Native(step, reply));
        }));
        let helper = worker.help_handle();
        let catalog = shared.clone();
        let settings = environment.clone();
        #[cfg(test)]
        let owner = worker.slot();
        *view.runner.borrow_mut() = Some(Rc::new(move |step| {
            let mut handoff = step.handoff();
            let (send, receive) = std::sync::mpsc::sync_channel(1);
            #[cfg(test)]
            let task = step.task_id();
            #[cfg(test)]
            {
                let hook = catalog.on_native_submit.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(task, owner, false);
                }
            }
            helper.push_urgent(UnitJob::Native(step, send));
            #[cfg(test)]
            {
                let hook = catalog.on_native_submit.lock().unwrap().clone();
                if let Some(hook) = hook {
                    hook(task, owner, true);
                }
            }
            loop {
                match receive.try_recv() {
                    Ok(outcome) => {
                        handoff.finish();
                        return outcome;
                    }
                    Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                        return Err("native task worker stopped".into());
                    }
                    Err(std::sync::mpsc::TryRecvError::Empty) => {}
                }
                if helper.cancelled() {
                    return Err("compiler pool cancelled".into());
                }
                if let Some(control) = &control {
                    control.poll().map_err(|e| e.to_string())?;
                }
                if help_pool(&helper, engines, &catalog, &settings)? {
                    continue;
                }
                match receive.recv_timeout(std::time::Duration::from_millis(20)) {
                    Ok(outcome) => {
                        handoff.finish();
                        return outcome;
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                        return Err("native task worker stopped".into());
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                }
            }
        }));
    }
    let started = std::time::Instant::now();
    #[cfg(debug_assertions)]
    if let Some(perturbation) = shared.perturbation.get()
        && perturbation.lane_hold.contains(&shared.unit_name(id))
    {
        shared.lanes.wait_parked(std::time::Duration::from_secs(10));
    }
    // The job is an activity of its own at its lane, even helped inside
    // another's wait.
    let root = shared.lanes.enter_root();
    let analysis = view.analyze(id, None);
    drop(root);
    // A task-local native failure can finish this VM without terminating
    // the shared control. Keep other consumers alive, but retire this view.
    let reusable = !view.host.native_macro_backend() || analysis.is_ok();
    if let Err(error) = analysis {
        shared.fail_unstarted(id, error);
    }
    shared.note_job(id);
    PoolCounters::add(&shared.counters.unit_ns, started);
    if slot.engines == PoolEngines::None || plain {
        shared
            .counters
            .engine_less_units
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if plain {
        slot.plain = Some(view);
    } else if reusable
        && slot.engines != PoolEngines::Fresh
        && !view.terminal()
        && !view.holds_excess_headroom()
    {
        #[cfg(feature = "bbp-macros")]
        view.park_context();
        slot.view = Some(view);
    }
}

/// Whether the build's test perturbation holds units back for a lane
/// ([`crate::scheduler::Perturbation::lane_hold`]); never in release builds.
#[cfg(feature = "bbp-macros")]
fn lane_hold(shared: &SharedCatalog) -> bool {
    #[cfg(debug_assertions)]
    {
        (shared.perturbation.get()).is_some_and(|p| !p.lane_hold.is_empty())
    }
    #[cfg(not(debug_assertions))]
    {
        let _ = shared;
        false
    }
}
/// Analyze `unit`, the earliest unfinished one, on a thread of its own, if
/// no thread has started it: every thread that could may be waiting for a
/// lane behind it ([`crate::effect_lanes::Lanes::rescue`]). It runs as a
/// pool job does, on a borrowed or new task context; the pool's job for it
/// then reuses the analysis.
#[cfg(feature = "bbp-macros")]
fn rescue_unit(
    shared: &std::sync::Weak<SharedCatalog>,
    environment: &MacroEnvironment,
    unit: usize,
) -> Option<std::thread::JoinHandle<()>> {
    let shared = shared.upgrade()?;
    {
        let c = shared.lock();
        if c.abandoned.is_some() || !matches!(c.units.get(unit)?.state, AnalysisState::Unstarted) {
            return None;
        }
    }
    if !shared.admits(unit) {
        return None;
    }
    let environment = environment.clone();
    std::thread::Builder::new()
        .name("light-cljs-lane-rescue".into())
        .stack_size(8 << 20)
        .spawn(move || {
            let root = shared.lanes.enter_root();
            let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let view = match worker_view(&shared, PoolEngines::Persistent, &environment) {
                    Ok(view) => view,
                    Err(error) => return shared.fail_unstarted(unit, error),
                };
                if let Err(error) = view.analyze(unit, None) {
                    shared.fail_unstarted(unit, error);
                }
                if !view.terminal() {
                    view.park_context();
                }
            }));
            drop(root);
            shared.note_job(unit);
            if outcome.is_err() {
                shared.abandon("a lane rescue thread panicked".into());
            }
        })
        .ok()
}
fn serve_pool(
    inbox: &Inbox,
    shared: &Arc<SharedCatalog>,
    slots: crate::scheduler::Slots,
    (engines, engine_workers): (PoolEngines, usize),
    environment: &MacroEnvironment,
    traced: bool,
) -> Result<(), String> {
    use crate::scheduler::{Config, Next};
    let started = std::time::Instant::now();
    let counters = &shared.counters;
    // A lane that waits for an earliest unit no free thread will start has
    // it analyzed on a thread of its own (light-cljs-480).
    // (A test's lane hold keeps units back on purpose, in debug builds.)
    #[cfg(feature = "bbp-macros")]
    if environment.native_macro_backend()
        && engines == PoolEngines::Persistent
        && !lane_hold(shared)
    {
        let weak = Arc::downgrade(shared);
        let environment = environment.clone();
        shared.lanes.set_rescuer(Some(Arc::new(move |unit| {
            rescue_unit(&weak, &environment, unit)
        })));
    }
    let relaxed = std::sync::atomic::Ordering::Relaxed;
    (counters.workers).store(slots.workers(), relaxed);
    (counters.engine_workers).store(engine_workers, relaxed);
    // With workers of both kinds, the units that may enter an engine are a
    // phase of their own, which only the first workers, those with an
    // engine, serve; any worker serves the macro-free ones.
    let mixed = engine_workers > 0 && engine_workers < slots.workers();
    let mut config = Config::new(slots, if mixed { 2 } else { 1 });
    if mixed {
        config.servers = vec![slots.workers(), engine_workers];
    }
    // A worker analyzes as deep as the coordinator, whose main thread has
    // 8 MiB; V8's stack limit is set from where each engine entry starts.
    config.stack_size = Some(8 << 20);
    #[cfg(debug_assertions)]
    {
        config.perturbation = shared.perturbation.get().cloned();
    }
    let run = crate::scheduler::run(
        &config,
        // A slot makes its view on its first job, and again after its
        // engine failed for good, which is retired, not reused; with
        // `Fresh`, for every job. While the coordinator records a trace,
        // the slot's thread records its units' for it (light-cljs-369).
        |slot| WorkerSlot {
            view: None,
            plain: None,
            engines: match slot <= engine_workers {
                true => engines,
                false => PoolEngines::None,
            },
            _trace: traced.then(|| crate::compile_trace::record_worker(slot)),
        },
        |slot, job, worker| execute_pool_job(slot, job, worker, shared, environment),
        |pool| -> Result<(), String> {
            let served = loop {
                match pool.next_outcome() {
                    Ok(Next::Outcome(())) => {
                        // The build ended early: what is queued is not needed.
                        if inbox.closed() {
                            pool.cancel();
                        }
                        if let Some(batch) = inbox.try_next() {
                            pool.submit_batch(shared.unit_jobs(batch, mixed));
                        }
                    }
                    Ok(Next::Quiescent) => match inbox.next() {
                        Some(batch) => pool.submit_batch(shared.unit_jobs(batch, mixed)),
                        None => break Ok(()),
                    },
                    Ok(Next::Stalled { .. }) => unreachable!("unit jobs hold no demands"),
                    Err(panic) => {
                        let message = panic.to_string();
                        shared.abandon(message.clone());
                        pool.cancel();
                        break Err(message);
                    }
                }
            };
            *counters
                .scheduler
                .lock()
                .unwrap_or_else(PoisonError::into_inner) = pool.counts();
            PoolCounters::add(&counters.pool_ns, started);
            served
        },
    );
    // No slot borrows a parked task context any more.
    #[cfg(feature = "bbp-macros")]
    if let Some(Ok(runtime)) = shared.native_runtime.get() {
        runtime.clear_idle();
    }
    match run {
        Ok(result) => result,
        Err(error) => {
            let message = error.to_string();
            shared.abandon(message.clone());
            Err(message)
        }
    }
}
impl CatalogView for WorkerView {
    fn help(&self) -> Result<bool, String> {
        let help = self.help.borrow().clone();
        help.map_or(Ok(false), |help| help())
    }
    #[cfg(feature = "bbp-macros")]
    fn run_native(
        &self,
        step: crate::bbp_evaluator::NativeStep,
    ) -> Result<big_boy_pants_js::RunOutcome, String> {
        let runner = self.runner.borrow().clone();
        match runner {
            Some(run) => run(step),
            None => step.run(),
        }
    }
    #[cfg(feature = "bbp-macros")]
    fn native_submit(&self) -> Option<crate::compiler_evaluator::NativeSubmit> {
        self.submit.borrow().clone()
    }
    fn compile_unit(&self, active: &dyn MacroEvaluator, id: usize) -> Result<String, String> {
        self.install(Some(active))?;
        (self.shared.counters.initializations).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        compile_unit_in(&self.shared, active, id, || self.analyze(id, Some(active)))
    }
    fn unit_source(&self, id: usize) -> Option<u32> {
        self.shared.unit_source(id)
    }
    fn analysis_sources(&self) -> Vec<u32> {
        self.shared.analysis_sources(self.thread)
    }
    fn forward_build(&self, request: &str) -> Option<String> {
        self.forwards.get().then(|| {
            let (reply, log) = self
                .shared
                .forward_build(request, self.shared.analysis_sources(self.thread));
            // What it recorded is the unit's this worker records.
            if let Some(log) = log {
                crate::macro_host::record_observations(*log);
            }
            reply
        })
    }
}
/// Install in `evaluator` the services through which its code reaches the
/// compilation: `__lightCompileUnit`, which demands units from `view`, and
/// `light.build`'s requests, which `build` serves and `log` records. The
/// view is weak: the catalog may own the engine.
/// What the runtime sends `__lightBuildService` to report an extension
/// environment data could reach; a `light.build` request has five
/// NUL-separated parts ([`crate::build_services::serve_request`]).
const ENVIRONMENT_EXTENSION_REPORT: &str = "environment-extension";
pub(crate) fn install_catalog_services(
    evaluator: &Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
    view: std::rc::Weak<dyn CatalogView>,
    build: crate::build_services::SharedBuild,
    log: Rc<RefCell<Option<crate::dependency_manifest::ObservationLog>>>,
) -> Result<(), String> {
    let waiting = view.clone();
    evaluator.install_wait_service(Rc::new(move || {
        waiting
            .upgrade()
            .ok_or("compilation session expired")?
            .help()
    }));
    #[cfg(feature = "bbp-macros")]
    {
        let running = view.clone();
        evaluator.install_native_runner(Rc::new(move |step| {
            running
                .upgrade()
                .ok_or("compilation session expired")?
                .run_native(step)
        }));
        let submitting = view.clone();
        evaluator.install_native_submitter(Rc::new(move || submitting.upgrade()?.native_submit()));
    }
    // `light.build`: a request names the units whose initializers run; the
    // catalog knows the units being analyzed. The runtime also reports on it
    // the first extension environment data could reach, which is this
    // engine's own (light-cljs-433, `noteEnvironmentExtension` in build.rs):
    // a service of its own would cost every compilation an engine entry.
    let requests = view.clone();
    let extensions = evaluator.environment_extensions();
    evaluator
        .install_string_service(
            "__lightBuildService",
            Rc::new(move |request| {
                if request == ENVIRONMENT_EXTENSION_REPORT {
                    if let Some(tracking) = &extensions {
                        tracking.set(crate::evaluator::EnvironmentExtensions::Noted);
                    }
                    return String::new();
                }
                let view = requests.upgrade();
                if let Some(answer) = view.as_ref().and_then(|view| view.forward_build(request)) {
                    return answer;
                }
                let analysis = view
                    .as_ref()
                    .map_or_else(Vec::new, |view| view.analysis_sources());
                let units = |unit| view.as_ref()?.unit_source(unit);
                // A unit this thread records logs what the request records
                // in its record, as a worker's forwarded request does, for
                // the drain point that charges it (light-cljs-391).
                if crate::macro_host::MacroFreeUnit::active() && log.borrow().is_some() {
                    let own = log.borrow().as_ref().map(|log| {
                        crate::dependency_manifest::ObservationLog {
                            window: log.window,
                            ..Default::default()
                        }
                    });
                    let own = Rc::new(RefCell::new(own));
                    let answer = crate::build_services::serve_request(
                        &build, &own, request, units, analysis,
                    );
                    if let Some(recorded) = own.take() {
                        crate::macro_host::record_observations(recorded);
                    }
                    return answer;
                }
                crate::build_services::serve_request(&build, &log, request, units, analysis)
            }),
        )
        .map_err(|e| e.to_string())?;
    evaluator
        .clone()
        .install_compiler_service(
            "__lightCompileUnit",
            Rc::new(move |active, id| {
                let view = view.upgrade().ok_or("compilation session expired")?;
                view.compile_unit(active, id as usize)
            }),
        )
        .map_err(|e| e.to_string())
}
impl Clone for DefinitionSnapshot {
    fn clone(&self) -> Self {
        Self {
            namespace: self.namespace.clone(),
            aliases: self.aliases.clone(),
            requires: self.requires.clone(),
            resolution: self.resolution.clone(),
            resolution_program: self.resolution_program.clone(),
            global_names: self.global_names.clone(),
            namespace_globals: self.namespace_globals.clone(),
            record_layouts: self.record_layouts.clone(),
            session_namespaces: self.session_namespaces.clone(),
            options: self.options,
            unchecked_arrays: self.unchecked_arrays,
            source_index: self.source_index,
            definition_name: self.definition_name.clone(),
            definition_test: self.definition_test.clone(),
            definition_test_unit: self.definition_test_unit,
            extension_frontier: self.extension_frontier,
            names: self.names.clone(),
        }
    }
}
/// Settles a claimed unit however its analysis ends: with `state`, or as
/// interrupted.
struct AnalysisGuard<'a> {
    shared: &'a SharedCatalog,
    thread: usize,
    id: usize,
    state: Option<AnalysisState>,
    /// The analyzed IR's estimated bytes, when measured.
    held: usize,
}
impl Drop for AnalysisGuard<'_> {
    fn drop(&mut self) {
        self.shared
            .settle(self.thread, self.id, self.state.take(), self.held);
    }
}

pub(crate) struct FrontierGuard<'a> {
    engine: &'a dyn MacroEvaluator,
}
impl<'a> FrontierGuard<'a> {
    pub(crate) fn bootstrap(engine: &'a dyn MacroEvaluator) -> Result<Self, String> {
        // Used only after attaching a compilation with user declarations.
        engine
            .compiler_frontier(Some(0))
            .map_err(|e| e.to_string())?;
        Ok(Self { engine })
    }
}
impl Drop for FrontierGuard<'_> {
    fn drop(&mut self) {
        // Terminal cancellation makes this heap unusable; no later demand runs.
        let _ = self.engine.compiler_frontier(None);
    }
}

#[cfg(test)]
mod environment_bundle_tests {
    use super::*;

    /// The extension bundles and how many of them environment data reaches.
    fn counts(source: &str) -> (usize, usize) {
        let mut host = MacroHost::new();
        crate::compile_with_macros(&format!("(defmacro m [] 1) {source} (m)"), &mut host).unwrap();
        let session = host.session_handle().unwrap();
        (session.extension_frontier(), session.environment_bundles())
    }

    /// Extensions of source protocols, of user types and of multimethods
    /// are out of environment data's reach; those of core protocols with a
    /// native, `default`, computed or core type selector are not
    /// (light-cljs-433).
    #[test]
    fn only_core_protocol_extensions_of_core_or_unknown_types_reach_environment_data() {
        let unreachable = "(defprotocol P (f [x])) (deftype T [v]) (defrecord R [v])
            (extend-protocol P string (f [x] x) nil (f [x] x) default (f [x] x) object (f [x] x))
            (extend-type T IEquiv (-equiv [a b] true) IHash (-hash [_] 1))
            (defmulti g identity) (defmethod g :a [x] x) (defonce h 1)";
        let (bundles, reaching) = counts(unreachable);
        assert!(bundles >= 5, "{bundles}");
        assert_eq!(reaching, 0);
        for reachable in [
            "(extend-type Symbol IEquiv (-equiv [a b] true))",
            "(extend-type Keyword IHash (-hash [_] 1))",
            "(extend-type string IHash (-hash [_] 1))",
            "(extend-type number IEquiv (-equiv [a b] true))",
            "(extend-type default ILookup (-lookup [x k] k))",
            "(extend-type object IEquiv (-equiv [a b] true))",
            "(extend-type nil ISeqable (-seq [_] nil))",
            "(def C (.-constructor (js-obj))) (extend-type C IEquiv (-equiv [a b] true))",
            "(extend-type PersistentArrayMap ICounted (-count [_] 0))",
        ] {
            let (_, reaching) = counts(&format!("{unreachable} {reachable}"));
            assert_eq!(reaching, 1, "{reachable}");
        }
    }

    /// What the compile-time engine noted of extensions its code installed.
    fn noted(source: &str) -> crate::evaluator::EnvironmentExtensions {
        let mut host = MacroHost::new();
        crate::compile_with_macros(source, &mut host).unwrap();
        let bootstrap = host.session_handle().unwrap().bootstrap().unwrap();
        bootstrap.evaluator.environment_extensions().unwrap().get()
    }

    /// An extension compile-time code installs outside a declaration is
    /// noted when environment data could reach it: of a core protocol on a
    /// native, core or unknown type or on any instance, whose type is not
    /// asked for, but not of a source protocol or on a user type
    /// (light-cljs-433).
    #[test]
    fn compile_time_extensions_that_environment_data_could_reach_are_noted() {
        use crate::evaluator::EnvironmentExtensions;
        let installed = |body: &str| {
            noted(&format!(
                "(defprotocol P (f [x])) (deftype T [v]) (defrecord R [v])
                 (defn install [] {body}) (defmacro setup [] (install) nil) (setup)"
            ))
        };
        assert_eq!(noted("(defmacro m [] 1) (m)"), EnvironmentExtensions::None);
        for body in [
            "(extend-type T IEquiv (-equiv [a b] true))",
            "(extend-type R IHash (-hash [_] 1))",
            "(extend-protocol P string (f [x] x) default (f [x] x) nil (f [x] x))",
            "(specify! (js-obj) P (f [x] x))",
        ] {
            assert_eq!(installed(body), EnvironmentExtensions::None, "{body}");
        }
        for body in [
            "(extend-type Symbol IEquiv (-equiv [a b] (identical? (str a) (str b))))",
            "(extend-type string IHash (-hash [_] 1))",
            "(specify! (js-obj) ICounted (-count [_] 0))",
            "(specify! (T. 1) IEquiv (-equiv [a b] true))",
            "(extend-type T Object (toString [_] \"t\")) (extend-type number IPrintWithWriter (-pr-writer [_ w _] nil))",
        ] {
            assert_eq!(installed(body), EnvironmentExtensions::Noted, "{body}");
        }
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;

    #[test]
    fn completed_units_release_their_analysis_inputs() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defn f [x] (+ x 1)) (defmacro m [] (f 41)) (m)", &mut host)
            .unwrap();
        let session = host.session_handle().unwrap();
        let catalog = session.shared.lock();
        let bindings = session.unit_bindings.borrow();
        let mut completed = 0;
        for (id, unit) in catalog.units.iter().enumerate() {
            if matches!(
                unit.state,
                AnalysisState::Analyzed(..) | AnalysisState::Prepared(..)
            ) {
                completed += 1;
                assert!(unit.inputs.is_none());
                assert!(bindings.get(id).is_none_or(Option::is_none));
            }
        }
        assert!(completed > 0);
    }

    #[test]
    fn application_assembly_releases_programs_but_retains_lazy_source() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defn f [x] (+ x 1)) (defmacro m [] (f 41)) (m)", &mut host)
            .unwrap();
        let session = host.session_handle().unwrap();
        let catalog = session.shared.lock();
        let mut released = 0;
        for unit in &catalog.units {
            if let AnalysisState::Prepared(program, _, code) = &unit.state {
                assert!(program.upgrade().is_none());
                assert!(!code.code().is_empty());
                released += 1;
            }
        }
        assert!(released > 0);
    }
}

#[cfg(test)]
mod worker_tests {
    //! A unit analyzed by a worker from what it can receive (docs/compiler-concurrency.md,
    //! "As built (light-cljs-334)").
    use super::*;
    use crate::macro_host::MacroEnvironment;

    /// What a worker receives to analyze one unit.
    type Job = (usize, Form, DefinitionSnapshot, MacroEnvironment);

    thread_local! {
        static RETAINED: RefCell<Option<Vec<Job>>> = const { RefCell::new(None) };
    }

    /// While a test retains them, keep the inputs of each unit registered on
    /// this thread as a worker would receive them.
    pub(super) fn retain(id: usize, form: &Form, snapshot: &DefinitionSnapshot, host: &MacroHost) {
        RETAINED.with_borrow_mut(|retained| {
            if let Some(jobs) = retained {
                jobs.push((id, form.clone(), snapshot.clone(), host.environment()));
            }
        });
    }

    /// A fresh compile-time engine of the build's kind, made on this thread.
    fn engine(
        environment: &MacroEnvironment,
    ) -> Rc<dyn crate::compiler_evaluator::CompilerEvaluator> {
        let options = environment.evaluator_options().clone();
        #[cfg(light_v8)]
        return Rc::new(crate::v8_evaluator::V8Evaluator::new(options).unwrap());
        #[cfg(all(not(light_v8), light_quickjs))]
        return Rc::new(crate::evaluator::Evaluator::new(options).unwrap());
        #[cfg(all(not(any(light_v8, light_quickjs)), feature = "bbp-macros"))]
        return Rc::new(crate::bbp_evaluator::BbpEvaluator::new(options).unwrap());
        #[cfg(not(any(light_v8, light_quickjs, feature = "bbp-macros")))]
        {
            let _ = options;
            panic!("no compile-time engine enabled")
        }
    }

    /// The unit's prepared JavaScript as the main thread's session has it.
    fn main_code(session: &CompilationSession, id: usize) -> String {
        let state = session.shared.lock().units[id].state.clone();
        match state {
            AnalysisState::Prepared(_, _, prepared) => prepared.code().to_owned(),
            AnalysisState::Analyzed(program, _) => crate::emitter::DeferredUnit::prepare(&program)
                .code()
                .to_owned(),
            AnalysisState::Unstarted => {
                let (program, _) = session.analyze(id, None).unwrap();
                crate::emitter::DeferredUnit::prepare(&program)
                    .code()
                    .to_owned()
            }
            _ => panic!("unit {id} did not analyze"),
        }
    }

    #[test]
    fn a_worker_binds_a_sent_environment_and_analyzes_units_as_the_main_thread() {
        RETAINED.set(Some(Vec::new()));
        let mut host = MacroHost::new();
        crate::compile_with_macros(
            "(defn plain [x] (+ x 1))
             (defn expanded [x] (when (pos? x) (-> x inc (* 2))))",
            &mut host,
        )
        .unwrap();
        let jobs = RETAINED.take().unwrap();
        let session = host.session_handle().unwrap();
        let name = |id: usize| session.shared.unit_name(id);
        let mut expected: Vec<_> = jobs
            .iter()
            .map(|(id, ..)| (name(*id), main_code(&session, *id)))
            .collect();
        expected.sort();
        assert!(
            expected
                .iter()
                .any(|(name, code)| name == "cljs.user/plain" && code.contains("rt.core.add")),
            "{expected:?}"
        );
        // The core macros expanded: no call of them is left.
        assert!(
            expected
                .iter()
                .any(|(name, code)| name == "cljs.user/expanded"
                    && !code.contains("when")
                    && code.contains("rt.core.isPositive")
                    && code.contains("rt.core.multiply")),
            "{expected:?}"
        );
        let names: HashMap<usize, String> = jobs.iter().map(|(id, ..)| (*id, name(*id))).collect();
        // The jobs cross to another thread, which binds its own engine.
        let found = std::thread::spawn(move || {
            let mut found: Vec<_> = jobs
                .into_iter()
                .map(|(id, form, snapshot, environment)| {
                    let engine = engine(&environment);
                    let mut host = MacroHost::bind(environment.clone(), Some(engine)).unwrap();
                    let (program, _) =
                        crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, None)
                            .unwrap();
                    let code = crate::emitter::DeferredUnit::prepare(&program)
                        .code()
                        .to_owned();
                    // A unit that expands no macro needs no engine at all.
                    if names[&id] == "cljs.user/plain" {
                        let mut host = MacroHost::bind(environment, None).unwrap();
                        let (program, _) = crate::analyzer::analyze_deferred_form(
                            &form, &mut host, &snapshot, None,
                        )
                        .unwrap();
                        assert_eq!(crate::emitter::DeferredUnit::prepare(&program).code(), code);
                    }
                    (names[&id].clone(), code)
                })
                .collect();
            found.sort();
            found
        })
        .join()
        .unwrap();
        assert_eq!(found, expected);
    }

    /// A catalog view whose every unit initializes to 42.
    struct Answers(Cell<Option<usize>>);
    impl CatalogView for Answers {
        fn compile_unit(&self, _: &dyn MacroEvaluator, id: usize) -> Result<String, String> {
            self.0.set(Some(id));
            Ok("(rt) => 42".into())
        }
        fn unit_source(&self, _: usize) -> Option<u32> {
            None
        }
        fn analysis_sources(&self) -> Vec<u32> {
            Vec::new()
        }
    }

    #[test]
    fn a_bound_engine_reaches_the_catalog_view_its_thread_gives_it() {
        let environment = MacroHost::new().environment();
        std::thread::spawn(move || {
            let evaluator = engine(&environment);
            let host = MacroHost::bind(environment, Some(evaluator.clone())).unwrap();
            let view = Rc::new(Answers(Cell::new(None)));
            let weak = Rc::downgrade(&view) as std::rc::Weak<Answers>;
            install_catalog_services(
                &evaluator,
                weak,
                host.engine.build.clone(),
                host.engine.dependency_log.clone(),
            )
            .unwrap();
            let value = evaluator
                .evaluate_js(
                    "(() => { if ((0, eval)(globalThis.__lightCompileUnit(7))(null) !== 42) throw new Error(\"not the view's unit\"); return null; })()",
                )
                .unwrap();
            evaluator.release(value).unwrap();
            assert_eq!(view.0.get(), Some(7));
            // Without its view, the engine reports the session gone.
            drop(view);
            assert!(
                evaluator
                    .evaluate_js("globalThis.__lightCompileUnit(7)")
                    .unwrap_err()
                    .0
                    .contains("compilation session expired")
            );
        })
        .join()
        .unwrap();
    }

    #[test]
    fn a_worker_runs_the_data_readers_its_environment_was_taken_with() {
        fn number(form: &Form) -> f64 {
            match form.data().kind() {
                DatumKind::Integer(n) => *n as f64,
                DatumKind::Double(n) => *n,
                _ => panic!("expected a number: {form:?}"),
            }
        }
        fn read(host: &mut MacroHost, text: &str) -> Form {
            let source = crate::compiler_data::Source::new("reader.cljs", text);
            host.read_compiler_form(&source).unwrap().unwrap()
        }
        let mut host = MacroHost::new();
        host.register_compiler_default_data_reader("(fn [tag value] (* value 10))")
            .unwrap();
        host.register_compiler_data_reader(Name::new(None, "named"), "(fn [value] (+ value 1))")
            .unwrap();
        // The coordinator's own reader functions are evaluated and cached.
        assert_eq!(number(&read(&mut host, "#named 7")), 8.0);
        assert_eq!(number(&read(&mut host, "#other 7")), 70.0);
        let environment = host.environment();
        // Readers registered after the environment was taken are not its.
        host.register_compiler_data_reader(Name::new(None, "named"), "(fn [value] (- value 1))")
            .unwrap();
        host.register_compiler_default_data_reader("(fn [tag value] 0)")
            .unwrap();
        assert_eq!(number(&read(&mut host, "#named 7")), 6.0);
        std::thread::spawn(move || {
            let mut worker = MacroHost::bind(environment, None).unwrap();
            assert_eq!(number(&read(&mut worker, "#named 7")), 8.0);
            assert_eq!(number(&read(&mut worker, "#other 7")), 70.0);
            // Built-in tags keep their reader before the default one.
            assert!(matches!(
                read(&mut worker, "#inst \"2020-01-01\"").data().kind(),
                DatumKind::CompilerLiteral { .. }
            ));
        })
        .join()
        .unwrap();
        assert_eq!(number(&read(&mut host, "#other 7")), 0.0);
    }

    /// A catalog view that records what `light.build` asked of it.
    struct Sources(RefCell<Vec<usize>>, Cell<bool>);
    impl CatalogView for Sources {
        fn compile_unit(&self, _: &dyn MacroEvaluator, _: usize) -> Result<String, String> {
            Err("no units".into())
        }
        fn unit_source(&self, id: usize) -> Option<u32> {
            self.0.borrow_mut().push(id);
            Some(100 + id as u32)
        }
        fn analysis_sources(&self) -> Vec<u32> {
            self.1.set(true);
            vec![9]
        }
    }

    #[test]
    fn a_bound_engine_asks_its_own_catalog_view_whom_build_requests_come_from() {
        let environment = MacroHost::new().environment();
        std::thread::spawn(move || {
            let evaluator = engine(&environment);
            let host = MacroHost::bind(environment, Some(evaluator.clone())).unwrap();
            let view = Rc::new(Sources(RefCell::new(Vec::new()), Cell::new(false)));
            let weak = Rc::downgrade(&view) as std::rc::Weak<Sources>;
            install_catalog_services(
                &evaluator,
                weak,
                host.engine.build.clone(),
                host.engine.dependency_log.clone(),
            )
            .unwrap();
            // An initializer of units 3 and 5 asks; no compilation serves
            // this engine's builds, so the answer is an error.
            let value = evaluator
                .evaluate_js(
                    r#"(() => { const answer = globalThis.__lightBuildService("i\u00003,5\u0000config\u0000a\u0000b"); if (!answer.startsWith("e")) throw new Error(answer); return null; })()"#,
                )
                .unwrap();
            evaluator.release(value).unwrap();
            assert_eq!(*view.0.borrow(), [3, 5]);
            assert!(view.1.get());
        })
        .join()
        .unwrap();
    }

    /// A coordinator's session whose catalog holds the units `source`
    /// registers, unanalyzed, by name, and the environment a worker binds.
    fn registered(source: &str) -> (CompilationSession, HashMap<String, usize>, MacroEnvironment) {
        registered_with_host(source, MacroHost::new())
    }
    fn registered_with_host(
        source: &str,
        mut host: MacroHost,
    ) -> (CompilationSession, HashMap<String, usize>, MacroEnvironment) {
        RETAINED.set(Some(Vec::new()));
        // Whether or not the compilation's own analyses succeed.
        let _ = crate::compile_with_macros(source, &mut host);
        let jobs = RETAINED.take().unwrap();
        let compiled = host.session_handle().unwrap();
        let session = CompilationSession::default();
        let mut units = HashMap::new();
        for (id, form, snapshot, environment) in jobs {
            let name = compiled.shared.unit_name(id);
            let worker = MacroHost::bind(environment, None).unwrap();
            let unit = session
                .register(
                    name.clone(),
                    Some(form),
                    snapshot,
                    worker,
                    false,
                    name.clone(),
                )
                .unwrap()
                .unwrap();
            units.insert(name, unit);
        }
        (session, units, host.environment())
    }

    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_build_cancellation_reaches_every_shared_runtime_task() {
        let session = CompilationSession::default();
        let runtime = session.shared.native_runtime(&Default::default()).unwrap();
        let first = runtime.evaluator().unwrap();
        let second = runtime.evaluator().unwrap();
        session.cancel_pool();
        for evaluator in [first, second] {
            let error = evaluator.evaluate_unit("rt => 42").unwrap_err();
            assert!(error.0.contains("cancelled"), "{error}");
        }
    }

    #[cfg(feature = "bbp-macros")]
    #[test]
    fn a_task_local_native_failure_retires_its_worker_view_without_cancelling_the_runtime() {
        let mut host = MacroHost::new();
        host.use_native_macros();
        let (session, units, environment) = registered_with_host(
            include_str!("../tests/fixtures/native-helper-demand/pool.cljs"),
            host,
        );
        let shared = session.shared();
        shared.select_native();
        let runtime = shared
            .native_runtime(environment.evaluator_options())
            .unwrap();
        runtime.on_wait(Arc::new(|task, key| {
            if key.kind() == big_boy_pants_js::ResultKind::new(3) {
                let _cancel = task.cancel();
            }
        }));
        let id = units["cljs.user/left"];
        crate::scheduler::run(
            &crate::scheduler::Config::new(crate::scheduler::Slots::ONE, 1),
            |_| WorkerSlot {
                view: None,
                plain: None,
                engines: PoolEngines::Persistent,
                _trace: None,
            },
            |slot, job, worker| {
                execute_pool_job(slot, job, worker, shared, &environment);
                assert!(
                    slot.view.is_none(),
                    "a cancelled VM cannot serve the next analysis"
                );
            },
            |pool| {
                pool.submit_batch(shared.unit_jobs(vec![id], false));
                while matches!(
                    pool.next_outcome().unwrap(),
                    crate::scheduler::Next::Outcome(())
                ) {}
            },
        )
        .unwrap();
        assert!(matches!(
            shared.lock().units[id].state,
            AnalysisState::Failed(..)
        ));
        assert!(
            runtime.evaluator().is_ok(),
            "only the consumer was cancelled"
        );
    }

    /// Hold each claimed unit's analysis until `waiters` threads wait for
    /// a unit.
    #[cfg(feature = "bbp-macros")]
    #[test]
    fn native_pool_helps_parked_consumers_and_migrates_a_saved_continuation() {
        use big_boy_pants_js::{DependencyId, ResultKind, Task, TaskId, ValueView};
        #[derive(Default)]
        struct Progress {
            started: usize,
            backedge_reads: usize,
            tasks: HashMap<TaskId, Task>,
            useful: bool,
            held: Option<(TaskId, usize)>,
            migrated: bool,
        }
        let mut host = MacroHost::new();
        host.use_native_macros();
        let (session, units, environment) = registered_with_host(
            include_str!("../tests/fixtures/native-helper-demand/pool.cljs"),
            host,
        );
        let shared = session.shared();
        shared.select_native();
        let runtime = shared
            .native_runtime(environment.evaluator_options())
            .unwrap();
        let helper = units["cljs.user/helper"];
        let left = units["cljs.user/left"];
        let right = units["cljs.user/right"];
        let other = units["cljs.user/other"];
        let blocked = units["cljs.user/blocked"];
        session.set_macro_free(other);
        let progress = Arc::new((Mutex::new(Progress::default()), Condvar::new()));
        let state = progress.clone();
        runtime.on_wait(Arc::new(move |task, key| {
            if key.kind() == ResultKind::new(3)
                && key.dependency() == DependencyId::new(left as u64)
            {
                state.0.lock().unwrap().backedge_reads += 1;
            }
            if key.kind() == ResultKind::new(3)
                && key.dependency() == DependencyId::new(helper as u64)
            {
                state
                    .0
                    .lock()
                    .unwrap()
                    .tasks
                    .insert(task.id(), task.clone());
                state.1.notify_all();
            }
        }));
        let state = progress.clone();
        *shared.on_claim.lock().unwrap() = Some(Arc::new(move |unit| {
            if unit == blocked {
                assert_eq!(
                    HELP_DEPTH.get(),
                    0,
                    "C may wait on outer A and must not be nested below it"
                );
            }
            if unit == left || unit == right {
                let mut p = state.0.lock().unwrap();
                p.started += 1;
                state.1.notify_all();
                while p.started != 2 {
                    let (next, timeout) = state
                        .1
                        .wait_timeout(p, std::time::Duration::from_secs(5))
                        .unwrap();
                    p = next;
                    assert!(!timeout.timed_out(), "two compiler workers did not start");
                }
            }
            if unit == other {
                let mut p = state.0.lock().unwrap();
                assert_eq!(p.tasks.len(), 2, "useful work runs while both macros wait");
                p.useful = true;
                state.1.notify_all();
            }
            if unit == helper {
                let mut p = state.0.lock().unwrap();
                while p.tasks.len() != 2 || !p.useful {
                    let (next, timeout) = state
                        .1
                        .wait_timeout(p, std::time::Duration::from_secs(5))
                        .unwrap();
                    p = next;
                    assert!(
                        !timeout.timed_out(),
                        "waiting consumers did not help their pool"
                    );
                }
            }
        }));
        let state = progress.clone();
        *shared.on_native_submit.lock().unwrap() = Some(Arc::new(move |task, owner, queued| {
            let mut p = state.0.lock().unwrap();
            if !p.tasks.contains_key(&task) || p.migrated {
                return;
            }
            if !queued {
                p.held.get_or_insert((task, owner));
                return;
            }
            if p.held != Some((task, owner)) {
                return;
            }
            // This worker cannot execute its own saved continuation. Its peer,
            // still helping from the other macro's wait, must claim the step.
            while !p.migrated {
                let (next, timeout) = state
                    .1
                    .wait_timeout(p, std::time::Duration::from_secs(5))
                    .unwrap();
                p = next;
                assert!(!timeout.timed_out(), "a peer did not resume the saved task");
            }
        }));
        let state = progress.clone();
        *shared.on_native_run.lock().unwrap() = Some(Arc::new(move |task, worker| {
            let mut p = state.0.lock().unwrap();
            if let Some((held, owner)) = p.held
                && task == held
                && !p.migrated
            {
                assert_ne!(worker, owner);
                p.migrated = true;
                state.1.notify_all();
            }
        }));
        let mut config = crate::scheduler::Config::new(crate::scheduler::Slots::new(3).unwrap(), 1);
        config.stack_size = Some(8 << 20);
        config.limits.batch_jobs = 1;
        let counts = crate::scheduler::run(
            &config,
            |slot| WorkerSlot {
                view: None,
                plain: None,
                engines: PoolEngines::Persistent,
                _trace: Some(crate::compile_trace::record_worker(slot)),
            },
            |slot, job, worker| execute_pool_job(slot, job, worker, shared, &environment),
            |pool| {
                pool.submit_batch(shared.unit_jobs(vec![left, right, blocked, other], false));
                loop {
                    match pool.next_outcome().unwrap() {
                        crate::scheduler::Next::Outcome(()) => {}
                        crate::scheduler::Next::Quiescent => break,
                        other => panic!("unexpected {other:?}"),
                    }
                }
                pool.counts()
            },
        )
        .unwrap();
        assert!(counts.helped > 0);
        let p = progress.0.lock().unwrap();
        assert!(p.useful && p.migrated);
        assert_eq!(p.tasks.len(), 2);
        assert_eq!(p.backedge_reads, 1, "C really demands A after A can resume");
        for task in p.tasks.values() {
            for name in ["beforeDemand", "afterDemand"] {
                assert_eq!(
                    task.global_value(name).unwrap().unwrap().view(),
                    ValueView::Number(1.0)
                );
            }
        }
        let counters = runtime.helper_counts();
        assert_eq!(
            counters.compiled, 2,
            "one helper plus C's later demand for A"
        );
        assert!(counters.migrations > 0);
        let trace = crate::compile_trace::record();
        for id in [left, right, blocked, other] {
            assert!(matches!(
                shared.lock().units[id].state,
                AnalysisState::Analyzed(..)
            ));
            if let Some(rows) = shared.lock().units[id]
                .charges
                .as_mut()
                .and_then(|charges| charges.trace.take())
            {
                crate::compile_trace::replay(*rows, 0);
            }
        }
        let trace = trace.finish().unwrap();
        assert!(
            trace
                .units
                .iter()
                .any(|row| row.id == other as u32 && row.depth > 0)
        );
    }

    fn hold_claims(shared: &Arc<SharedCatalog>, waiters: usize) {
        let catalog = Arc::downgrade(shared);
        *shared.on_claim.lock().unwrap() = Some(Arc::new(move |_| {
            let catalog = catalog.upgrade().unwrap();
            while catalog.lock().waiting < waiters {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }));
    }

    /// Analyze unit `id` on `threads` workers at once, each with a view of
    /// its own and no engine.
    fn demand_together(
        shared: &Arc<SharedCatalog>,
        environment: &MacroEnvironment,
        id: usize,
        threads: usize,
    ) -> Vec<Result<Arc<Program>, String>> {
        std::thread::scope(|scope| {
            let workers = (0..threads)
                .map(|_| {
                    scope.spawn(|| {
                        let host = MacroHost::bind(environment.clone(), None).unwrap();
                        let view = WorkerView::new(shared.clone(), host).unwrap();
                        view.analyze(id, None).map(|(program, _)| program)
                    })
                })
                .collect::<Vec<_>>();
            workers.into_iter().map(|w| w.join().unwrap()).collect()
        })
    }

    #[test]
    fn two_threads_demanding_one_unit_share_its_one_analysis() {
        let (session, units, environment) = registered("(defn plain [x] (+ x 1))");
        let id = units["cljs.user/plain"];
        let shared = session.shared().clone();
        hold_claims(&shared, 1);
        let results = demand_together(&shared, &environment, id, 2);
        let [Ok(first), Ok(second)] = &results[..] else {
            panic!("{results:?}");
        };
        // One thread analyzed it while the other waited for that analysis.
        assert!(Arc::ptr_eq(first, second));
        assert_eq!(shared.lock().units[id].analyses, 1);
        assert_eq!(shared.lock().waiting, 0);
        // The coordinator reuses it too.
        let (again, _) = session.analyze(id, None).unwrap();
        assert!(Arc::ptr_eq(first, &again));
        assert_eq!(shared.lock().units[id].analyses, 1);
        let code = crate::emitter::DeferredUnit::prepare(first)
            .code()
            .to_owned();
        assert!(code.contains("rt.core.add"), "{code}");
    }

    #[test]
    fn every_thread_waiting_for_a_failed_unit_sees_its_one_failure() {
        let (session, units, environment) = registered("(defn bad [] (recur 1))");
        let id = units["cljs.user/bad"];
        let shared = session.shared().clone();
        hold_claims(&shared, 2);
        let results = demand_together(&shared, &environment, id, 3);
        let errors = results
            .into_iter()
            .map(|result| result.unwrap_err())
            .collect::<Vec<_>>();
        assert!(
            errors[0].contains("cljs.user/bad") && errors[0].contains("recur argument count"),
            "{errors:?}"
        );
        assert!(errors.iter().all(|error| *error == errors[0]), "{errors:?}");
        assert_eq!(shared.lock().units[id].analyses, 1);
        // A later demand is not a retry.
        assert_eq!(session.analyze(id, None).unwrap_err(), errors[0]);
        assert!(session.diagnostic(id).is_some());
        assert_eq!(shared.lock().units[id].analyses, 1);
    }

    #[test]
    fn a_traced_thread_blocked_in_the_catalog_records_a_wait_not_work() {
        use crate::compile_trace::{Stage, enter, record};
        let (session, units, _) = registered("(defn a [] 1)");
        let a = units["cljs.user/a"];
        let shared = session.shared().clone();
        let (first, second) = (shared.join(), shared.join());
        assert!(matches!(
            shared.claim(second, a, false, None),
            Ok(Claim::Analyze(..))
        ));
        let blocked = std::time::Duration::from_millis(30);
        let trace = std::thread::scope(|scope| {
            let waiter = scope.spawn(|| {
                let recording = record();
                {
                    let _drain = enter(Stage::Drain, 0, None);
                    assert!(shared.claim(first, a, false, None).is_err());
                }
                recording.finish().unwrap()
            });
            while shared.lock().waiting == 0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            std::thread::sleep(blocked);
            shared.settle(second, a, Some(AnalysisState::Failed("x".into(), None)), 0);
            waiter.join().unwrap()
        });
        let wait = trace.stage(Stage::Wait).time;
        assert!(wait >= blocked, "{wait:?}");
        // The drain's own time is what it did around the wait.
        assert!(trace.namespaces[0].drain < blocked, "{trace:?}");
        assert_eq!(trace.exclusive_total(), trace.wall);
    }

    #[test]
    fn a_wait_that_would_close_a_cycle_across_threads_reports_it_from_the_lowest_unit() {
        let (session, units, _) = registered("(defn a [] 1) (defn b [] 2) (defn c [] 3)");
        let (a, c) = (units["cljs.user/a"], units["cljs.user/c"]);
        assert!(a < c);
        let shared = session.shared().clone();
        let (first, second) = (shared.join(), shared.join());
        assert!(matches!(
            shared.claim(first, c, false, None),
            Ok(Claim::Analyze(..))
        ));
        assert!(matches!(
            shared.claim(second, a, false, None),
            Ok(Claim::Analyze(..))
        ));
        let error = std::thread::scope(|scope| {
            // `c`'s analysis demands `a`, which the second thread analyzes.
            let waiter = scope.spawn(|| shared.claim(first, a, false, None).map(drop));
            while shared.lock().waiting == 0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            // `a`'s analysis demands `c`: that wait would close the cycle,
            // which reads from `a`, the lower unit, whichever thread asks.
            let error = shared.claim(second, c, false, None).map(drop).unwrap_err();
            assert_eq!(
                error,
                "compile dependency cycle: cljs.user/a -> cljs.user/c -> cljs.user/a"
            );
            // The error fails `a`, and the waiting thread shares the failure.
            shared.settle(
                second,
                a,
                Some(AnalysisState::Failed(error.clone(), None)),
                0,
            );
            assert_eq!(waiter.join().unwrap().unwrap_err(), error);
            error
        });
        shared.settle(first, c, Some(AnalysisState::Failed(error, None)), 0);
        let c = shared.lock();
        assert!(
            c.threads
                .iter()
                .all(|t| t.stack.is_empty() && t.waiting.is_none())
        );
        assert_eq!(c.waiting, 0);
    }

    #[test]
    fn a_worker_engine_installs_the_registrations_and_evaluates_what_the_catalog_prepared() {
        let mut host = MacroHost::new();
        crate::compile_with_macros("(defn f [x] (+ x 1)) (defmacro m [] (f 41)) (m)", &mut host)
            .unwrap();
        let session = host.session_handle().unwrap();
        let shared = session.shared().clone();
        let id = (0..session.next_unit())
            .find(|&id| shared.unit_name(id) == "cljs.user/f")
            .unwrap();
        assert!(shared.prepared(id).unwrap().is_some());
        let analyses = shared.lock().units[id].analyses;
        let environment = host.environment();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let evaluator = engine(&environment);
                    let host = MacroHost::bind(environment.clone(), Some(evaluator.clone())).unwrap();
                    let view = WorkerView::new(shared.clone(), host).unwrap();
                    // The unit's initializer runs in this engine, where `f`'s
                    // root was registered from the catalog.
                    let value = evaluator
                        .evaluate_js(&format!(
                            "(() => {{ const f = (0, eval)(globalThis.__lightCompileUnit({id}))(globalThis.__lightRuntimeForCompiler); if (f(41) !== 42) throw new Error(String(f)); return null; }})()"
                        ))
                        .unwrap();
                    evaluator.release(value).unwrap();
                    assert_eq!(view.installed.get(), shared.lock().registrations.len());
                })
                .join()
                .unwrap();
        });
        // Evaluated again in another engine, not analyzed again.
        assert_eq!(shared.lock().units[id].analyses, analyses);
    }

    /// A native producer imports catalog data without evaluating registration
    /// source, however often its helper asks the view to install. The worker's
    /// own cursor stays put.
    #[cfg(feature = "bbp-macros")]
    #[test]
    fn a_native_producer_takes_each_catalog_registration_once() {
        let mut host = MacroHost::new();
        host.use_native_macros();
        crate::compile_with_macros("(defn f [x] (+ x 1)) (defmacro m [] (f 41)) (m)", &mut host)
            .unwrap();
        let session = host.session_handle().unwrap();
        let shared = session.shared().clone();
        let total = shared.lock().registrations.len();
        assert!(total > 0);
        let environment = host.environment();
        std::thread::scope(|scope| {
            scope
                .spawn(|| {
                    let runtime = shared
                        .native_runtime(environment.evaluator_options())
                        .unwrap();
                    let worker: Rc<dyn crate::compiler_evaluator::CompilerEvaluator> =
                        Rc::new(runtime.evaluator().unwrap());
                    let view = WorkerView::new(
                        shared.clone(),
                        MacroHost::bind(environment.clone(), Some(worker)).unwrap(),
                    )
                    .unwrap();
                    let producer = runtime.evaluator().unwrap();
                    let held = || producer.registration_count();
                    let before = held();
                    view.install(Some(&producer)).unwrap();
                    assert_eq!(held(), before, "catalog source was retained for replay");
                    assert_eq!(producer.catalog_cursor(), Some(total));
                    view.install(Some(&producer)).unwrap();
                    assert_eq!(held(), before, "catalog source was retained for replay");
                    assert_eq!(
                        view.installed.get(),
                        0,
                        "the worker's engine still lacks them"
                    );
                })
                .join()
                .unwrap();
        });
    }

    #[test]
    fn roots_initialize_once_per_isolate() {
        // `counted`'s initializer counts its runs in its engine's globals.
        let mut host = MacroHost::new();
        crate::compile_with_macros(
            "(def counted (set! (.-countedRuns js/globalThis) (+ 1 (or (.-countedRuns js/globalThis) 0))))
             (defmacro m [] counted)
             (m)",
            &mut host,
        )
        .unwrap();
        let session = host.session_handle().unwrap();
        let shared = session.shared().clone();
        let id = (0..session.next_unit())
            .find(|&id| shared.unit_name(id) == "cljs.user/counted")
            .unwrap();
        assert!(shared.prepared(id).unwrap().is_some());
        assert_eq!(shared.lock().units[id].analyses, 1);
        // Read the registry root twice: both reads see the one run of its
        // initializer in this engine.
        let force_twice = |evaluator: &dyn crate::compiler_evaluator::CompilerEvaluator| {
            let value = evaluator
                .evaluate_js(
                    r#"(() => { const roots = globalThis.__lightCompilerRoots; const seen = [roots["cljs.user/counted"], roots["cljs.user/counted"], globalThis.countedRuns]; if (seen.some(n => n !== 1)) throw new Error("reads and runs: " + seen); return null; })()"#,
                )
                .unwrap();
            evaluator.release(value).unwrap();
        };
        // The coordinator's isolate ran it once, for the macro.
        let main = host.engine.bootstrap.as_ref().unwrap().evaluator.clone();
        force_twice(main.as_ref());
        let environment = host.environment();
        // Two workers, each with an isolate of its own, read it at once.
        let both = std::sync::Barrier::new(2);
        std::thread::scope(|scope| {
            let workers = (0..2)
                .map(|_| {
                    scope.spawn(|| {
                        let evaluator = engine(&environment);
                        let host =
                            MacroHost::bind(environment.clone(), Some(evaluator.clone())).unwrap();
                        let view = WorkerView::new(shared.clone(), host).unwrap();
                        // What a worker installs before its engine runs code.
                        view.install(None).unwrap();
                        both.wait();
                        force_twice(evaluator.as_ref());
                        // After the other isolate initialized it too, a read
                        // here still runs nothing again.
                        both.wait();
                        force_twice(evaluator.as_ref());
                    })
                })
                .collect::<Vec<_>>();
            for worker in workers {
                worker.join().unwrap();
            }
        });
        // Each isolate initialized it from the one shared analysis, and the
        // coordinator's run stays its only one.
        assert_eq!(shared.lock().units[id].analyses, 1);
        force_twice(main.as_ref());
    }

    #[test]
    fn a_waiter_gets_the_analysis_after_the_application_took_the_program() {
        waiter_after_application_transfer(false);
    }

    #[test]
    fn a_waiter_gets_the_analysis_after_a_worker_claimed_first() {
        waiter_after_application_transfer(true);
    }

    fn waiter_after_application_transfer(worker_first: bool) {
        use std::sync::{Barrier, atomic::AtomicBool, atomic::Ordering::Relaxed, mpsc};
        use std::time::Duration;

        let source = (0..3)
            .map(|i| format!("(defn u{i} [x] (+ x {i}))"))
            .collect::<String>();
        let (session, units, environment) = registered(&source);
        let shared = session.shared().clone();
        let claimed = Arc::new(Barrier::new(2));
        let (waiting, registrations) = mpsc::channel();
        let registrations = Mutex::new(registrations);
        *shared.wait_registered.lock().unwrap() = Some(waiting);
        let start = claimed.clone();
        *shared.on_claim.lock().unwrap() = Some(Arc::new(move |id| {
            start.wait();
            // A worker producer has both the application coordinator and
            // another worker waiting; a coordinator producer has one waiter.
            for _ in 0..if worker_first { 2 } else { 1 } {
                assert_eq!(
                    registrations
                        .lock()
                        .unwrap()
                        .recv_timeout(Duration::from_secs(5))
                        .unwrap(),
                    id
                );
            }
        }));
        let transferred = Arc::new((Mutex::new(None), Condvar::new()));
        let released = transferred.clone();
        let hook_timed_out = Arc::new(AtomicBool::new(false));
        let timed_out = hook_timed_out.clone();
        let coordinator = std::thread::current().id();
        let catalog = Arc::downgrade(&shared);
        *shared.on_wake.lock().unwrap() = Some(Arc::new(move |id| {
            let catalog = catalog.upgrade().unwrap();
            // Reenter the hook registry: another callback may briefly be
            // cloning it, but this invocation must not hold its mutex.
            drop(catalog.on_wake.lock().unwrap());
            // When a worker produces the analysis the coordinator also
            // waits. It must return so it can transfer the application IR.
            if std::thread::current().id() == coordinator {
                return;
            }
            let (done, _) = released
                .1
                .wait_timeout_while(released.0.lock().unwrap(), Duration::from_secs(5), |done| {
                    *done != Some(id)
                })
                .unwrap();
            if *done != Some(id) {
                timed_out.store(true, Relaxed);
            }
        }));
        for i in 0..3 {
            let id = units[&format!("cljs.user/u{i}")];
            let waited = std::thread::scope(|scope| {
                let producer = worker_first.then(|| {
                    scope.spawn(|| {
                        let host = MacroHost::bind(environment.clone(), None).unwrap();
                        let view = WorkerView::new(shared.clone(), host).unwrap();
                        view.analyze(id, None).map(drop).unwrap();
                    })
                });
                if worker_first {
                    claimed.wait();
                }
                let waiter = scope.spawn(|| {
                    if !worker_first {
                        claimed.wait();
                    }
                    let host = MacroHost::bind(environment.clone(), None).unwrap();
                    let view = WorkerView::new(shared.clone(), host).unwrap();
                    view.analyze(id, None).map(|(program, _)| program)
                });
                // Both producer orders leave a worker waiting while the
                // application takes the program and drops its own copy.
                let (program, _) = session.analyze(id, None).unwrap();
                let code = crate::emitter::DeferredUnit::prepare(&program)
                    .code()
                    .to_owned();
                session.release_application_ir(id);
                drop(program);
                if let Some(producer) = producer {
                    producer.join().unwrap();
                }
                {
                    let c = shared.lock();
                    let unit = &c.units[id];
                    assert!(matches!(unit.state, AnalysisState::Prepared(..)));
                    assert_eq!(unit.waiters, 1);
                    assert_eq!(Arc::strong_count(unit.retained.as_ref().unwrap()), 1);
                }
                *transferred.0.lock().unwrap() = Some(id);
                transferred.1.notify_all();
                (waiter.join().unwrap(), code)
            });
            assert!(
                !hook_timed_out.load(Relaxed),
                "wake hook blocked the application transfer"
            );
            let (Ok(program), code) = waited else {
                panic!("u{i}: {:?}", waited.0);
            };
            assert_eq!(crate::emitter::DeferredUnit::prepare(&program).code(), code);
            let c = shared.lock();
            assert_eq!(c.units[id].analyses, 1);
            assert!(c.units[id].retained.is_none() && c.units[id].waiters == 0);
        }
    }

    #[test]
    fn a_producer_that_unwinds_wakes_every_waiter_with_its_interruption() {
        let (session, units, environment) = registered("(defn plain [x] (+ x 1))");
        let id = units["cljs.user/plain"];
        let shared = session.shared().clone();
        let catalog = Arc::downgrade(&shared);
        *shared.on_claim.lock().unwrap() = Some(Arc::new(move |_| {
            let catalog = catalog.upgrade().unwrap();
            while catalog.lock().waiting < 2 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            panic!("the producer unwinds");
        }));
        let results = std::thread::scope(|scope| {
            let workers = (0..3)
                .map(|_| {
                    scope.spawn(|| {
                        let host = MacroHost::bind(environment.clone(), None).unwrap();
                        let view = WorkerView::new(shared.clone(), host).unwrap();
                        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            view.analyze(id, None).map(drop)
                        }))
                    })
                })
                .collect::<Vec<_>>();
            workers
                .into_iter()
                .map(|w| w.join().unwrap())
                .collect::<Vec<_>>()
        });
        assert_eq!(results.iter().filter(|r| r.is_err()).count(), 1);
        let errors = results
            .into_iter()
            .filter_map(Result::ok)
            .map(Result::unwrap_err)
            .collect::<Vec<_>>();
        assert_eq!(errors, ["analysis interrupted", "analysis interrupted"]);
        assert_eq!(
            session.analyze(id, None).unwrap_err(),
            "analysis interrupted"
        );
        let c = shared.lock();
        assert_eq!(c.units[id].analyses, 1);
        assert_eq!(c.waiting, 0);
        assert!(
            c.threads
                .iter()
                .all(|t| t.stack.is_empty() && t.waiting.is_none())
        );
    }
}
