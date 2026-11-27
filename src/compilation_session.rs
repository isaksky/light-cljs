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
    /// The rule that places its analysis with a pool, which its
    /// namespace's artifact records with what it adds to the registry.
    dispatch: crate::namespace_artifact::UnitDispatch,
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
    /// Maximum nested unit demand depth recorded during a top-level overlay pass.
    unit_depth: usize,
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
    /// What the unit added to its worker's compile-time registry while
    /// artifacts are captured, which the coordinator adds to its own where
    /// it charges the unit, as a serial build adds it (light-cljs-1004).
    namespaces: Vec<crate::namespace_artifact::RegistryEntry>,
    /// Shared catalog unit IDs emitted on demand during this recorded analysis,
    /// applied to `Unit::emitted` when charged in canonical order.
    emitted: Vec<usize>,
    /// What a worker's `--stats-file` trace recorded of the unit and the
    /// units it demanded, which the coordinator records where it charges
    /// the unit (light-cljs-369, [`crate::compile_trace::replay`]).
    trace: Option<Box<crate::compile_trace::WorkerRows>>,
}
impl UnitCharges {
    pub(crate) fn namespace(
        recorded: crate::macro_host::RecordedUnit,
        host: &MacroHost,
        name: &str,
        gensyms: Vec<Vec<u16>>,
        namespaces: Vec<crate::namespace_artifact::RegistryEntry>,
        trace: Option<Box<crate::compile_trace::WorkerRows>>,
    ) -> Self {
        Self {
            events: recorded.events,
            macro_depth: host.engine.work.macro_depth.get(),
            analysis_depth: host.engine.work.analysis_depth.get(),
            unit_depth: host.engine.work.unit_depth.get(),
            limits: (host.limits.expansions, host.limits.generated_nodes),
            depth_limit: host.limits.expansion_depth.min(64),
            location: name.to_owned(),
            observations: recorded.observations,
            gensyms,
            namespaces,
            emitted: recorded.emitted,
            trace,
        }
    }
    pub(crate) fn absent_namespaces(&self) -> impl Iterator<Item = &str> + '_ {
        self.observations
            .iter()
            .flat_map(|log| log.absent.iter().map(|(_, ns)| ns.as_str()))
    }
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
        depth: Option<usize>,
        mut control: Option<crate::evaluator::ExecutionControl>,
        start: &dyn Fn() -> Result<Option<crate::evaluator::ExecutionControl>, String>,
        session: &CompilationSession,
    ) -> Result<(), (String, crate::AnalysisError)> {
        use crate::macro_host::UnitEvent;
        use crate::macro_host::WorkMarks;
        if let Some(depth) = depth {
            WorkMarks::note(&binding.work.unit_depth, depth + 1);
        }
        if self.unit_depth > 0 {
            WorkMarks::note(&binding.work.unit_depth, self.unit_depth);
        }
        WorkMarks::note(&binding.work.macro_depth, self.macro_depth);
        WorkMarks::note(&binding.work.analysis_depth, self.analysis_depth);
        let nodes = &binding.generated_nodes;
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
                    if let Err(message) =
                        binding.charge_expansion(expansion.exempt, expansion.nodes, self.limits)
                    {
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
                    exempt,
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
                    if let Err(message) = binding.charge_expansion(*exempt, *generated, self.limits)
                    {
                        return fail(message);
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
    /// Analyzed IR and warnings, plus optional worker-prepared compile-time JS
    /// so coordinator `release_application_ir` avoids serial JS emission.
    Analyzed(
        Arc<Program>,
        Vec<AnalysisWarning>,
        Option<Arc<crate::emitter::DeferredUnit>>,
    ),
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
#[derive(Clone)]
enum RegistrationName {
    Var(String),
    Declaration,
    Test(String),
}
impl RegistrationName {
    fn resolve(self, unit: usize) -> String {
        match self {
            Self::Var(name) => name,
            Self::Declaration => format!("declaration#{unit}"),
            Self::Test(owner) => format!("{owner} :test#{unit}"),
        }
    }
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
    #[cfg(all(test, light_v8))]
    fn source(&self) -> String {
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
    bindings: HashMap<String, (VarId, usize)>,
    journal: RegistrationJournal,
    /// Compiler-owned index of registered roots, bundles, :test units and
    /// protocol extensions so V8 isolates resolve on demand instead
    /// of replaying the full registration journal (light-cljs-1090).
    roots: RootIndex,
    /// How many direct root reads [`SharedCatalog::resolve_root`] answered.
    #[cfg(all(test, light_v8))]
    root_resolutions: usize,
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

type JournalMap<T> = HashMap<String, T>;

/// An append-only visibility history, separate from shared unit allocation.
/// Its derived indexes must describe exactly the registrations an engine sees.
#[derive(Clone, Default)]
pub(crate) struct RegistrationJournal {
    // Frozen namespace prefixes share registration payloads. Each writer
    // appends to its own tail without copying the history it inherited.
    registrations: crate::arena::Arena<Registration>,
    active: JournalMap<bool>,
    // Only active defmulti products have stable named demand keys.
    multifn_targets: JournalMap<String>,
    // Built once when a namespace prefix is first requested, then advanced
    // from the same registration history that a fresh writer used to scan.
    bundle_count: usize,
    environment_bundles: usize,
}
impl RegistrationJournal {
    fn push(&mut self, registration: Registration) -> usize {
        match &registration {
            Registration::Root(name, unit, _, once, _) => {
                if !(unit.is_none() && self.active.contains_key(name)
                    || *once && self.active.get(name) == Some(&true))
                {
                    self.multifn_targets.remove(name);
                    self.active.insert(name.clone(), unit.is_some());
                }
            }
            Registration::Bundle(bundle) => {
                self.bundle_count += 1;
                self.environment_bundles +=
                    usize::from(reaches_environment_data(&bundle.extensions));
                for name in &bundle.products {
                    if bundle.once && self.active.get(name) == Some(&true) {
                        continue;
                    }
                    if bundle.once {
                        self.multifn_targets.insert(name.clone(), name.clone());
                    } else {
                        self.multifn_targets.remove(name);
                    }
                    self.active.insert(name.clone(), true);
                }
            }
        }
        self.registrations.push(registration);
        self.registrations.len()
    }
}
/// What the JS registry (`src/compiler_roots.js`) holds for a name from one
/// journal position on, as far as direct root resolution needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RootSlot {
    /// A root with its unit, or a declaration without one. `over_group`: a
    /// `defonce` that replaced a group bundle's slot, which the registry
    /// reads first while that bundle has the product (`oncePrevious`).
    Root {
        unit: Option<usize>,
        over_group: bool,
    },
    /// A declaration bundle's product.
    Bundle { group: bool },
}
impl RootSlot {
    /// Whether the registry's slot has a unit (`slot.unit !== null`): a
    /// bundle's slot does, with unit -1.
    fn defined(self) -> bool {
        !matches!(self, Self::Root { unit: None, .. })
    }
}
/// Compiler-owned index of compile-time roots, bundles, `:test` units and
/// protocol extensions, updated where each registration is recorded with the
/// exact rules of `__lightRegisterUnit` and `__lightRegisterBundle` in
/// `src/compiler_roots.js`.
///
/// Serves both direct name resolution ([`Self::resolve`]) and lazy on-demand
/// materialization for V8 engines (`__lightRootService`), eliminating
/// eager catalog replay on both the coordinator and worker isolates.
#[derive(Default)]
pub(crate) struct RootIndex {
    /// Per qualified name, the effective slot history by journal position.
    slots: HashMap<String, Vec<(usize, RootSlot)>>,
    /// Per qualified name, every journal position touching `name` in ascending
    /// order (`Registration::Root(name, ...)` or a `Registration::Bundle`
    /// naming `name` in `products`).
    name_positions: HashMap<String, Vec<usize>>,
    /// For each `unit` registered via `Registration::Root(_, Some(unit), ...)`,
    /// its journal position (so `__lightCompilerTest(unit)` resolves in O(1)).
    unit_positions: HashMap<usize, usize>,
    /// 0-based index `(order - 1)` -> `(journal_position, Arc<BundleRecord>)`.
    bundles: Vec<(usize, Arc<BundleRecord>)>,
    /// `bundles.len()` strictly before each journal position `pos`
    /// (`declarationFrontier` for a root registered at `pos`).
    bundle_count_at: Vec<usize>,
    /// Per protocol name in `bundle.extensions`, 1-based bundle orders that
    /// extend it, in ascending order.
    protocol_bundles: HashMap<String, Vec<usize>>,
    /// Append-only log of `(journal_pos, protocol)` the first time each
    /// protocol is extended by any bundle.
    extended_protocols: Vec<(usize, String)>,
}
/// Summary of what a single [`RootIndex::record`] call changed, so the
/// coordinator knows whether its attached engine needs an immediate cursor
/// notification.
#[derive(Clone, Copy, Debug, Default)]
struct RecordEffect {
    /// True if any touched name already had an earlier registration.
    redefined: bool,
    /// True if a bundle carried protocol/multimethod extensions.
    has_extensions: bool,
    /// New journal length, bundle count, and distinct extended protocol count.
    journal_len: usize,
    bundle_count: usize,
    ext_protocol_count: usize,
}
impl RootIndex {
    fn current(&self, name: &str) -> Option<RootSlot> {
        self.slots.get(name)?.last().map(|&(_, slot)| slot)
    }
    fn set(&mut self, name: &str, position: usize, slot: RootSlot) {
        let history = match self.slots.get_mut(name) {
            Some(history) => history,
            None => self.slots.entry(name.to_owned()).or_default(),
        };
        match history.last_mut() {
            // A bundle naming a product twice sets it once.
            Some(last) if last.0 == position => last.1 = slot,
            _ => history.push((position, slot)),
        }
    }
    fn note_position(&mut self, name: &str, position: usize) -> bool {
        let positions = match self.name_positions.get_mut(name) {
            Some(positions) => positions,
            None => self.name_positions.entry(name.to_owned()).or_default(),
        };
        let existed = !positions.is_empty();
        if positions.last() != Some(&position) {
            positions.push(position);
        }
        existed
    }
    /// Note the registration recorded at journal `position`.
    fn record(&mut self, position: usize, registration: &Registration) -> RecordEffect {
        let bundles_before = self.bundles.len();
        debug_assert_eq!(self.bundle_count_at.len(), position);
        self.bundle_count_at.push(bundles_before);
        let mut redefined = false;
        let mut has_extensions = false;
        match registration {
            Registration::Root(name, unit, _, once, _) => {
                redefined |= self.note_position(name, position);
                if let Some(id) = *unit {
                    self.unit_positions.insert(id, position);
                }
                let old = self.current(name);
                // A declaration without a unit never replaces a slot, and a
                // `defonce` keeps one with a unit unless a group bundle's.
                if !(unit.is_none() && old.is_some()
                    || *once
                        && old.is_some_and(|old| {
                            old.defined() && old != RootSlot::Bundle { group: true }
                        }))
                {
                    let over_group = *once && old == Some(RootSlot::Bundle { group: true });
                    let unit = *unit;
                    self.set(name, position, RootSlot::Root { unit, over_group });
                }
            }
            Registration::Bundle(bundle) => {
                let order = self.bundles.len() + 1;
                self.bundles.push((position, bundle.clone()));
                for name in &bundle.products {
                    redefined |= self.note_position(name, position);
                }
                if !bundle.extensions.is_empty() {
                    has_extensions = true;
                    for (protocol, _, _) in &bundle.extensions {
                        let orders = match self.protocol_bundles.get_mut(protocol) {
                            Some(orders) => orders,
                            None => self.protocol_bundles.entry(protocol.clone()).or_default(),
                        };
                        if orders.is_empty() {
                            self.extended_protocols.push((position, protocol.clone()));
                        }
                        if orders.last() != Some(&order) {
                            orders.push(order);
                        }
                    }
                }
                // A `defonce`-like bundle whose every product is defined
                // changes nothing.
                let skipped = bundle.once
                    && !bundle.products.is_empty()
                    && bundle
                        .products
                        .iter()
                        .all(|name| self.current(name).is_some_and(RootSlot::defined));
                if !skipped {
                    let group = bundle.group;
                    for name in &bundle.products {
                        self.set(name, position, RootSlot::Bundle { group });
                    }
                }
            }
        }
        RecordEffect {
            redefined,
            has_extensions,
            journal_len: position + 1,
            bundle_count: self.bundles.len(),
            ext_protocol_count: self.extended_protocols.len(),
        }
    }
    /// Whether registrations in `limit..installed` do not redefine any root or
    /// bundle product already registered `< limit`. Protocol extensions in
    /// `limit..installed` are isolated dynamically by `compiler_roots.js`'s
    /// `overlayMode` frontier window (`overlayPrefixBundles`), so they do not
    /// block overlay reuse.
    fn is_isolated_suffix(
        &self,
        limit: usize,
        installed: usize,
        registrations: &crate::arena::Arena<Registration>,
    ) -> bool {
        for reg in registrations.range(limit..installed) {
            match reg {
                Registration::Root(name, ..) => {
                    if self
                        .name_positions
                        .get(name)
                        .is_some_and(|positions| positions.first().is_some_and(|&p| p < limit))
                    {
                        return false;
                    }
                }
                Registration::Bundle(bundle) => {
                    if bundle.products.iter().any(|name| {
                        self.name_positions
                            .get(name)
                            .is_some_and(|positions| positions.first().is_some_and(|&p| p < limit))
                    }) {
                        return false;
                    }
                }
            }
        }
        true
    }
    /// The unit of the root `name` names to the journal's registrations
    /// before `position`, matching `__lightRegisterUnit` / `__lightRegisterBundle`.
    #[cfg_attr(not(all(test, light_v8)), allow(dead_code))]
    pub(crate) fn resolve(&self, name: &str, position: usize) -> Result<usize, String> {
        let slot = self.slots.get(name).and_then(|history| {
            let visible = history.partition_point(|&(at, _)| at < position);
            visible.checked_sub(1).map(|last| history[last].1)
        });
        match slot {
            None => Err(format!("compiler root is not registered: {name}")),
            Some(RootSlot::Root { unit: None, .. }) => Err(format!(
                "compiler root is not registered: {name} (declared without a definition)"
            )),
            Some(RootSlot::Root {
                unit: Some(unit),
                over_group: false,
            }) => Ok(unit),
            Some(_) => Err(format!(
                "{name} is a declaration-bundle product; bundle products and protocol extensions require the JS bundle registry"
            )),
        }
    }
    /// Distinct protocol names whose first extension arrived at or before
    /// `visible` registrations, starting from `synced` already-delivered
    /// entries in `extended_protocols`. Returns `(new_synced_count, names)`.
    fn new_extended_protocols(&self, synced: usize, visible: usize) -> (usize, Vec<String>) {
        if synced >= self.extended_protocols.len() {
            return (synced, Vec::new());
        }
        let end = self
            .extended_protocols
            .partition_point(|(pos, _)| *pos < visible);
        if end <= synced {
            return (synced, Vec::new());
        }
        let names = self.extended_protocols[synced..end]
            .iter()
            .map(|(_, protocol)| protocol.clone())
            .collect();
        (end, names)
    }
    /// Bundle count visible strictly before journal position `visible`.
    fn bundles_before(&self, visible: usize) -> usize {
        self.bundle_count_at
            .get(visible)
            .copied()
            .unwrap_or(self.bundles.len())
    }
}
fn write_opt_usize(out: &mut String, value: Option<usize>) {
    use std::fmt::Write;
    match value {
        Some(n) => {
            let _ = write!(out, "{n}");
        }
        None => out.push_str("null"),
    }
}
fn write_bundle_fields(out: &mut String, pos: usize, order: usize, bundle: &BundleRecord) {
    use std::fmt::Write;
    let _ = write!(out, "{pos},{order},{},[", bundle.unit);
    for (i, name) in bundle.products.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        crate::source_map::json_string(out, name);
    }
    out.push_str("],[");
    for (i, (protocol, selector, identity)) in bundle.extensions.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        crate::source_map::json_string(out, protocol);
        out.push(',');
        match selector {
            Some(s) => crate::source_map::json_string(out, s),
            None => out.push_str("null"),
        }
        out.push(',');
        crate::source_map::json_string(out, identity);
        out.push(']');
    }
    out.push_str("],");
    crate::source_map::json_string(out, &bundle.location);
    let _ = write!(out, ",{},{}", bundle.once, bundle.group);
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
    /// The deadline and cancellation of the build's isolates, and their
    /// macro memory pool ([`crate::evaluator::BuildResources`]).
    resources: std::sync::OnceLock<crate::evaluator::BuildResources>,
    /// One more than the ID of the earliest failed pool job in declaration
    /// order, or 0. The ID locates the cutoff; IDs themselves are not ordered.
    /// Updates hold the catalog lock; the atomic keeps the no-failure fast path.
    failed_job: std::sync::atomic::AtomicUsize,
    /// What the compilation records of each analysis beyond its charges,
    /// which a unit recorded off the coordinator hands it
    /// ([`crate::macro_host::UnitRecording`]); set when the pool starts.
    recording: std::sync::OnceLock<crate::macro_host::UnitRecording>,
    /// Whether [`Catalog::queued_ir`] is measured: only with `--stats`, so
    /// that a build without reads one relaxed load per unit.
    measuring_ir: std::sync::atomic::AtomicBool,
    /// Atomic mirrors of the shared journal/root-index cursors so workers and
    /// the coordinator can skip locking the catalog mutex when nothing new was
    /// registered since their last sync.
    journal_len: std::sync::atomic::AtomicUsize,
    bundle_count: std::sync::atomic::AtomicUsize,
    ext_protocol_count: std::sync::atomic::AtomicUsize,
    /// The build's test-only perturbation ([`crate::scheduler::Perturbation`]):
    /// the unit pool's scheduling, and the unit whose analysis panics.
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
    engine_participants: Mutex<std::collections::HashSet<usize>>,
    engine_less_units: std::sync::atomic::AtomicUsize,
    pool_ns: std::sync::atomic::AtomicU64,
    /// What the pool's scheduler did, once it has stopped.
    scheduler: Mutex<crate::scheduler::PoolCounts>,
    /// Acquisitions of the catalog's lock that found it held.
    catalog_contended: std::sync::atomic::AtomicU64,
}
impl PoolCounters {
    fn note_engine_participant(&self, slot: usize) {
        self.engine_participants
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(slot);
    }
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
    /// Wake any coordinator or worker waiting on `self.settled` without risking
    /// a lost wakeup between predicate check and `Condvar::wait`.
    pub(crate) fn notify_settled(&self) {
        let _guard = self.lock();
        self.settled.notify_all();
    }
    /// Snapshot `(journal_len, bundle_count, environment_bundles)` under one
    /// catalog lock before launching a parallel top-level namespace wave.
    pub(crate) fn overlay_snapshot_marks(&self) -> (usize, usize, usize) {
        let c = self.lock();
        (
            c.journal.registrations.len(),
            c.journal.bundle_count,
            c.journal.environment_bundles,
        )
    }
    /// Read a `defmulti` target from the shared journal (used by a parallel
    /// namespace overlay when the target wasn't defined locally).
    fn multifn_target(&self, name: &str) -> Option<String> {
        self.lock().journal.multifn_targets.get(name).cloned()
    }
    /// Wake the threads waiting for a pool job that will not start
    /// ([`Self::admits`]), which [`Self::claim`] then fails.
    fn skipped(&self) {
        let _c = self.lock();
        self.settled.notify_all();
    }
    /// The pool jobs of `(id, macro_free)` pairs: with `mixed`, in a pool whose
    /// workers past the first ones have no engine, a unit not found macro-free
    /// is for the ones with an engine only ([`UNIT_ENGINE_PHASE`]); macro-free
    /// units take [`UNIT_PLAIN_PHASE`]. In an unmixed pool all units take phase
    /// [`UNMIXED_UNIT_PHASE`] (below top-level namespace phase 0).
    fn unit_jobs(&self, units: Vec<(usize, bool)>, mixed: bool) -> Vec<UnitJob> {
        if let Some(&(first, _)) = units.first() {
            crate::wait_timeline::instant(
                crate::wait_timeline::Kind::Submit,
                first as u64,
                units.len() as u64,
            );
        }
        units
            .into_iter()
            .map(|(id, macro_free)| {
                let phase = if mixed {
                    if macro_free {
                        UNIT_PLAIN_PHASE
                    } else {
                        UNIT_ENGINE_PHASE
                    }
                } else {
                    UNMIXED_UNIT_PHASE
                };
                UnitJob::Analyze(id, phase, macro_free)
            })
            .collect()
    }
    /// Note that pool job `id` ended with its unit failed, if it did.
    fn note_job(&self, id: usize) {
        let catalog = self.lock();
        if matches!(catalog.units[id].state, AnalysisState::Failed(..)) {
            use std::sync::atomic::Ordering::Relaxed;
            let failed = self.failed_job.load(Relaxed);
            if failed == 0 || id < failed - 1 {
                self.failed_job.store(id + 1, Relaxed);
            }
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
    /// What the build's isolates share once its resources are initialized.
    pub(crate) fn resources(&self) -> Option<&crate::evaluator::BuildResources> {
        self.resources.get()
    }
    pub(crate) fn initialize_resources(
        &self,
        options: &crate::evaluator::EvaluatorOptions,
        isolates: usize,
        reserve_workers: bool,
    ) -> Result<&crate::evaluator::BuildResources, crate::evaluator::EvalError> {
        let resources = match self.resources() {
            Some(resources) => resources,
            None => {
                let resources =
                    crate::evaluator::BuildResources::new(options, isolates, reserve_workers)?;
                self.resources.get_or_init(|| resources)
            }
        };
        if resources.isolates != isolates {
            return Err(crate::evaluator::EvalError(
                "compile-time engine resources already initialized for a different pool size"
                    .into(),
            ));
        }
        Ok(resources)
    }
    /// What the compilation records of each analysis beyond its charges.
    fn recording(&self) -> crate::macro_host::UnitRecording {
        self.recording.get().copied().unwrap_or_default()
    }
    /// The expansions and generated nodes the coordinator has charged.
    pub(crate) fn charged(&self) -> (usize, usize) {
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
    /// V8 coordinator never analyzes a pooled unit in its own isolate,
    /// whose compile-time state would then depend on scheduling.
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
                // The producer's state as this wait starts: 1 analyzing on
                // another thread, 2 queued and unstarted.
                let producer = match c.units[id].state {
                    AnalysisState::Analyzing(owner) => 1 | ((owner as u64) << 8),
                    _ => 2,
                };
                let span = crate::wait_timeline::begin(
                    crate::wait_timeline::Kind::UnitWait,
                    id as u64,
                    producer,
                );
                if let Some(help) = help {
                    drop(guard);
                    let helped = help();
                    guard = self.lock();
                    match helped {
                        Ok(true) => {
                            span.end(2, 0);
                            continue;
                        }
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
                        span.end(2, 0);
                        continue;
                    }
                    let (next, timeout) = self
                        .settled
                        .wait_timeout(guard, std::time::Duration::from_millis(20))
                        .unwrap_or_else(PoisonError::into_inner);
                    guard = next;
                    span.end(timeout.timed_out() as u64, 0);
                    continue;
                }
                guard = self
                    .settled
                    .wait(guard)
                    .unwrap_or_else(PoisonError::into_inner);
                span.end(0, 0);
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
                AnalysisState::Analyzed(program, warnings, _) => {
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
            return Ok(self.start_unit(c, thread, id));
        }
    }
    /// Claim an unstarted unit under the catalog lock.
    fn start_unit(&self, c: &mut Catalog, thread: usize, id: usize) -> Claim {
        let depth = c.threads[thread].stack.len();
        let unit = &mut c.units[id];
        debug_assert!(matches!(unit.state, AnalysisState::Unstarted));
        let inputs = unit.inputs.take().expect("unstarted unit inputs");
        if depth >= inputs.environment.limits().expansion_depth.min(64) {
            unit.state = AnalysisState::Failed(DEPTH_LIMIT.to_owned(), None);
            if c.waiting > 0 {
                self.settled.notify_all();
            }
            return Claim::TooDeep(inputs, depth);
        }
        unit.state = AnalysisState::Analyzing(thread);
        unit.analyses += 1;
        let (name, macro_free) = (unit.name.clone(), unit.macro_free);
        debug_assert!(unit.request.generation > 0);
        debug_assert_eq!(c.bindings[&unit.name].0, unit.request.var);
        c.threads[thread].stack.push(id);
        Claim::Analyze(inputs, depth, name, macro_free)
    }
    /// End the analysis of unit `id` that the thread in slot `thread` claimed
    /// with `state`, or as interrupted, recording optional `charges` and
    /// updating `failed_job` in the same lock acquisition before waking waiters.
    fn settle(
        &self,
        thread: usize,
        id: usize,
        state: Option<AnalysisState>,
        held: usize,
        charges: Option<(UnitCharges, std::time::Duration)>,
        note_failed: bool,
    ) {
        let (previous, waiting) = {
            let mut c = self.lock();
            let c = &mut *c;
            if held > 0 && matches!(state, Some(AnalysisState::Analyzed(..))) {
                c.queued_ir.hold(id, held);
            }
            let popped = c.threads[thread].stack.pop();
            debug_assert_eq!(popped, Some(id));
            let unit = &mut c.units[id];
            if let Some((charges, time)) = charges {
                unit.charges = Some(Box::new(charges));
                unit.time = time;
            }
            let next_state =
                state.unwrap_or_else(|| AnalysisState::Failed("analysis interrupted".into(), None));
            let next_failed = matches!(next_state, AnalysisState::Failed(..));
            let previous = matches!(unit.state, AnalysisState::Analyzing(_))
                .then(|| std::mem::replace(&mut unit.state, next_state));
            let failed = previous.is_some() && next_failed;
            if failed && note_failed {
                use std::sync::atomic::Ordering::Relaxed;
                let prev_failed = self.failed_job.load(Relaxed);
                if prev_failed == 0 || id < prev_failed - 1 {
                    self.failed_job.store(id + 1, Relaxed);
                }
            }
            (previous, c.waiting > 0)
        };
        crate::wait_timeline::instant(crate::wait_timeline::Kind::Settle, id as u64, 0);
        drop(previous);
        if waiting {
            self.settled.notify_all();
        }
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
        let _forward = crate::wait_timeline::begin(crate::wait_timeline::Kind::Forward, 0, 0);
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
        if waiting {
            self.settled.notify_all();
        }
    }
    /// End every wait with `message`: the pool stopped, so units it was
    /// given may never be analyzed.
    pub(crate) fn abandon(&self, message: String) {
        self.lock().abandoned.get_or_insert(message);
        self.settled.notify_all();
    }
    /// How many extension bundles the compilation registered.
    pub(crate) fn extension_frontier(&self) -> usize {
        self.lock().journal.bundle_count
    }
    /// The unit of the root `name` names, resolved in [`Catalog::roots`]
    /// against registrations before `visible` (or the whole journal).
    #[cfg(all(test, light_v8))]
    pub(crate) fn resolve_root(&self, name: &str, visible: Option<usize>) -> Result<usize, String> {
        let mut c = self.lock();
        c.root_resolutions += 1;
        let position = visible.unwrap_or(c.journal.registrations.len());
        c.roots.resolve(name, position)
    }
    /// Answer a `__lightRootService` query from `src/compiler_roots.js` against
    /// registrations strictly before `visible` (or the whole shared journal).
    fn serve_root_request(&self, request: &str, visible: Option<usize>) -> String {
        use std::fmt::Write;
        let mut parts = request.split('\0');
        let Some(kind) = parts.next() else {
            return String::new();
        };
        let c = self.lock();
        let max_pos = visible.unwrap_or(c.journal.registrations.len());
        match kind {
            "n" => {
                let (Some(name), Some(from), Some(limit)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return String::new();
                };
                let Ok(from) = from.parse::<usize>() else {
                    return String::new();
                };
                let Ok(limit) = limit.parse::<isize>() else {
                    return String::new();
                };
                let to_pos = if limit < 0 {
                    max_pos
                } else {
                    (limit as usize).min(max_pos)
                };
                if from >= to_pos {
                    return String::new();
                }
                let Some(positions) = c.roots.name_positions.get(name) else {
                    return String::new();
                };
                let start = positions.partition_point(|&pos| pos < from);
                let end = positions.partition_point(|&pos| pos < to_pos);
                if start >= end {
                    return String::new();
                }
                let mut out = String::new();
                let _ = write!(out, "[{to_pos},[");
                for (i, &pos) in positions[start..end].iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    let decl_frontier = c.roots.bundle_count_at[pos];
                    match &c.journal.registrations[pos] {
                        Registration::Root(_, unit, test, once, location) => {
                            let _ = write!(out, "[0,{pos},");
                            write_opt_usize(&mut out, *unit);
                            out.push(',');
                            write_opt_usize(&mut out, *test);
                            let _ = write!(out, ",{once},");
                            crate::source_map::json_string(&mut out, location);
                            let _ = write!(out, ",{decl_frontier}]");
                        }
                        Registration::Bundle(bundle) => {
                            // `bundle_count_at[pos]` is bundles strictly before `pos`,
                            // so this bundle's 1-based order is `decl_frontier + 1`.
                            out.push_str("[1,");
                            write_bundle_fields(&mut out, pos, decl_frontier + 1, bundle);
                            out.push(']');
                        }
                    }
                }
                out.push_str("]]");
                out
            }
            "p" => {
                let (Some(protocol), Some(from), Some(to)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return String::new();
                };
                let (Ok(from), Ok(to)) = (from.parse::<usize>(), to.parse::<usize>()) else {
                    return String::new();
                };
                let to_pos = to.min(max_pos);
                if from >= to_pos {
                    return String::new();
                }
                let Some(orders) = c.roots.protocol_bundles.get(protocol) else {
                    return String::new();
                };
                let start = orders.partition_point(|&order| c.roots.bundles[order - 1].0 < from);
                let end = orders.partition_point(|&order| c.roots.bundles[order - 1].0 < to_pos);
                if start >= end {
                    return String::new();
                }
                let mut out = String::from("[");
                for (i, &order) in orders[start..end].iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    let (pos, bundle) = &c.roots.bundles[order - 1];
                    out.push('[');
                    write_bundle_fields(&mut out, *pos, order, bundle);
                    out.push(']');
                }
                out.push(']');
                out
            }
            "t" => {
                let Some(unit) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
                    return String::new();
                };
                let Some(&pos) = c.roots.unit_positions.get(&unit) else {
                    return String::new();
                };
                if pos >= max_pos {
                    return String::new();
                }
                if let Registration::Root(name, ..) = &c.journal.registrations[pos] {
                    let mut out = String::new();
                    let _ = write!(out, "[{pos},");
                    crate::source_map::json_string(&mut out, name);
                    out.push(']');
                    out
                } else {
                    String::new()
                }
            }
            _ => String::new(),
        }
    }
    /// How many of them [`reaches_environment_data`].
    pub(crate) fn environment_bundles(&self) -> usize {
        self.lock().journal.environment_bundles
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
    #[allow(dead_code)]
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
    /// Single-lock fast path for draining an already-analyzed, worker-prepared
    /// unit: clears `held`, takes `charges`, reads `stack.len()` and `dispatch`,
    /// and transitions `Analyzed` -> `Prepared` while returning `Arc<Program>`.
    #[allow(clippy::type_complexity)]
    fn drain_ready(
        &self,
        id: usize,
        thread: usize,
    ) -> Option<(
        Arc<Program>,
        Vec<AnalysisWarning>,
        Option<Box<UnitCharges>>,
        usize,
        crate::namespace_artifact::UnitDispatch,
    )> {
        let mut c = self.lock();
        let unit = c.units.get_mut(id)?;
        if !matches!(unit.state, AnalysisState::Analyzed(_, _, Some(_))) {
            return None;
        }
        unit.held = false;
        let charges = unit.charges.take();
        let dispatch = unit.dispatch;
        let AnalysisState::Analyzed(program, warnings, Some(prepared)) =
            std::mem::replace(&mut unit.state, AnalysisState::Unstarted)
        else {
            unreachable!()
        };
        unit.state = AnalysisState::Prepared(Arc::downgrade(&program), warnings.clone(), prepared);
        if unit.waiters > 0 {
            unit.retained = Some(program.clone());
        }
        let depth = c.threads[thread].stack.len();
        Some((program, warnings, charges, depth, dispatch))
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
    }
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
    /// How many of the catalog's registrations `bootstrap` holds (or is synced to).
    installed: usize,
    /// How many bundles `bootstrap` is synced to (V8 RootIndex path).
    synced_bundles: usize,
    /// How many entries of [`RootIndex::extended_protocols`] `bootstrap` holds.
    synced_ext_protocols: usize,
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
    /// The namespaces installed artifacts added to the compile-time
    /// registry before the session started, which it adds when it starts
    /// ([`Self::reserve_namespaces`]), each after `p` or `c`
    /// ([`Self::create_namespaces`]).
    namespaces_pending: RefCell<Vec<String>>,
    /// Whether capture reads what the compile-time registry gained.
    namespaces_logged: Cell<bool>,
    /// How many of the program's first namespaces installed artifacts have
    /// added to the registry, or know it has.
    namespaces_ensured: Cell<usize>,
    /// What pool workers' units added to their registries, absorbed before
    /// the session started ([`Self::absorb_namespaces`]), for the next
    /// [`Self::take_namespaces`].
    namespaces_absorbed: RefCell<Vec<crate::namespace_artifact::RegistryEntry>>,
    /// Every namespace logged so far, with the [`placement_rank`] of the
    /// most coordinator-bound placement that logged it: an entry is logged
    /// again only from a placement of a higher rank (light-cljs-1006).
    namespaces_seen: RefCell<std::collections::HashMap<String, u8>>,
    /// The build's unit pool, with `--jobs N` ([`UnitPool`]).
    pool: RefCell<Option<UnitPool>>,
    /// Number of macro isolates planned for the pool, including the
    /// coordinator, whose isolate may start before the pool does.
    #[cfg(light_v8)]
    v8_pool_isolates: Cell<usize>,
    #[cfg(light_v8)]
    v8_pool_elided: Cell<bool>,
    /// The last unit, in unit order, a drain point took
    /// ([`Self::analyze_drained`] or [`Self::note_drained`]); a failed build's
    /// counts stop there ([`Self::failed_unit_counts`]).
    drained_through: Cell<Option<usize>>,
    /// Private local overlay used while analyzing a namespace's top-level pass
    /// on a pool worker thread (Stage C, light-cljs-1090). `None` on the
    /// coordinator.
    local_overlay: RefCell<Option<NamespaceOverlay>>,
    /// Namespaces whose top-level pass entered a compile-time JS engine on a
    /// parallel worker; materialized lazily into the coordinator's JS namespace
    /// registry only if `find-ns` / `all-ns` actually reflects on it.
    deferred_engine_namespaces: RefCell<Vec<String>>,
}
/// Base ID for units registered inside a worker's private [`NamespaceOverlay`]
/// before canonical unit IDs are assigned at publication time.
pub(crate) const LOCAL_UNIT_BASE: usize = 1 << 30;

/// Immutable pool sizing policy copied into a parallel namespace worker so
/// `pools()` and `pools_bundle()` produce the exact same classification as on
/// the coordinator.
#[derive(Clone, Copy, Debug)]
pub(crate) struct PoolPolicy {
    pub(crate) engines: PoolEngines,
    pub(crate) workers: usize,
    pub(crate) engine_workers: usize,
}

/// A deferred unit registered inside a parallel namespace worker's private
/// overlay before being relocated to a canonical `SharedCatalog` unit ID.
pub(crate) struct LocalUnit {
    raw_name: RegistrationName,
    source: u32,
    inputs: Option<Box<UnitInputs>>,
    state: AnalysisState,
    analyses: usize,
    emitted: bool,
    macro_free: bool,
    pooled: bool,
    dispatch: crate::namespace_artifact::UnitDispatch,
    view_globals: usize,
    /// Number of bundles registered earlier in THIS namespace before this unit.
    local_bundle_offset: usize,
}

/// Private namespace-local registration buffer layered over
/// `(shared, prefix_cursor)` during a parallel top-level namespace pass.
struct NamespaceOverlay {
    prefix_cursor: usize,
    prefix_bundles: usize,
    prefix_env_bundles: usize,
    units: Vec<LocalUnit>,
    registrations: Vec<Registration>,
    roots: RootIndex,
    multifn_targets: HashMap<String, String>,
    local_env_bundles: usize,
    local_redefined: bool,
    pool_policy: PoolPolicy,
    capturing: bool,
    captured_macros: Vec<CapturedMacro>,
    /// Set whenever V8 actually observed a local root/bundle/unit,
    /// telling the worker to retire its isolate after this namespace pass.
    touched_local_in_engine: bool,
    /// Set whenever V8 reflected on the compile-time namespace registry
    /// (`find-ns` / `the-ns` / `all-ns`) during this overlay pass, signaling an
    /// immediate fallback to the coordinator's canonical namespace registry.
    reflected_in_engine: bool,
    /// Namespaces created via `createNs` (outside `registerReaderNamespace`)
    /// during this overlay pass, in exact creation order, so the coordinator
    /// can materialize them lazily before any later `createNs`/`findNs`/`allNs`.
    created_namespaces: Vec<String>,
    /// Whether this overlay synced a temporary overlay window into V8
    /// (`"\0overlay"`) that must be cleared via `"\0clear"` when reusing the
    /// isolate.
    overlay_synced: bool,
    /// Number of local `extended_protocols` already delivered to the worker's
    /// engine overlay window.
    synced_local_ext: usize,
    synced_local_cursor: usize,
}

/// Output extracted from a completed worker [`NamespaceOverlay`] for canonical
/// publication into the coordinator's [`SharedCatalog`].
pub(crate) struct NamespaceOverlayOutput {
    units: Vec<LocalUnit>,
    registrations: Vec<Registration>,
    captured_macros: Vec<CapturedMacro>,
    pub(crate) touched_local_in_engine: bool,
    pub(crate) reflected_in_engine: bool,
    created_namespaces: Vec<String>,
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
    /// The canonical globals count at the unit's definition snapshot, used by
    /// `artifact_capture::Converter::view`.
    view_globals: usize,
    /// The unit's own `snapshot.resolution_program.globals.len()`, which may
    /// differ from `view_globals` for overlay units whose `WaveSnapshot` was
    /// taken before earlier siblings published.
    fragment_globals: usize,
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
            namespaces_pending: Default::default(),
            namespaces_logged: Default::default(),
            namespaces_ensured: Default::default(),
            namespaces_absorbed: Default::default(),
            namespaces_seen: Default::default(),
            pool: Default::default(),
            #[cfg(light_v8)]
            v8_pool_isolates: Default::default(),
            #[cfg(light_v8)]
            v8_pool_elided: Default::default(),
            drained_through: Default::default(),
            local_overlay: RefCell::new(None),
            deferred_engine_namespaces: Default::default(),
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
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            return overlay.capturing;
        }
        self.capture.borrow().is_some()
    }
    /// How many units, registrations and bundles the catalog holds.
    pub fn capture_marks(&self) -> (usize, usize, usize) {
        let c = self.shared.lock();
        (
            c.units.len(),
            c.journal.registrations.len(),
            c.journal.bundle_count,
        )
    }
    pub fn capture_macro(
        &self,
        key: String,
        code: String,
        view_globals: usize,
        reader_context: Option<crate::analyzer::RuntimeReaderNamespace>,
    ) {
        if let Some(overlay) = self.local_overlay.borrow_mut().as_mut() {
            if overlay.capturing {
                overlay.captured_macros.push(CapturedMacro {
                    key,
                    code,
                    view_globals,
                    reader_context,
                });
            }
            return;
        }
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
                .filter(|event| {
                    matches!(
                        event,
                        UnitEvent::Expansion(e) if !e.exempt
                    ) || matches!(event, UnitEvent::Script { exempt: false, .. })
                })
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
    /// The names of `namespace`'s globals the analyses of units `units` add
    /// to their fragments beyond their definition snapshots, which the
    /// program gets only when they merge at the namespace's drain point, or
    /// `None` when one of them has no analyzed fragment to read (not
    /// analyzed yet, failed or released) or capture is off
    /// (light-cljs-347, `compile_cache::declaration_view`). The units in
    /// `skip`, declaration bundles', are left out: their products are
    /// the top-level pass's globals, and their fragments are released.
    ///
    /// When `merged_program` is `Some`, the units have already merged at
    /// `merge_front`, so their added globals are read directly from the
    /// captured `added` global indices on `merged_program`. Otherwise (at a
    /// miss->hit barrier before merge), `macro_free` units are skipped because
    /// they cannot define globals, and non-`macro_free` settled fragments are
    /// read beyond `fragment_globals`.
    pub(crate) fn unit_global_names(
        &self,
        units: std::ops::Range<usize>,
        namespace: &str,
        skip: &[usize],
        merged_program: Option<&Program>,
    ) -> Option<Vec<String>> {
        if let Some(program) = merged_program {
            let capture = self.capture.borrow();
            let capture = capture.as_ref()?;
            let mut names = Vec::new();
            for unit in units {
                if skip.contains(&unit) {
                    continue;
                }
                let entry = unit
                    .checked_sub(capture.first_unit)
                    .and_then(|index| capture.units.get(index))?;
                for &global_id in &entry.added {
                    let global = &program.globals[global_id];
                    if global.namespace == namespace {
                        names.push(global.name.clone());
                    }
                }
            }
            return Some(names);
        }
        let views: Vec<usize> = {
            let capture = self.capture.borrow();
            let capture = capture.as_ref()?;
            units
                .clone()
                .map(|unit| {
                    unit.checked_sub(capture.first_unit)
                        .and_then(|index| capture.units.get(index))
                        .map(|entry| entry.fragment_globals)
                })
                .collect::<Option<_>>()?
        };
        let c = self.shared.lock();
        let mut names = Vec::new();
        for (unit, view) in units.zip(views) {
            if skip.contains(&unit) {
                continue;
            }
            let entry = c.units.get(unit)?;
            if entry.macro_free {
                continue;
            }
            let program = match &entry.state {
                AnalysisState::Analyzed(program, ..) => program.clone(),
                AnalysisState::Prepared(program, ..) => program.upgrade()?,
                _ => return None,
            };
            names.extend(
                program
                    .globals
                    .iter()
                    .skip(view)
                    .filter(|global| global.namespace == namespace)
                    .map(|global| global.name.clone()),
            );
        }
        Some(names)
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
            .journal
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
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            return LOCAL_UNIT_BASE + overlay.units.len();
        }
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
        let mut needs_sync = false;
        for registration in registrations {
            let effect = {
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
                                // Prepared: never analyzed again.
                                dispatch: crate::namespace_artifact::UnitDispatch::Definition,
                                held: false,
                                dispatched: false,
                                charges: None,
                                time: Default::default(),
                            });
                        }
                        self.record_registration(
                            c,
                            Registration::Root(name.clone(), *unit, *test, *once, location.clone()),
                        )
                    }
                    InstalledRegistration::Bundle(bundle) => {
                        self.record_registration(c, Registration::Bundle(bundle.clone()))
                    }
                }
            };
            if effect.redefined || effect.has_extensions {
                needs_sync = true;
            }
        }
        if units.next().is_some() {
            return Err("an installed unit has no root".into());
        }
        if needs_sync {
            self.sync_catalog(None)?;
        }
        Ok(())
    }
    pub fn bootstrap(&self) -> Option<Bootstrap> {
        self.engine.borrow().bootstrap.clone()
    }
    /// Sync the coordinator's V8 engine with the latest catalog cursor,
    /// bundle count and newly extended protocols from [`Catalog::roots`],
    /// without replaying individual registrations. Lock-free when nothing new
    /// was registered or no new protocol was extended.
    pub(crate) fn sync_catalog(&self, active: Option<&dyn MacroEvaluator>) -> Result<(), String> {
        if self.local_overlay.borrow().is_some() {
            let sync = {
                let mut overlay_ref = self.local_overlay.borrow_mut();
                let overlay = overlay_ref.as_mut().unwrap();
                if overlay.registrations.is_empty() {
                    None
                } else {
                    let total = overlay.prefix_cursor + overlay.registrations.len();
                    let bundles = overlay.prefix_bundles + overlay.roots.bundles.len();
                    let ext_total = overlay.roots.extended_protocols.len();
                    if overlay.overlay_synced
                        && total <= overlay.synced_local_cursor
                        && ext_total <= overlay.synced_local_ext
                    {
                        None
                    } else {
                        let (new_ext, new_protocols) = overlay.roots.new_extended_protocols(
                            overlay.synced_local_ext,
                            overlay.registrations.len(),
                        );
                        overlay.synced_local_ext = new_ext;
                        overlay.synced_local_cursor = total;
                        overlay.overlay_synced = true;
                        let mut tagged = Vec::with_capacity(new_protocols.len() + 3);
                        tagged.push("\0overlay".to_owned());
                        tagged.push(overlay.prefix_cursor.to_string());
                        tagged.push(overlay.prefix_bundles.to_string());
                        tagged.extend(new_protocols);
                        Some((total, bundles, tagged))
                    }
                }
            };
            if let Some((total, bundles, tagged)) = sync {
                let bootstrap_hold = active.is_none().then(|| self.bootstrap()).flatten();
                let evaluator = match (active, &bootstrap_hold) {
                    (Some(engine), _) => engine,
                    (None, Some(bootstrap)) => bootstrap.evaluator.as_ref(),
                    (None, None) => return Ok(()),
                };
                evaluator
                    .sync_catalog(total, bundles, &tagged)
                    .map_err(|e| e.to_string())?;
            }
            return Ok(());
        }
        use std::sync::atomic::Ordering::Acquire;
        let total = self.shared.journal_len.load(Acquire);
        let bundles = self.shared.bundle_count.load(Acquire);
        let ext_count = self.shared.ext_protocol_count.load(Acquire);
        let (engine_ref, bootstrap_hold, prev_ext) = {
            let mut state = self.engine.borrow_mut();
            let Some(bootstrap) = state.bootstrap.as_ref() else {
                return Ok(());
            };
            if state.installed >= total
                && state.synced_bundles >= bundles
                && state.synced_ext_protocols >= ext_count
            {
                return Ok(());
            }
            let prev_ext = state.synced_ext_protocols;
            let bootstrap_hold = active.is_none().then(|| bootstrap.clone());
            state.installed = total;
            state.synced_bundles = bundles;
            (active, bootstrap_hold, prev_ext)
        };
        let new_protocols = if ext_count > prev_ext {
            let c = self.shared.lock();
            let (new_synced, protocols) = c.roots.new_extended_protocols(prev_ext, total);
            drop(c);
            self.engine.borrow_mut().synced_ext_protocols = new_synced;
            protocols
        } else {
            Vec::new()
        };
        let evaluator = match (engine_ref, &bootstrap_hold) {
            (Some(engine), _) => engine,
            (None, Some(bootstrap)) => bootstrap.evaluator.as_ref(),
            (None, None) => return Ok(()),
        };
        evaluator
            .sync_catalog(total, bundles, &new_protocols)
            .map_err(|e| e.to_string())
    }
    fn with_journal<T>(&self, read: impl FnOnce(&RegistrationJournal) -> T) -> T {
        read(&self.shared.lock().journal)
    }
    fn record_registration(
        &self,
        catalog: &mut Catalog,
        registration: Registration,
    ) -> RecordEffect {
        use std::sync::atomic::Ordering::Release;
        let position = catalog.journal.registrations.len();
        let effect = catalog.roots.record(position, &registration);
        catalog.journal.push(registration);
        self.shared.journal_len.store(effect.journal_len, Release);
        self.shared.bundle_count.store(effect.bundle_count, Release);
        self.shared
            .ext_protocol_count
            .store(effect.ext_protocol_count, Release);
        effect
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
    /// Enable permanent `__lightGensym` logging in `engine`'s runtime once per
    /// isolate without decoding the returned vector (`build.rs` keeps
    /// `gensymLog = true` permanently once `takeGensyms()` runs).
    pub(crate) fn enable_gensym_log(engine: &dyn MacroEvaluator) -> Result<(), String> {
        let value = engine
            .evaluate_unit("rt => { rt.compilerForms.takeGensyms(); return null; }")
            .map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())
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
    /// Start logging the namespaces the compile-time registry gains, which
    /// `all-ns` and `find-ns` read, for [`Self::take_namespaces`].
    pub fn log_namespaces(&self) -> Result<(), String> {
        self.namespaces_logged.set(true);
        match self.bootstrap() {
            Some(bootstrap) => {
                Self::take_namespaces_from(bootstrap.evaluator.as_ref(), false).map(drop)
            }
            None => Ok(()),
        }
    }
    /// The namespaces the compile-time registry gained since the last call
    /// or since [`Self::log_namespaces`], in the order it gained them: an
    /// artifact's [`crate::namespace_artifact::NamespaceArtifact::registry`].
    pub fn take_namespaces(&self) -> Result<Vec<crate::namespace_artifact::RegistryEntry>, String> {
        if !self.namespaces_logged.get() {
            return Ok(Vec::new());
        }
        if let Some(bootstrap) = self.bootstrap() {
            self.flush_namespaces(bootstrap.evaluator.as_ref(), false)?;
        }
        Ok(std::mem::take(&mut *self.namespaces_absorbed.borrow_mut()))
    }
    /// Move what `engine`, this session's, logged into its registry so far
    /// into what [`Self::take_namespaces`] returns, ahead of what a unit
    /// charged later adds, as [`Self::flush_gensyms`] does. What it logs
    /// next is of a unit a pool may place with `pooled`, and of an analysis
    /// the coordinator always runs without.
    pub(crate) fn flush_namespaces(
        &self,
        engine: &dyn MacroEvaluator,
        pooled: bool,
    ) -> Result<(), String> {
        if !self.namespaces_logged.get() {
            return Ok(());
        }
        // The coordinator's own analyses. A pool worker's unit may have
        // logged a namespace before, in a registry of its own, and this one
        // gains it now all the same; with one job, the engine logs it again
        // when it touches one only such units added (light-cljs-1006).
        let logged = Self::take_namespaces_from(engine, pooled)?;
        self.note_namespaces(logged, None);
        Ok(())
    }
    /// Log `entries`, placed as `unit` places its analysis, those no
    /// placement of at least its [`placement_rank`] has logged, so that a
    /// namespace's artifact is the same at every `--jobs`: each entry is
    /// where the registry of a coordinator that analyzes placements of that
    /// rank, and none of a higher one, first gains the namespace.
    fn note_namespaces(
        &self,
        entries: Vec<crate::namespace_artifact::RegistryEntry>,
        unit: Option<crate::namespace_artifact::UnitDispatch>,
    ) {
        let rank = placement_rank(unit);
        let mut seen = self.namespaces_seen.borrow_mut();
        self.namespaces_absorbed.borrow_mut().extend(
            entries
                .into_iter()
                .filter(|entry| note_rank(&mut seen, &entry.namespace, rank))
                .map(|entry| crate::namespace_artifact::RegistryEntry { unit, ..entry }),
        );
    }
    /// Log `entries`, what a deferred unit's analysis added to its engine's
    /// registry or touched there after only units a pool may place added
    /// it, where the coordinator charges the unit, whichever engine analyzed
    /// it, so that its namespace's artifact is the same at every `--jobs`
    /// (light-cljs-1004), marked with the unit's `dispatch`, which
    /// installing reads (light-cljs-1005), those no placement of its rank or
    /// a higher one logged ([`Self::note_namespaces`], light-cljs-1006). A
    /// pool worker's registry is its own: the coordinator's does not gain
    /// them.
    fn absorb_namespaces(
        &self,
        entries: Vec<crate::namespace_artifact::RegistryEntry>,
        dispatch: crate::namespace_artifact::UnitDispatch,
    ) -> Result<(), String> {
        if entries.is_empty() || !self.namespaces_logged.get() {
            return Ok(());
        }
        self.note_namespaces(entries, Some(dispatch));
        Ok(())
    }
    /// Whether this build analyzes on the coordinator, in its registry, a
    /// unit of `dispatch` that adds to the registry, and so is not
    /// macro-free, as its namespace's dispatch decides ([`Self::pools`],
    /// [`Self::pools_bundle`]); `None`, the namespace's own analysis,
    /// always. A pool worker with an engine analyzes the others in a
    /// registry of its own (light-cljs-1005).
    fn analyzes_on_coordinator(
        &self,
        dispatch: Option<crate::namespace_artifact::UnitDispatch>,
    ) -> bool {
        use crate::namespace_artifact::UnitDispatch;
        match dispatch {
            None | Some(UnitDispatch::Coordinator) => true,
            Some(UnitDispatch::Definition) => !self.pools(false, false),
            Some(UnitDispatch::Bundle) => !self.pools_bundle(false),
        }
    }
    /// Add `entries`, the namespaces an installed artifact's analysis added
    /// to the compile-time registry, in its order, as that analysis did:
    /// before one a registration added, the namespaces of `program` before
    /// it (`position` finds them) that the registry may lack, as the
    /// registration added them. Before the session starts, when it starts.
    /// They are not logged: the next capture is of the analyses after the
    /// installed one (light-cljs-1004). Every entry counts as logged, with
    /// its placement, where this build installs it or not, as it was where
    /// the analysis ran; a namespace only placements of a lower rank than
    /// the coordinator's own logged is added `pooled`, so that a later touch
    /// logs it again, as it would have after that analysis (light-cljs-1006).
    pub fn reserve_namespaces(
        &self,
        entries: &[crate::namespace_artifact::RegistryEntry],
        program: &crate::arena::Arena<String>,
        position: impl Fn(&str) -> Option<usize>,
    ) -> Result<(), String> {
        if entries.is_empty() {
            return Ok(());
        }
        // The program's namespaces before this position are in the
        // registry, or among `names`.
        let mut ensured = self.namespaces_ensured.get();
        let mut names = Vec::new();
        let mut seen = self.namespaces_seen.borrow_mut();
        for entry in entries {
            let rank = placement_rank(entry.unit);
            note_rank(&mut seen, &entry.namespace, rank);
            // With engines in the pool, a unit the pool takes added its
            // namespaces to a worker's registry, not this one.
            if !self.analyzes_on_coordinator(entry.unit) {
                continue;
            }
            if entry.registered
                && let Some(at) = position(&entry.namespace)
                && at < program.len()
            {
                if at > ensured {
                    for name in program.range(ensured..at) {
                        // In the registration's placement, as it adds them.
                        note_rank(&mut seen, name, rank);
                        names.push(name.clone());
                    }
                }
                ensured = ensured.max(at + 1);
            }
            names.push(entry.namespace.clone());
        }
        self.namespaces_ensured.set(ensured);
        // Each after its mark, as the registry's last touch leaves it.
        let names: Vec<String> = names
            .into_iter()
            .map(|name| {
                let pooled = seen.get(&name).is_some_and(|&rank| rank < COORDINATOR_RANK);
                format!("{}{name}", if pooled { 'p' } else { 'c' })
            })
            .collect();
        drop(seen);
        match self.bootstrap() {
            Some(bootstrap) => Self::create_namespaces(bootstrap.evaluator.as_ref(), &names),
            None => {
                self.namespaces_pending.borrow_mut().extend(names);
                Ok(())
            }
        }
    }
    /// Add `names` to `engine`'s registry, unlogged, each after `p` for one
    /// whose later touches are logged (`pooled`, see build.rs) or `c`.
    fn create_namespaces(engine: &dyn MacroEvaluator, names: &[String]) -> Result<(), String> {
        if names.is_empty() {
            return Ok(());
        }
        let mut source = String::from("rt => rt.compilerForms.createNamespaces?.([");
        for (index, name) in names.iter().enumerate() {
            if index > 0 {
                source.push(',');
            }
            crate::source_map::json_string(&mut source, name);
        }
        source.push_str("]) ?? null");
        let value = engine.evaluate_unit(&source).map_err(|e| e.to_string())?;
        engine.release(value).map_err(|e| e.to_string())
    }
    /// Like [`Self::take_namespaces_from`], caching the per-isolate JS wrapper
    /// in `cached` so repeated unit drains call the handle directly without
    /// recompiling a JS function string. Returns the raw registry vector
    /// without invoking user-overridable protocol methods like `ICounted`,
    /// propagates errors from a cached handle instead of re-draining a cleared
    /// log, and skips native-bridge evaluators that have no JS `takeNamespaces`
    /// hook (light-cljs-1102).
    pub(crate) fn take_namespaces_from_cached(
        engine: &dyn MacroEvaluator,
        pooled: bool,
        cached: &std::cell::Cell<Option<crate::evaluator::ValueId>>,
    ) -> Result<Vec<crate::namespace_artifact::RegistryEntry>, String> {
        let function = match cached.get() {
            Some(function) => function,
            None => {
                let source = match pooled {
                    true => "rt => () => rt.compilerForms.takeNamespaces?.(true) ?? null",
                    false => "rt => () => rt.compilerForms.takeNamespaces?.() ?? null",
                };
                let function = engine.evaluate_unit(source).map_err(|e| e.to_string())?;
                cached.set(Some(function));
                function
            }
        };
        let value = engine.call(function, &[]).map_err(|e| e.to_string())?;
        Self::export_namespaces(engine, value)
    }
    fn take_namespaces_from(
        engine: &dyn MacroEvaluator,
        pooled: bool,
    ) -> Result<Vec<crate::namespace_artifact::RegistryEntry>, String> {
        let value = engine
            .evaluate_unit(match pooled {
                true => "rt => rt.compilerForms.takeNamespaces?.(true) ?? null",
                false => "rt => rt.compilerForms.takeNamespaces?.() ?? null",
            })
            .map_err(|e| e.to_string())?;
        Self::export_namespaces(engine, value)
    }
    fn export_namespaces(
        engine: &dyn MacroEvaluator,
        value: crate::evaluator::ValueId,
    ) -> Result<Vec<crate::namespace_artifact::RegistryEntry>, String> {
        let source = crate::compiler_data::Source::new("<namespaces>", "");
        let span = source.span(0..0).expect("an empty span");
        let call = crate::compiler_data::Form::from_data(
            Datum::new(DatumKind::Nil),
            span.clone(),
            crate::compiler_data::Origin::SuppliedData,
        );
        let origin = crate::compiler_data::ExpansionOrigin {
            macro_name: Name::new(None, "all-ns"),
            call: span,
            definition: None,
            helpers: Vec::new(),
        };
        let limits = crate::form_bridge::ConversionLimits {
            nodes: i32::MAX as usize,
            depth: 2,
        };
        let exported = engine.export_form(value, &call, origin, limits);
        engine.release(value).map_err(|e| e.to_string())?;
        let data = exported.map_err(|e| e.to_string())?.data();
        match data.kind() {
            DatumKind::Nil => Ok(Vec::new()),
            DatumKind::Collection { children, .. } => children
                .iter()
                .map(|name| {
                    let DatumKind::String(text) = name.kind() else {
                        return Err("a registered namespace is not a string".to_owned());
                    };
                    let text = String::from_utf16(text)
                        .map_err(|_| "a registered namespace is not text".to_owned())?;
                    let registered = match text.get(..1) {
                        Some("r") => true,
                        Some("c") => false,
                        _ => return Err(format!("unknown registry log entry {text:?}")),
                    };
                    Ok(crate::namespace_artifact::RegistryEntry {
                        namespace: text[1..].to_owned(),
                        registered,
                        unit: None,
                    })
                })
                .collect(),
            _ => Err("registered namespaces are not a vector".into()),
        }
    }
    /// Whether this session is a parallel namespace worker's private overlay
    /// (Stage C, light-cljs-1090).
    pub(crate) fn is_local_overlay(&self) -> bool {
        self.local_overlay.borrow().is_some()
    }
    /// Create a worker-thread session with a private [`NamespaceOverlay`] layered
    /// strictly over `(shared, prefix_cursor)`.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new_namespace_overlay(
        shared: Arc<SharedCatalog>,
        thread: usize,
        bootstrap: Option<Bootstrap>,
        prefix_cursor: usize,
        prefix_bundles: usize,
        prefix_env_bundles: usize,
        pool_policy: PoolPolicy,
        capturing: bool,
        overlay_synced: bool,
    ) -> Rc<Self> {
        let engine = RefCell::new(InstalledRegistrations {
            bootstrap,
            installed: prefix_cursor,
            synced_bundles: prefix_bundles,
            synced_ext_protocols: 0,
        });
        Rc::new(Self {
            shared,
            thread,
            engine,
            unit_bindings: Default::default(),
            capture: Default::default(),
            gensyms_pending: Default::default(),
            gensym_floor: Default::default(),
            gensyms_logged: Default::default(),
            gensym_buffer: Default::default(),
            gensyms_seen: Default::default(),
            namespaces_pending: Default::default(),
            namespaces_logged: Default::default(),
            namespaces_ensured: Default::default(),
            namespaces_absorbed: Default::default(),
            namespaces_seen: Default::default(),
            pool: Default::default(),
            #[cfg(light_v8)]
            v8_pool_isolates: Default::default(),
            #[cfg(light_v8)]
            v8_pool_elided: Default::default(),
            drained_through: Default::default(),
            deferred_engine_namespaces: Default::default(),
            local_overlay: RefCell::new(Some(NamespaceOverlay {
                prefix_cursor,
                prefix_bundles,
                prefix_env_bundles,
                units: Vec::new(),
                registrations: Vec::new(),
                roots: RootIndex::default(),
                multifn_targets: HashMap::new(),
                local_env_bundles: 0,
                local_redefined: false,
                pool_policy,
                capturing,
                captured_macros: Vec::new(),
                touched_local_in_engine: false,
                reflected_in_engine: false,
                created_namespaces: Vec::new(),
                overlay_synced,
                synced_local_ext: 0,
                synced_local_cursor: prefix_cursor,
            })),
        })
    }
    /// Clear any temporary V8 overlay window installed during a parallel
    /// top-level pass so the persistent worker isolate remains pristine for
    /// reuse whenever no local root/bundle was actually materialized in V8.
    pub(crate) fn clear_engine_overlay(&self) -> Result<(), String> {
        let should_clear = self
            .local_overlay
            .borrow()
            .as_ref()
            .is_some_and(|o| o.overlay_synced && !o.touched_local_in_engine);
        if !should_clear {
            return Ok(());
        }
        if let Some(bootstrap) = self.bootstrap() {
            let tag = ["\0clear".to_owned()];
            bootstrap
                .evaluator
                .sync_catalog(0, 0, &tag)
                .map_err(|e| e.to_string())?;
        }
        if let Some(overlay) = self.local_overlay.borrow_mut().as_mut() {
            overlay.overlay_synced = false;
        }
        Ok(())
    }
    /// Exact analysis diagnostic recorded when a local overlay unit failed.
    pub(crate) fn local_diagnostic(&self, id: usize) -> Option<crate::AnalysisError> {
        let local_idx = id.checked_sub(LOCAL_UNIT_BASE)?;
        let overlay = self.local_overlay.borrow();
        match &overlay.as_ref()?.units.get(local_idx)?.state {
            AnalysisState::Failed(_, diagnostic) => diagnostic.clone(),
            _ => None,
        }
    }
    /// Extract the completed overlay for canonical publication on the coordinator.
    pub(crate) fn take_namespace_overlay(&self) -> NamespaceOverlayOutput {
        let overlay = self
            .local_overlay
            .borrow_mut()
            .take()
            .expect("namespace overlay session");
        NamespaceOverlayOutput {
            units: overlay.units,
            registrations: overlay.registrations,
            captured_macros: overlay.captured_macros,
            touched_local_in_engine: overlay.touched_local_in_engine,
            reflected_in_engine: overlay.reflected_in_engine,
            created_namespaces: overlay.created_namespaces,
        }
    }
    /// Answer a `__lightRootService` lookup against `(shared < prefix_cursor)`
    /// concatenated with this worker's private overlay registrations.
    fn serve_overlay_root_request(&self, request: &str) -> String {
        use std::fmt::Write;
        let mut parts = request.split('\0');
        let Some(kind) = parts.next() else {
            return String::new();
        };
        let mut overlay_ref = self.local_overlay.borrow_mut();
        let Some(overlay) = overlay_ref.as_mut() else {
            return String::new();
        };
        let prefix_cursor = overlay.prefix_cursor;
        let prefix_bundles = overlay.prefix_bundles;
        let total_pos = prefix_cursor + overlay.registrations.len();
        match kind {
            "n" => {
                let (Some(name), Some(from), Some(limit)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return String::new();
                };
                let (Ok(from), Ok(limit)) = (from.parse::<usize>(), limit.parse::<isize>()) else {
                    return String::new();
                };
                let to_pos = if limit < 0 {
                    total_pos
                } else {
                    (limit as usize).min(total_pos)
                };
                if from >= to_pos {
                    return String::new();
                }
                let mut entries_buf = String::new();
                let mut count = 0usize;
                if from < prefix_cursor {
                    let shared_to = to_pos.min(prefix_cursor);
                    let c = self.shared.lock();
                    if let Some(positions) = c.roots.name_positions.get(name) {
                        let start = positions.partition_point(|&pos| pos < from);
                        let end = positions.partition_point(|&pos| pos < shared_to);
                        for &pos in &positions[start..end] {
                            if count > 0 {
                                entries_buf.push(',');
                            }
                            count += 1;
                            let decl_frontier = c.roots.bundle_count_at[pos];
                            match &c.journal.registrations[pos] {
                                Registration::Root(_, unit, test, once, location) => {
                                    let _ = write!(entries_buf, "[0,{pos},");
                                    write_opt_usize(&mut entries_buf, *unit);
                                    entries_buf.push(',');
                                    write_opt_usize(&mut entries_buf, *test);
                                    let _ = write!(entries_buf, ",{once},");
                                    crate::source_map::json_string(&mut entries_buf, location);
                                    let _ = write!(entries_buf, ",{decl_frontier}]");
                                }
                                Registration::Bundle(bundle) => {
                                    entries_buf.push_str("[1,");
                                    write_bundle_fields(
                                        &mut entries_buf,
                                        pos,
                                        decl_frontier + 1,
                                        bundle,
                                    );
                                    entries_buf.push(']');
                                }
                            }
                        }
                    }
                }
                if to_pos > prefix_cursor {
                    let local_from = from.saturating_sub(prefix_cursor);
                    let local_to = to_pos - prefix_cursor;
                    if let Some(positions) = overlay.roots.name_positions.get(name) {
                        let start = positions.partition_point(|&pos| pos < local_from);
                        let end = positions.partition_point(|&pos| pos < local_to);
                        if start < end {
                            overlay.touched_local_in_engine = true;
                        }
                        for &local_pos in &positions[start..end] {
                            if count > 0 {
                                entries_buf.push(',');
                            }
                            count += 1;
                            let pos = prefix_cursor + local_pos;
                            let decl_frontier =
                                prefix_bundles + overlay.roots.bundle_count_at[local_pos];
                            match &overlay.registrations[local_pos] {
                                Registration::Root(_, unit, test, once, location) => {
                                    let _ = write!(entries_buf, "[0,{pos},");
                                    write_opt_usize(&mut entries_buf, *unit);
                                    entries_buf.push(',');
                                    write_opt_usize(&mut entries_buf, *test);
                                    let _ = write!(entries_buf, ",{once},");
                                    crate::source_map::json_string(&mut entries_buf, location);
                                    let _ = write!(entries_buf, ",{decl_frontier}]");
                                }
                                Registration::Bundle(bundle) => {
                                    entries_buf.push_str("[1,");
                                    write_bundle_fields(
                                        &mut entries_buf,
                                        pos,
                                        decl_frontier + 1,
                                        bundle,
                                    );
                                    entries_buf.push(']');
                                }
                            }
                        }
                    }
                }
                if count == 0 {
                    return String::new();
                }
                format!("[{to_pos},[{entries_buf}]]")
            }
            "p" => {
                let (Some(protocol), Some(from), Some(to)) =
                    (parts.next(), parts.next(), parts.next())
                else {
                    return String::new();
                };
                let (Ok(from), Ok(to)) = (from.parse::<usize>(), to.parse::<usize>()) else {
                    return String::new();
                };
                let to_pos = to.min(total_pos);
                if from >= to_pos {
                    return String::new();
                }
                let mut entries_buf = String::new();
                let mut count = 0usize;
                if from < prefix_cursor {
                    let shared_to = to_pos.min(prefix_cursor);
                    let c = self.shared.lock();
                    if let Some(orders) = c.roots.protocol_bundles.get(protocol) {
                        let start =
                            orders.partition_point(|&order| c.roots.bundles[order - 1].0 < from);
                        let end = orders
                            .partition_point(|&order| c.roots.bundles[order - 1].0 < shared_to);
                        for &order in &orders[start..end] {
                            if count > 0 {
                                entries_buf.push(',');
                            }
                            count += 1;
                            let (pos, bundle) = &c.roots.bundles[order - 1];
                            entries_buf.push('[');
                            write_bundle_fields(&mut entries_buf, *pos, order, bundle);
                            entries_buf.push(']');
                        }
                    }
                }
                if to_pos > prefix_cursor {
                    let local_from = from.saturating_sub(prefix_cursor);
                    let local_to = to_pos - prefix_cursor;
                    if let Some(orders) = overlay.roots.protocol_bundles.get(protocol) {
                        let start = orders.partition_point(|&order| {
                            overlay.roots.bundles[order - 1].0 < local_from
                        });
                        let end = orders.partition_point(|&order| {
                            overlay.roots.bundles[order - 1].0 < local_to
                        });
                        if start < end {
                            overlay.touched_local_in_engine = true;
                        }
                        for &local_order in &orders[start..end] {
                            if count > 0 {
                                entries_buf.push(',');
                            }
                            count += 1;
                            let (local_pos, bundle) = &overlay.roots.bundles[local_order - 1];
                            entries_buf.push('[');
                            write_bundle_fields(
                                &mut entries_buf,
                                prefix_cursor + *local_pos,
                                prefix_bundles + local_order,
                                bundle,
                            );
                            entries_buf.push(']');
                        }
                    }
                }
                if count == 0 {
                    return String::new();
                }
                format!("[{entries_buf}]")
            }
            "t" => {
                let Some(unit) = parts.next().and_then(|s| s.parse::<usize>().ok()) else {
                    return String::new();
                };
                if unit >= LOCAL_UNIT_BASE {
                    let Some(&local_pos) = overlay.roots.unit_positions.get(&unit) else {
                        return String::new();
                    };
                    overlay.touched_local_in_engine = true;
                    if let Registration::Root(name, ..) = &overlay.registrations[local_pos] {
                        let mut out = String::new();
                        let _ = write!(out, "[{},", prefix_cursor + local_pos);
                        crate::source_map::json_string(&mut out, name);
                        out.push(']');
                        return out;
                    }
                    String::new()
                } else {
                    drop(overlay_ref);
                    self.shared.serve_root_request(request, Some(prefix_cursor))
                }
            }
            "r" => {
                overlay.reflected_in_engine = true;
                String::new()
            }
            "c" => {
                if let Some(ns) = parts.next()
                    && !overlay
                        .created_namespaces
                        .iter()
                        .any(|existing| existing == ns)
                {
                    overlay.created_namespaces.push(ns.to_owned());
                }
                String::new()
            }
            _ => String::new(),
        }
    }
    /// Analyze a local unit registered in this worker's private overlay
    /// (used for `warning_mode` form-by-form draining and rare intra-namespace
    /// compile-time demands). Prepares compile-time JS right on the worker.
    pub(crate) fn analyze_local(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
        worker_frame: &EngineBinding,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        let local_idx = id - LOCAL_UNIT_BASE;
        let inputs = {
            let shared_depth = self.shared.lock().threads[self.thread].stack.len();
            let mut overlay_ref = self.local_overlay.borrow_mut();
            let overlay = overlay_ref.as_mut().expect("namespace overlay");
            let local_depth = overlay
                .units
                .iter()
                .filter(|u| matches!(u.state, AnalysisState::Analyzing(_)))
                .count();
            let depth = shared_depth + local_depth;
            let unit = &mut overlay.units[local_idx];
            match &unit.state {
                AnalysisState::Analyzed(program, warnings, _) => {
                    return Ok((program.clone(), warnings.clone()));
                }
                AnalysisState::Prepared(program, warnings, _) => {
                    return program
                        .upgrade()
                        .map(|p| (p, warnings.clone()))
                        .ok_or_else(|| "application IR was already transferred".to_owned());
                }
                AnalysisState::Failed(error, _) => return Err(error.clone()),
                AnalysisState::Analyzing(_) => {
                    return Err(format!(
                        "compile dependency cycle: {}",
                        unit.raw_name.clone().resolve(id)
                    ));
                }
                AnalysisState::Unstarted => {
                    let inputs = unit.inputs.take().expect("unstarted local unit inputs");
                    crate::macro_host::WorkMarks::note(&worker_frame.work.unit_depth, depth + 1);
                    if depth >= inputs.environment.limits().expansion_depth.min(64) {
                        unit.state = AnalysisState::Failed(DEPTH_LIMIT.to_owned(), None);
                        return Err(DEPTH_LIMIT.to_owned());
                    }
                    unit.state = AnalysisState::Analyzing(self.thread);
                    unit.analyses += 1;
                    inputs
                }
            }
        };
        let UnitInputs {
            form,
            snapshot,
            environment,
        } = *inputs;
        // A `:test` unit analyzed inline inside an overlay (`warning_mode`)
        // embeds `LOCAL_UNIT_BASE` inside its prepared JS and fragment IR;
        // fall back cleanly to the coordinator so canonical unit IDs exist
        // before analysis.
        if snapshot.definition_test_unit.is_some()
            && let Some(overlay) = self.local_overlay.borrow_mut().as_mut()
        {
            overlay.reflected_in_engine = true;
        }
        let mut host =
            MacroHost::from_parts(environment, worker_frame.frame()).macro_analysis_frame();
        if host.engine.bootstrap.is_none() {
            host.engine.bootstrap = self.bootstrap();
        }
        let evaluator = host.engine.bootstrap.as_ref().map(|b| b.evaluator.clone());
        let active_engine = engine.or_else(|| {
            evaluator
                .as_ref()
                .map(|e| e.as_ref() as &dyn MacroEvaluator)
        });
        let scope_engine = if self
            .local_overlay
            .borrow()
            .as_ref()
            .is_some_and(|o| o.units[local_idx].macro_free)
        {
            None
        } else {
            active_engine
        };
        let _frontier = if let Some(engine) = scope_engine {
            engine
                .compiler_frontier(Some(snapshot.extension_frontier))
                .map_err(|e| e.to_string())?;
            Some(FrontierGuard { engine })
        } else {
            None
        };
        let _gensyms = crate::generated_names::RestoreGensymScope(scope_engine);
        let analysis =
            crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, active_engine);
        let diagnostic = analysis.as_ref().err().cloned();
        let result = analysis
            .map(|(mut p, w)| {
                p.shrink_to_fit();
                (Arc::new(p), w)
            })
            .map_err(|e| {
                let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
                let name = self.local_overlay.borrow().as_ref().unwrap().units[local_idx]
                    .raw_name
                    .clone()
                    .resolve(id);
                format!("{}:{}:{}: {name}: {e}", loc.source, loc.line, loc.column)
            });
        let prepared = match &result {
            Ok((p, _)) => Some(Arc::new(crate::emitter::DeferredUnit::prepare(p))),
            Err(_) => None,
        };
        let mut overlay_ref = self.local_overlay.borrow_mut();
        let unit = &mut overlay_ref.as_mut().unwrap().units[local_idx];
        unit.state = match (&result, prepared) {
            (Ok((p, w)), Some(prepared)) => {
                AnalysisState::Prepared(Arc::downgrade(p), w.clone(), prepared)
            }
            (Ok((p, w)), None) => AnalysisState::Analyzed(p.clone(), w.clone(), None),
            (Err(e), _) => AnalysisState::Failed(e.clone(), diagnostic),
        };
        result
    }
    /// Compile a local unit (`id >= LOCAL_UNIT_BASE`) demanded by a macro running
    /// inside this worker's namespace top-level pass.
    fn compile_local_unit(
        &self,
        active: &dyn MacroEvaluator,
        id: usize,
        worker_frame: &EngineBinding,
    ) -> Result<String, String> {
        let local_idx = id - LOCAL_UNIT_BASE;
        if let Some(overlay) = self.local_overlay.borrow_mut().as_mut() {
            overlay.touched_local_in_engine = true;
        }
        let prepared = {
            let overlay_ref = self.local_overlay.borrow();
            let unit = &overlay_ref.as_ref().unwrap().units[local_idx];
            match &unit.state {
                AnalysisState::Prepared(_, _, p) | AnalysisState::Analyzed(_, _, Some(p)) => {
                    Some(p.clone())
                }
                _ => None,
            }
        };
        let register = |tables: &crate::reader_registration::ReaderTables| {
            crate::reader_registration::register(active, tables).map_err(|e| e.to_string())
        };
        crate::reader_registration::in_callback(active, || {
            if let Some(prepared) = prepared {
                self.local_overlay.borrow_mut().as_mut().unwrap().units[local_idx].emitted = true;
                register(prepared.tables())?;
                return Ok(prepared.code().to_owned());
            }
            let (program, _) = self.analyze_local(id, Some(active), worker_frame)?;
            let code = crate::emitter::emit_deferred_unit(&program);
            self.local_overlay.borrow_mut().as_mut().unwrap().units[local_idx].emitted = true;
            register(&crate::reader_registration::ReaderTables::of(&program))?;
            Ok(code)
        })
    }
    /// Commit a worker's [`NamespaceOverlayOutput`] into this coordinator session
    /// in canonical order under a single catalog lock, relocating all local unit
    /// IDs (`>= LOCAL_UNIT_BASE`) to canonical IDs `base .. base + units.len()`.
    /// Returns `(base_unit_id, pooled_pairs)` ready for pool submission.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn publish_overlay(
        &self,
        output: NamespaceOverlayOutput,
        host: &MacroHost,
        dispatched_local_ids: &[usize],
        can_pool: bool,
        snap_globals_len: usize,
        canonical_globals_len: usize,
    ) -> Result<(usize, Vec<(usize, bool)>), String> {
        let relocate = |id: usize, base: usize| -> usize {
            if id >= LOCAL_UNIT_BASE {
                base + (id - LOCAL_UNIT_BASE)
            } else {
                id
            }
        };
        let (base, pooled_pairs, needs_sync, unit_count) = {
            use std::sync::atomic::Ordering::Release;
            let mut c = self.shared.lock();
            let c = &mut *c;
            let base = c.units.len();
            let publish_bundles = c.journal.bundle_count;
            let unit_count = output.units.len();
            let mut capture_units = self.capture.borrow_mut();
            let mut resolved_names = Vec::with_capacity(unit_count);
            for (offset, mut local) in output.units.into_iter().enumerate() {
                let id = base + offset;
                let name = local.raw_name.resolve(id);
                resolved_names.push(name.clone());
                let next_var = VarId(c.bindings.len());
                let binding = c.bindings.entry(name.clone()).or_insert((next_var, 0));
                binding.1 += 1;
                let request = UnitRequest {
                    var: binding.0,
                    generation: binding.1,
                };
                let canonical_frontier = publish_bundles + local.local_bundle_offset;
                if let Some(inputs) = local.inputs.as_mut() {
                    inputs.snapshot.extension_frontier = canonical_frontier;
                    if let Some(test_id) = inputs.snapshot.definition_test_unit.as_mut() {
                        *test_id = relocate(*test_id, base);
                    }
                }
                if let Some(capture) = capture_units.as_mut() {
                    let own_globals = local.view_globals.saturating_sub(snap_globals_len);
                    capture.units.push(UnitCapture {
                        view_globals: canonical_globals_len + own_globals,
                        fragment_globals: local.view_globals,
                        extension_frontier: canonical_frontier,
                        added: Vec::new(),
                    });
                }
                c.units.push(Unit {
                    request,
                    name,
                    source: local.source,
                    inputs: local.inputs,
                    state: local.state,
                    analyses: local.analyses,
                    emitted: local.emitted,
                    waiters: 0,
                    retained: None,
                    macro_free: local.macro_free,
                    pooled: local.pooled,
                    dispatch: local.dispatch,
                    held: false,
                    dispatched: false,
                    charges: None,
                    time: Default::default(),
                });
            }
            drop(capture_units);
            let mut needs_sync = false;
            for reg in output.registrations {
                let canonical_reg = match reg {
                    Registration::Root(name, unit, test, once, location) => {
                        let unit = unit.map(|u| relocate(u, base));
                        let test = test.map(|t| relocate(t, base));
                        // Preserve synthetic unit names resolved at canonical IDs.
                        let canonical_name = match unit {
                            Some(u) if u >= base && u < base + unit_count => {
                                resolved_names[u - base].clone()
                            }
                            _ => name,
                        };
                        Registration::Root(canonical_name, unit, test, once, location)
                    }
                    Registration::Bundle(bundle) => {
                        let canonical_bundle = Arc::new(BundleRecord {
                            products: bundle.products.clone(),
                            unit: relocate(bundle.unit, base),
                            extensions: bundle.extensions.clone(),
                            location: bundle.location.clone(),
                            once: bundle.once,
                            group: bundle.group,
                        });
                        Registration::Bundle(canonical_bundle)
                    }
                };
                let position = c.journal.registrations.len();
                let effect = c.roots.record(position, &canonical_reg);
                c.journal.push(canonical_reg);
                self.shared.journal_len.store(effect.journal_len, Release);
                self.shared.bundle_count.store(effect.bundle_count, Release);
                self.shared
                    .ext_protocol_count
                    .store(effect.ext_protocol_count, Release);
                if effect.redefined || effect.has_extensions {
                    needs_sync = true;
                }
            }
            let mut pooled_pairs = Vec::new();
            for &local_id in dispatched_local_ids {
                let id = relocate(local_id, base);
                if let Some(unit) = c.units.get_mut(id) {
                    unit.held = true;
                    unit.dispatched = true;
                    if can_pool && unit.pooled && matches!(unit.state, AnalysisState::Unstarted) {
                        pooled_pairs.push((id, unit.macro_free));
                    }
                }
            }
            (base, pooled_pairs, needs_sync, unit_count)
        };
        if unit_count > 0 {
            let mut bindings = self.unit_bindings.borrow_mut();
            if bindings.len() < base + unit_count {
                bindings.resize_with(base + unit_count, || None);
            }
            for id in base..(base + unit_count) {
                bindings[id] = Some(Box::new(host.engine.frame()));
            }
        }
        if !output.created_namespaces.is_empty() {
            self.deferred_engine_namespaces
                .borrow_mut()
                .extend(output.created_namespaces);
        }
        if !output.captured_macros.is_empty()
            && let Some(capture) = self.capture.borrow_mut().as_mut()
        {
            for mut captured in output.captured_macros {
                let own_globals = captured.view_globals.saturating_sub(snap_globals_len);
                captured.view_globals = canonical_globals_len + own_globals;
                capture.macros.push(captured);
            }
        }
        if needs_sync {
            self.sync_catalog(None)?;
        }
        Ok((base, pooled_pairs))
    }
    /// Submit a parallel namespace top-level task to the build's unit pool.
    pub(crate) fn submit_namespace(
        &self,
        source_index: usize,
        needs_engine: bool,
        run: NamespaceJob,
    ) -> bool {
        match &*self.pool.borrow() {
            Some(pool) => {
                pool.submit_namespace(source_index, needs_engine, run);
                true
            }
            None => false,
        }
    }
    /// Block the coordinator on `shared.settled` until `ready()` returns true,
    /// servicing any forwarded `light.build` requests from workers along the
    /// way so workers never deadlock waiting on the coordinator.
    pub(crate) fn wait_for_namespace_ready(
        &self,
        host: &MacroHost,
        mut ready: impl FnMut() -> bool,
    ) -> Result<(), String> {
        loop {
            if ready() {
                return Ok(());
            }
            // Service any pending worker `light.build` requests first.
            let request = {
                let mut c = self.shared.lock();
                if let Some(abandoned) = &c.abandoned {
                    return Err(abandoned.clone());
                }
                c.requests.pop_front()
            };
            if let Some(request) = request {
                let log = host.engine.dependency_log.borrow().as_ref().map(|log| {
                    crate::dependency_manifest::ObservationLog {
                        window: request.analysis.first().copied().unwrap_or(log.window),
                        ..Default::default()
                    }
                });
                let log = Rc::new(RefCell::new(log));
                let reply = crate::build_services::serve_request(
                    &host.engine.build,
                    &log,
                    &request.text,
                    |unit| self.shared.unit_source(unit),
                    request.analysis,
                );
                self.shared
                    .lock()
                    .replies
                    .push((request.ticket, reply, log.take().map(Box::new)));
                self.shared.settled.notify_all();
                continue;
            }
            let guard = self.shared.lock();
            if ready() {
                return Ok(());
            }
            if let Some(abandoned) = &guard.abandoned {
                return Err(abandoned.clone());
            }
            if !guard.requests.is_empty() {
                continue;
            }
            let _ = self
                .shared
                .settled
                .wait_timeout(guard, std::time::Duration::from_millis(5))
                .unwrap_or_else(PoisonError::into_inner);
        }
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
        // The namespaces installed artifacts added to the registry, and the
        // log of what analyses add to it.
        let pending = std::mem::take(&mut *self.namespaces_pending.borrow_mut());
        Self::create_namespaces(evaluator.as_ref(), &pending)?;
        if self.namespaces_logged.get() {
            Self::take_namespaces_from(evaluator.as_ref(), false)?;
        }
        self.engine.borrow_mut().bootstrap = Some(bootstrap.clone());
        self.sync_catalog(None)
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
    /// Advance the failed-build count cutoff at a namespace drain point.
    /// Warning-mode units are prepared during the top-level pass and count
    /// when their namespace merges them, without [`Self::analyze_drained`].
    /// Other merges, such as a speculative definition's `:test` metadata,
    /// must not advance the cutoff past an earlier failing unit.
    pub(crate) fn note_drained(&self, id: usize) {
        let last = self.drained_through.get().map_or(id, |last| last.max(id));
        self.drained_through.set(Some(last));
    }
    /// How many units this compilation registered, analyzed, prepared and
    /// executed (`--stats`).
    pub fn unit_counts(&self) -> crate::compile_stats::DeferredUnitCounts {
        self.counts_through(Some(usize::MAX))
    }
    /// [`Self::unit_counts`] of a build that failed before its final
    /// analysis: the analyses only of the units up to the last one a drain
    /// point took, in unit order. Pool workers may have analyzed later
    /// units before the failure cancelled them, and one slot analyzes the
    /// units its dispatch keeps ahead of their drain points, so analyses
    /// after that unit depend on scheduling; up to it every unit was
    /// drained, under any schedule. Units drained early in warning mode
    /// count when their namespace merges them. A failed build's namespaces
    /// and warnings are cut at its failure the same way.
    pub fn failed_unit_counts(&self) -> crate::compile_stats::DeferredUnitCounts {
        self.counts_through(self.drained_through.get())
    }
    /// The unit counts, with the analyses and preparations of the units
    /// through `end`, or none when no unit has been drained.
    fn counts_through(&self, end: Option<usize>) -> crate::compile_stats::DeferredUnitCounts {
        let catalog = self.shared.lock();
        let mut counts = crate::compile_stats::DeferredUnitCounts {
            registered: catalog.units.len(),
            ..Default::default()
        };
        for (id, unit) in catalog.units.iter().enumerate() {
            counts.macro_free += usize::from(unit.macro_free);
            if unit.macro_free {
                counts.macro_free_time += unit.time;
            }
            let drained = end.is_some_and(|end| id <= end);
            counts.analyzed += usize::from(drained && unit.analyses > 0);
            counts.prepared +=
                usize::from(drained && matches!(unit.state, AnalysisState::Prepared(..)));
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
        counts.pool_engine_participants = pool
            .engine_participants
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len();
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
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            return overlay.prefix_bundles + overlay.roots.bundles.len();
        }
        self.with_journal(|journal| journal.bundle_count)
    }
    /// How many registered bundles [`reaches_environment_data`].
    pub fn environment_bundles(&self) -> usize {
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            return overlay.prefix_env_bundles + overlay.local_env_bundles;
        }
        self.with_journal(|journal| journal.environment_bundles)
    }
    pub fn multifn_target(&self, name: &str) -> Option<String> {
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            if let Some(target) = overlay.multifn_targets.get(name) {
                return Some(target.clone());
            }
            return self.shared.multifn_target(name);
        }
        self.with_journal(|journal| journal.multifn_targets.get(name).cloned())
    }
    pub fn register_bundle(
        &self,
        program: &Program,
        registration: BundleRegistration<'_>,
    ) -> Result<usize, String> {
        self.register_bundle_with_flags(program, registration, false, false)
    }
    pub(crate) fn register_bundle_with_flags(
        &self,
        program: &Program,
        registration: BundleRegistration<'_>,
        macro_free: bool,
        pooled: bool,
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
        let unit = self
            .register_named(
                RegistrationName::Declaration,
                Some(form),
                snapshot,
                host,
                false,
                location.clone(),
                macro_free,
                pooled,
            )?
            .unwrap();
        let group = analyzed.is_some();
        if self.local_overlay.borrow().is_some() {
            let local_idx = unit - LOCAL_UNIT_BASE;
            if let Some(mut program) = analyzed {
                program.shrink_to_fit();
                let prepared = Arc::new(crate::emitter::DeferredUnit::prepare(&program));
                let mut overlay_ref = self.local_overlay.borrow_mut();
                let entry = &mut overlay_ref.as_mut().unwrap().units[local_idx];
                entry.state = AnalysisState::Prepared(std::sync::Weak::new(), Vec::new(), prepared);
                entry.analyses = 1;
                entry.inputs = None;
            }
            let bundle = Arc::new(BundleRecord {
                products: names,
                unit,
                extensions: extensions.to_vec(),
                location,
                once,
                group,
            });
            let mut overlay_ref = self.local_overlay.borrow_mut();
            let overlay = overlay_ref.as_mut().unwrap();
            let local_pos = overlay.registrations.len();
            if reaches_environment_data(&bundle.extensions) {
                overlay.local_env_bundles += 1;
            }
            if let Some((target, _)) = bundle
                .extensions
                .iter()
                .find_map(|(protocol, _, _)| protocol.strip_prefix("multifn:").map(|t| (t, ())))
                && target != "*"
                && bundle.products.len() == 1
            {
                overlay
                    .multifn_targets
                    .insert(bundle.products[0].clone(), target.to_owned());
            }
            let registration = Registration::Bundle(bundle);
            let effect = overlay.roots.record(local_pos, &registration);
            if effect.redefined {
                overlay.local_redefined = true;
            }
            overlay.registrations.push(registration);
            return Ok(unit);
        }
        if let Some(mut program) = analyzed {
            program.shrink_to_fit();
            let inputs = {
                let mut c = self.shared.lock();
                let entry = &mut c.units[unit];
                entry.state = AnalysisState::Analyzed(Arc::new(program), Vec::new(), None);
                entry.analyses = 1;
                entry.inputs.take()
            };
            let binding = self.take_unit_binding(unit);
            drop((inputs, binding));
        }
        let bundle = Arc::new(BundleRecord {
            products: names,
            unit,
            extensions: extensions.to_vec(),
            location,
            once,
            group,
        });
        let effect = {
            let mut c = self.shared.lock();
            self.record_registration(&mut c, Registration::Bundle(bundle))
        };
        if effect.has_extensions || effect.redefined {
            self.sync_catalog(None)?;
        }
        if group {
            self.release_application_ir(unit);
        }
        Ok(unit)
    }
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn register(
        &self,
        name: String,
        form: Option<Form>,
        snapshot: DefinitionSnapshot,
        host: MacroHost,
        once: bool,
        location: String,
    ) -> Result<Option<usize>, String> {
        self.register_named(
            RegistrationName::Var(name),
            form,
            snapshot,
            host,
            once,
            location,
            false,
            false,
        )
    }
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn register_with_flags(
        &self,
        name: String,
        form: Option<Form>,
        snapshot: DefinitionSnapshot,
        host: MacroHost,
        once: bool,
        location: String,
        macro_free: bool,
        pooled: bool,
    ) -> Result<Option<usize>, String> {
        self.register_named(
            RegistrationName::Var(name),
            form,
            snapshot,
            host,
            once,
            location,
            macro_free,
            pooled,
        )
    }
    #[allow(clippy::too_many_arguments)]
    fn register_named(
        &self,
        name: RegistrationName,
        form: Option<Form>,
        mut snapshot: DefinitionSnapshot,
        host: MacroHost,
        once: bool,
        location: String,
        macro_free: bool,
        pooled: bool,
    ) -> Result<Option<usize>, String> {
        // Executable metadata has its own source-site unit and root state.
        // Reflecting it cannot analyze or execute the ordinary initializer.
        if let Some(test) = snapshot.definition_test.take().filter(|_| form.is_some()) {
            let owner = name.clone().resolve(self.next_unit());
            snapshot.definition_test_unit = self.register_named(
                RegistrationName::Test(owner),
                Some(test),
                snapshot.clone(),
                host.macro_analysis_frame(),
                false,
                location.clone(),
                false,
                false,
            )?;
        }
        let test = snapshot.definition_test_unit;
        // Where a pool would analyze it: a `:test` form, and a definition
        // with one, on the coordinator (`NamespaceAnalysis::finish_def`).
        let dispatch = match (&name, test) {
            (RegistrationName::Declaration, _) => crate::namespace_artifact::UnitDispatch::Bundle,
            (RegistrationName::Test(_), _) | (_, Some(_)) => {
                crate::namespace_artifact::UnitDispatch::Coordinator
            }
            (RegistrationName::Var(_), None) => crate::namespace_artifact::UnitDispatch::Definition,
        };
        #[cfg(test)]
        if let Some(form) = &form {
            worker_tests::retain(self.next_unit(), form, &snapshot, &host);
        }
        if self.local_overlay.borrow().is_some() {
            let (environment, _binding) = host.into_parts();
            let mut overlay_ref = self.local_overlay.borrow_mut();
            let overlay = overlay_ref.as_mut().unwrap();
            let local_idx = overlay.units.len();
            let local_unit_id = LOCAL_UNIT_BASE + local_idx;
            let resolved_name = name.clone().resolve(local_unit_id);
            let id = form.map(|form| {
                let view_globals = snapshot.resolution_program.globals.len();
                let local_bundle_offset = snapshot
                    .extension_frontier
                    .saturating_sub(overlay.prefix_bundles);
                let source = snapshot.source_index as u32;
                overlay.units.push(LocalUnit {
                    raw_name: name.clone(),
                    source,
                    inputs: Some(Box::new(UnitInputs {
                        form,
                        snapshot,
                        environment,
                    })),
                    state: AnalysisState::Unstarted,
                    analyses: 0,
                    emitted: false,
                    macro_free,
                    pooled,
                    dispatch,
                    view_globals,
                    local_bundle_offset,
                });
                local_unit_id
            });
            let local_pos = overlay.registrations.len();
            let registration = Registration::Root(resolved_name, id, test, once, location);
            let effect = overlay.roots.record(local_pos, &registration);
            if effect.redefined {
                overlay.local_redefined = true;
            }
            overlay.registrations.push(registration);
            return Ok(id);
        }
        let (environment, binding) = host.into_parts();
        let (id, effect) = {
            let mut c = self.shared.lock();
            let c = &mut *c;
            // Allocate synthetic names under the same lock as their unit ID.
            // Reading units.len() before entering register races other writers.
            let name = name.resolve(c.units.len());
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
                    let view_globals = snapshot.resolution_program.globals.len();
                    capture.units.push(UnitCapture {
                        view_globals,
                        fragment_globals: view_globals,
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
                    macro_free,
                    pooled,
                    dispatch,
                    held: false,
                    dispatched: false,
                    charges: None,
                    time: Default::default(),
                });
                id
            });
            let effect =
                self.record_registration(c, Registration::Root(name, id, test, once, location));
            (id, effect)
        };
        if let Some(id) = id {
            let mut bindings = self.unit_bindings.borrow_mut();
            if bindings.len() <= id {
                bindings.resize_with(id + 1, || None);
            }
            bindings[id] = Some(Box::new(binding));
        }
        if effect.redefined {
            self.sync_catalog(None)?;
        }
        Ok(id)
    }
    /// Mark unit `id`, just registered, as macro-free: its form reaches no
    /// compile-time engine, so a pool worker may analyze it, and every
    /// analysis of it panics if it does reach one
    /// ([`crate::macro_host::MacroFreeUnit`]).
    #[allow(dead_code)]
    pub fn set_macro_free(&self, id: usize) {
        if id >= LOCAL_UNIT_BASE
            && let Some(overlay) = self.local_overlay.borrow_mut().as_mut()
        {
            overlay.units[id - LOCAL_UNIT_BASE].macro_free = true;
            return;
        }
        self.shared.lock().units[id].macro_free = true;
    }
    /// Let the build's unit pool analyze unit `id` ([`Self::submit_units`]).
    #[allow(dead_code)]
    pub(crate) fn set_pooled(&self, id: usize) {
        if id >= LOCAL_UNIT_BASE
            && let Some(overlay) = self.local_overlay.borrow_mut().as_mut()
        {
            overlay.units[id - LOCAL_UNIT_BASE].pooled = true;
            return;
        }
        self.shared.lock().units[id].pooled = true;
    }
    /// Of `units`, those for the pool that no thread has started, in order,
    /// paired with their `macro_free` flag so pool submission needs no lock.
    pub(crate) fn poolable(&self, units: impl Iterator<Item = usize>) -> Vec<(usize, bool)> {
        let c = self.shared.lock();
        units
            .filter_map(|id| {
                c.units
                    .get(id)
                    .filter(|u| u.pooled && matches!(u.state, AnalysisState::Unstarted))
                    .map(|u| (id, u.macro_free))
            })
            .collect()
    }
    /// Mark `units` held/dispatched and return the poolable `(id, macro_free)`
    /// subset in a single catalog lock acquisition.
    pub(crate) fn hold_and_poolable(
        &self,
        units: impl Iterator<Item = usize>,
        can_pool: bool,
    ) -> Vec<(usize, bool)> {
        let units: Vec<usize> = units.collect();
        let mut c = self.shared.lock();
        let mut pooled = if can_pool {
            Vec::with_capacity(units.len())
        } else {
            Vec::new()
        };
        for &id in &units {
            if let Some(unit) = c.units.get_mut(id) {
                unit.held = true;
                unit.dispatched = true;
                if can_pool && unit.pooled && matches!(unit.state, AnalysisState::Unstarted) {
                    pooled.push((id, unit.macro_free));
                }
            }
        }
        pooled
    }
    /// Perturb this build's unit pool and analyses, for a stress test.
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    pub(crate) fn perturb(&self, perturbation: crate::scheduler::Perturbation) {
        let _ = self.shared.perturbation.set(perturbation);
    }
    /// Note what the compilation records of each recorded analysis beyond
    /// its charges, with a pool or without: the units the coordinator keeps
    /// at a dispatch are recorded too (light-cljs-391).
    pub(crate) fn set_recording(&self, recording: crate::macro_host::UnitRecording) {
        let _ = self.shared.recording.set(recording);
    }
    #[cfg(light_v8)]
    pub(crate) fn plan_v8_pool(&self, isolates: usize) {
        self.v8_pool_isolates.set(isolates);
    }
    #[cfg(light_v8)]
    pub(crate) fn v8_isolate_count(&self) -> usize {
        self.v8_pool_isolates.get().max(1)
    }
    #[cfg(light_v8)]
    pub(crate) fn reserves_v8_worker_shares(&self) -> bool {
        !self.v8_pool_elided.get()
    }
    /// The build installed every artifact without starting the pool.
    /// Let the coordinator use its worker shares from here on.
    #[cfg(light_v8)]
    pub(crate) fn elide_v8_pool(&self) {
        debug_assert!(self.pool.borrow().is_none());
        if !self.v8_pool_elided.replace(true)
            && let Some(resources) = self.shared.resources()
        {
            resources.memory.release_unused_worker_shares();
        }
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
        recording: crate::macro_host::UnitRecording,
    ) -> Result<(), String> {
        #[cfg(light_v8)]
        if !self.reserves_v8_worker_shares() {
            return Err("cannot start the compiler unit pool after releasing its shares".into());
        }
        if engines != PoolEngines::None {
            self.shared
                .initialize_resources(environment.evaluator_options(), isolates, true)
                .map_err(|error| error.0)?;
        }
        let _ = self.shared.recording.set(recording);
        let pool = UnitPool::start(self.shared.clone(), slots, engines, isolates, environment)?;
        *self.pool.borrow_mut() = Some(pool);
        Ok(())
    }
    pub(crate) fn pool_policy(&self) -> Option<PoolPolicy> {
        self.pool.borrow().as_ref().map(|pool| PoolPolicy {
            engines: pool.engines,
            workers: pool.workers,
            engine_workers: pool.engine_workers,
        })
    }
    /// A `Send + Sync` handle for submitting pure-Rust CPU tasks onto the
    /// active build's [`UnitPool`], keeping background CPU work within `--jobs`.
    pub(crate) fn pool_task_submitter(&self) -> Option<PoolTaskSubmitter> {
        self.pool.borrow().as_ref().map(|pool| PoolTaskSubmitter {
            inbox: pool.inbox.clone(),
            submitter: pool.submitter.clone(),
            mixed: pool.mixed,
        })
    }
    /// Whether the build's unit pool takes a unit: with `macro_free`, one
    /// found macro-free; with `test`, one of a definition with `:test`
    /// metadata, analyzed with its definition through this session. Its
    /// workers without engines take only macro-free units. A cache
    /// blocker's consumer never asks (`NamespaceAnalysis::main_isolate`).
    pub(crate) fn pools(&self, macro_free: bool, test: bool) -> bool {
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            return match overlay.pool_policy.engines {
                PoolEngines::None => macro_free,
                _ => macro_free || !test,
            };
        }
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
        if !self.pools(macro_free, false) {
            return false;
        }
        if let Some(overlay) = self.local_overlay.borrow().as_ref() {
            return match macro_free {
                true => overlay.pool_policy.workers > 1,
                false => overlay.pool_policy.engine_workers > 1,
            };
        }
        self.pool
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
    pub(crate) fn submit_units(&self, units: Vec<(usize, bool)>, charged: (usize, usize)) -> bool {
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
    /// Clear the starting counts for recorded units between interactive
    /// operations, after any build workers have finished.
    pub(crate) fn reset_charged(&self) {
        debug_assert!(self.pool.borrow().is_none());
        for count in &self.shared.charged {
            count.store(0, std::sync::atomic::Ordering::Relaxed);
        }
    }
    /// Start the coordinator's engine for `host` at a namespace's dispatch,
    /// if it has none ([`MacroHost::start_session_engine`]).
    pub(crate) fn start_dispatch_engine(self: &Rc<Self>, host: &MacroHost) -> Result<(), String> {
        if self.bootstrap().is_some() {
            return Ok(());
        }
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
            pool.submitter.cancel();
            let mut inbox = pool.inbox.lock();
            inbox.closed = true;
            inbox.cancelled = true;
            drop(inbox);
            pool.inbox.ready.notify_all();
        }
        if let Some(resources) = self.shared.resources() {
            resources.workers.cancel();
        }
        // Nothing the pool still runs is ordered against an earlier unit.
    }
    /// Stop the build's unit pool and join its workers ([`UnitPool::finish`]).
    pub(crate) fn finish_pool(&self) {
        let pool = self.pool.borrow_mut().take();
        if let Some(pool) = pool {
            pool.finish();
        }
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
        let program = {
            let mut c = self.lock();
            let state = &mut c.units[id].state;
            match state {
                AnalysisState::Analyzed(_, _, Some(_)) => {
                    let AnalysisState::Analyzed(program, warnings, Some(prepared)) =
                        std::mem::replace(state, AnalysisState::Unstarted)
                    else {
                        unreachable!()
                    };
                    *state = AnalysisState::Prepared(Arc::downgrade(&program), warnings, prepared);
                    let unit = &mut c.units[id];
                    if unit.waiters > 0 {
                        unit.retained = Some(program);
                        return;
                    }
                    drop(c);
                    drop(program);
                    return;
                }
                AnalysisState::Analyzed(program, _, None) => program.clone(),
                _ => return,
            }
        };
        // Prepared outside the lock; the analyzed program is dropped by its
        // last holder, outside it too.
        let prepared = Arc::new(crate::emitter::DeferredUnit::prepare(&program));
        let previous = {
            let mut c = self.lock();
            let state = &mut c.units[id].state;
            match std::mem::replace(state, AnalysisState::Unstarted) {
                AnalysisState::Analyzed(program, warnings, _) => {
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
        let analysis = self.analyze_in(id, engine, false, Take::Hold).map(drop);
        // No pool job after it in unit order starts, as after a worker's.
        if analysis.is_err() {
            self.shared.note_job(id);
        }
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
    /// it (`queued`), waiting for it, or this thread's. Returns whether the
    /// unit's application IR was already transitioned to `Prepared` under the
    /// same catalog lock (avoiding a separate `release_application_ir` lock).
    pub(crate) fn analyze_drained(
        &self,
        id: usize,
        queued: bool,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>, bool), String> {
        // Single-lock fast path for a unit a worker already analyzed and
        // prepared: releases hold, swaps Analyzed -> Prepared, takes charges,
        // and reads stack depth & dispatch in one lock acquisition.
        if !crate::macro_host::MacroFreeUnit::active()
            && let Some((program, warnings, charges, depth, dispatch)) =
                self.shared.drain_ready(id, self.thread)
        {
            self.note_drained(id);
            let counters = self.take_unit_binding(id).map(|b| *b);
            if let (Some(binding), Some(charges)) = (counters, charges) {
                self.apply_drained_charges(
                    id,
                    binding,
                    *charges,
                    depth,
                    dispatch,
                    None,
                    Take::Drain,
                )?;
            }
            return Ok((program, warnings, true));
        }
        self.analyze_in(id, None, queued, Take::Drain)
            .map(|(program, warnings)| (program, warnings, false))
    }

    pub(crate) fn charge_namespace(
        &self,
        mut charges: UnitCharges,
        host: &MacroHost,
    ) -> Result<(), crate::AnalysisError> {
        let binding = &host.engine;
        let control = binding
            .bootstrap
            .as_ref()
            .or(self.bootstrap().as_ref())
            .map(|b| b.evaluator.control());
        let start = || {
            self.start_main_engine(binding)
                .map(|b| b.map(|b| b.evaluator.control()))
        };
        if let Some(rows) = charges.trace.take() {
            crate::compile_trace::replay(*rows, 0);
        }
        let before = (
            binding.expansions.get() + binding.exempt_expansions.get(),
            binding.generated_nodes.get() + binding.exempt_nodes.get(),
        );
        charges
            .charge(binding, None, control, &start, self)
            .map_err(|(_, diagnostic)| diagnostic)?;
        let after = (
            binding.expansions.get() + binding.exempt_expansions.get(),
            binding.generated_nodes.get() + binding.exempt_nodes.get(),
        );
        crate::compile_trace::charged(
            &binding.expansions,
            after.0.saturating_sub(before.0),
            after.1.saturating_sub(before.1),
        );
        let UnitCharges {
            observations,
            gensyms,
            namespaces,
            emitted,
            ..
        } = charges;
        for emitted_id in emitted {
            self.shared.set_emitted(emitted_id);
        }
        if let (Some(observations), Some(log)) =
            (observations, binding.dependency_log.borrow_mut().as_mut())
        {
            log.absorb(*observations);
        }
        self.absorb_gensyms(gensyms)
            .map_err(|error| crate::AnalysisError {
                span: 0..0,
                kind: crate::analyzer::AnalysisErrorKind::MacroExpansion(error),
            })?;
        if !namespaces.is_empty() && self.namespaces_logged.get() {
            self.note_namespaces(namespaces, None);
        }
        self.shared
            .raise_charged(binding.expansions.get(), binding.generated_nodes.get());
        Ok(())
    }
    /// Wait for unit `id`'s analysis, a worker's if the pool was given it
    /// (`queued`), and add what it observed to the dependency log now, at
    /// its namespace's settle point; its drain point charges the rest
    /// (light-cljs-394). An error is the unit's failure.
    pub(crate) fn observe_pooled(&self, id: usize, queued: bool) -> Result<(), String> {
        self.analyze_in(id, None, queued, Take::Observe).map(drop)
    }
    fn analyze_in(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
        queued: bool,
        take: Take,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        self.analyze_claimed_in(id, engine, queued, take, None)
    }
    fn analyze_claimed_in(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
        queued: bool,
        take: Take,
        claim: Option<Claim>,
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
                self.note_drained(id);
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
        if take == Take::Hold && claim.is_none() {
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
            claim,
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
                    let observer = self.shared.unit_source(id).unwrap_or(log.window);
                    log.absorb_for(observer, *observations);
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
            && let Some(charges) = self.shared.take_charges(id)
        {
            let (depth, dispatch) = {
                let c = self.shared.lock();
                (c.threads[self.thread].stack.len(), c.units[id].dispatch)
            };
            self.apply_drained_charges(id, binding, *charges, depth, dispatch, engine, take)?;
        }
        analysis
    }
    #[allow(clippy::too_many_arguments)]
    fn apply_drained_charges(
        &self,
        id: usize,
        binding: EngineBinding,
        mut charges: UnitCharges,
        depth: usize,
        dispatch: crate::namespace_artifact::UnitDispatch,
        engine: Option<&dyn MacroEvaluator>,
        take: Take,
    ) -> Result<(), String> {
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
        let before = (
            binding.expansions.get() + binding.exempt_expansions.get(),
            binding.generated_nodes.get() + binding.exempt_nodes.get(),
        );
        // A worker's unit that expanded a JavaScript macro would have
        // started the coordinator's engine with one slot, at that
        // expansion; it starts it there, so later polls are spent.
        let start = || {
            #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
            charges.charge(&binding, Some(depth), control, &start, self)
        {
            self.shared.fail(id, message.clone(), Some(diagnostic));
            return Err(message);
        }
        // The unit's trace frame counted these; the frame it is
        // charged in does not count them as its own.
        let after = (
            binding.expansions.get() + binding.exempt_expansions.get(),
            binding.generated_nodes.get() + binding.exempt_nodes.get(),
        );
        crate::compile_trace::charged(
            &binding.expansions,
            after.0.saturating_sub(before.0),
            after.1.saturating_sub(before.1),
        );
        let UnitCharges {
            observations,
            gensyms,
            namespaces,
            events,
            emitted,
            ..
        } = charges;
        for emitted_id in emitted {
            self.shared.set_emitted(emitted_id);
        }
        // What the drain point charged, for its namespace's artifact,
        // whose installation charges it there again.
        if take == Take::Drain {
            self.capture_charges(id, events);
        }
        if let (Some(observations), Some(log)) =
            (observations, binding.dependency_log.borrow_mut().as_mut())
        {
            let observer = self.shared.unit_source(id).unwrap_or(log.window);
            log.absorb_for(observer, *observations);
        }
        if let Err(error) = self.absorb_gensyms(gensyms) {
            self.shared.fail(id, error.clone(), None);
            return Err(error);
        }
        if !namespaces.is_empty()
            && let Err(error) = self.absorb_namespaces(namespaces, dispatch)
        {
            self.shared.fail(id, error.clone(), None);
            return Err(error);
        }
        (self.shared).raise_charged(binding.expansions.get(), binding.generated_nodes.get());
        Ok(())
    }
}
/// The rank of a placement, for the compile-time registry's log: the
/// coordinator analyzes a placement in every build where it analyzes one of
/// a lower rank. A definition's unit is on the coordinator without engines
/// in the pool, a declaration bundle also with one engine worker, and a
/// unit the coordinator keeps or a namespace's own analysis (`None`)
/// always ([`CompilationSession::pools`], `pools_bundle`).
fn placement_rank(unit: Option<crate::namespace_artifact::UnitDispatch>) -> u8 {
    use crate::namespace_artifact::UnitDispatch;
    match unit {
        Some(UnitDispatch::Definition) => 0,
        Some(UnitDispatch::Bundle) => 1,
        Some(UnitDispatch::Coordinator) | None => COORDINATOR_RANK,
    }
}

/// The [`placement_rank`] of what the coordinator always analyzes.
const COORDINATOR_RANK: u8 = 2;

/// Note that a placement of `rank` logged `namespace`: whether none of
/// that rank or a higher one had.
fn note_rank(seen: &mut std::collections::HashMap<String, u8>, namespace: &str, rank: u8) -> bool {
    match seen.get_mut(namespace) {
        Some(logged) if *logged >= rank => false,
        Some(logged) => {
            *logged = rank;
            true
        }
        None => {
            seen.insert(namespace.to_owned(), rank);
            true
        }
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
    claim: Option<Claim>,
    binding: impl FnOnce() -> (EngineBinding, Option<Bootstrap>),
) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
    let Demand {
        queued,
        on_worker,
        held,
        server,
        help,
    } = demand;
    let pooled_help = claim.is_some();
    let claim = match claim {
        Some(claim) => claim,
        None => shared.claim_help(thread, id, queued, server, help)?,
    };
    let (inputs, depth, name, macro_free) = match claim {
        Claim::Reuse(program, warnings) => return Ok((program, warnings)),
        Claim::TooDeep(inputs, depth) => {
            let (binding, _) = binding();
            crate::macro_host::WorkMarks::note(&binding.work.unit_depth, depth + 1);
            drop((inputs, binding));
            return Err(DEPTH_LIMIT.to_owned());
        }
        Claim::Analyze(inputs, depth, name, macro_free) => (inputs, depth, name, macro_free),
    };
    let deferred_rows = pooled_help
        .then(|| crate::compile_trace::defer_unit_rows(depth + HELP_DEPTH.get()))
        .flatten();
    // Its atom effects keep its `--jobs 1` place (light-cljs-480); declared
    // before the guard, so it ends after the unit settles.
    // From here on, however the analysis ends, even by unwinding, the unit
    // settles and its waiters wake.
    let mut guard = AnalysisGuard {
        shared,
        thread,
        id,
        state: None,
        held: 0,
        charges: None,
        note_failed: on_worker || held,
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
        host.engine.exempt_expansions = std::rc::Rc::new(std::cell::Cell::new(0));
        host.engine.exempt_nodes = std::rc::Rc::new(std::cell::Cell::new(0));
        if !records {
            host.reset_unit_work();
        }
    }
    let (analysis, charges) = {
        // Declared before the frontier guard, so the frontier's transitions
        // count in the unit's frame (`--stats`, [`crate::compile_trace`]).
        let trace_source = snapshot.source_index as u32;
        let _traced = crate::compile_trace::enter_unit(
            || crate::compile_trace::UnitInfo {
                id,
                source: trace_source,
                name: name.clone(),
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
        // A proven macro-free body cannot enter the compile-time engine on any
        // backend (`MacroFreeUnit` enforces this), just as it runs in the
        // pool's engine-free view. Its analysis cannot see or change the
        // engine's frontier or gensym scope. Keep the engine available to the
        // analyzer for control polls on native backends.
        let scope_engine = if macro_free { None } else { engine };
        let _frontier = if let Some(engine) = scope_engine {
            if !on_worker && let Some(session) = host.session_handle() {
                session.sync_catalog(Some(engine))?;
            }
            engine
                .compiler_frontier(Some(snapshot.extension_frontier))
                .map_err(|e| e.to_string())?;
            Some(FrontierGuard { engine })
        } else {
            None
        };
        // The unit's macros number in its scope; whatever demanded it, such
        // as a running macro calling it, numbers in its own again after.
        let _gensyms = crate::generated_names::RestoreGensymScope(scope_engine);
        if records {
            let started = crate::evaluator::Instant::now();
            host.reset_unit_work();
            let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
            let location = format!("{}:{}:{}: {name}", loc.source, loc.line, loc.column);
            let recording = shared.recording();
            // While artifacts are captured, a worker's engine logs what
            // `gensym` gives from here on, which the unit's analysis hands
            // the coordinator; what the engine gave before, setting itself
            // up or installing the catalog's registrations, is its own.
            // The log is this thread's: the engine logs into it, and so do
            // the core macros the unit expands natively, with or without an
            // engine (light-cljs-432). Engine-side gensym logging is enabled
            // lazily once per isolate on its first engine entry, and namespace
            // capture is gated on actual engine entry (light-cljs-1102).
            let logs_gensyms = on_worker && recording.capture && !nested;
            if logs_gensyms {
                crate::generated_names::start_log();
            }
            // And what the unit adds to its engine's registry, on any thread,
            // which the coordinator logs where it charges the unit: the
            // coordinator's log so far stays its own (light-cljs-1004).
            // A unit a pool may place logs, besides what it adds, what it
            // touches that only such units added (light-cljs-1006).
            let logs_namespaces = recording.capture && !nested && scope_engine.is_some();
            let session = (!on_worker).then(|| host.session_handle()).flatten();
            let captures_namespaces = logs_namespaces && (on_worker || session.is_some());
            let pooled = captures_namespaces
                && placement_rank(Some(shared.lock().units[id].dispatch)) < COORDINATOR_RANK;
            let pending_capture = (scope_engine.is_some() && (logs_gensyms || captures_namespaces))
                .then(|| {
                    crate::macro_host::PendingCaptureGuard::install(
                        host.engine.work_cache.clone(),
                        crate::macro_host::PendingUnitCapture {
                            on_worker,
                            logs_gensyms,
                            logs_namespaces: captures_namespaces,
                            pooled,
                            session: session.clone(),
                            gensym_error: false,
                            namespace_error: false,
                            entered: false,
                        },
                    )
                });
            // A unit recorded on its own inside another's recording, on any
            // thread, logs into a log of its own, if the thread logs, and
            // the other's goes on after it, so that its names are its own
            // charges' wherever it is analyzed; the engine logs into this
            // thread's log already (light-cljs-442).
            let nested_log = own.then(crate::generated_names::nest_log);
            // Coordinator help records polls for the drain just as a
            // worker does; ordinary dispatch-held work polls as it runs.
            let free = crate::macro_host::MacroFreeUnit::enter(
                name.clone(),
                macro_free,
                recording,
                !held || pooled_help,
            );
            let analysis =
                crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);
            let recorded = free.finish();
            let capture = pending_capture.and_then(|guard| guard.finish());
            let unit_entered = capture.as_ref().is_some_and(|c| c.entered);
            let started_log = if capture.as_ref().is_some_and(|c| c.gensym_error) {
                Err(String::new())
            } else {
                Ok(())
            };
            let started_namespaces = if capture.as_ref().is_some_and(|c| c.namespace_error) {
                Err(String::new())
            } else {
                Ok(())
            };
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
            let namespaces = match scope_engine {
                Some(engine) if captures_namespaces && unit_entered => {
                    let next_pooled = if on_worker { pooled } else { false };
                    match started_namespaces.and_then(|()| {
                        CompilationSession::take_namespaces_from_cached(
                            engine,
                            next_pooled,
                            host.engine.work_cache.take_namespaces_handle(next_pooled),
                        )
                    }) {
                        Ok(entries) => {
                            host.engine
                                .work_cache
                                .namespace_pool_mode
                                .set(Some(next_pooled));
                            if !entries.is_empty() {
                                host.engine.work_cache.encode_peak_clean.set(false);
                            }
                            Ok(entries)
                        }
                        Err(error) => {
                            host.engine.work_cache.namespace_pool_mode.set(None);
                            host.engine.work_cache.encode_peak_clean.set(false);
                            Err(error)
                        }
                    }
                }
                _ => Ok(Vec::new()),
            };
            let namespaces = namespaces.unwrap_or_else(|_| {
                events.push(crate::macro_host::UnitEvent::Unmeasured);
                Vec::new()
            });
            let charges = UnitCharges {
                events,
                macro_depth: host.engine.work.macro_depth.get(),
                analysis_depth: host.engine.work.analysis_depth.get(),
                unit_depth: 0,
                limits: (host.limits.expansions, host.limits.generated_nodes),
                depth_limit: host.limits.expansion_depth.min(64),
                location,
                observations: recorded.observations,
                gensyms,
                namespaces,
                emitted: recorded.emitted,
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
        if let Some(rows) = deferred_rows {
            charges.trace = rows.finish();
        } else if !nested {
            charges.trace = crate::compile_trace::take_worker_rows();
        }
        guard.charges = Some((charges, time));
    }
    let diagnostic = analysis.as_ref().err().cloned();
    let result = analysis
        .map(|(mut p, w)| {
            p.shrink_to_fit();
            (Arc::new(p), w)
        })
        .map_err(|e| {
            let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
            format!("{}:{}:{}: {name}: {e}", loc.source, loc.line, loc.column)
        });
    if let Ok((p, _)) = &result
        && shared.measuring_ir()
    {
        guard.held = crate::program_fragment::SharedPrefix::same(&snapshot.resolution_program)
            .added_bytes(p);
    }
    // Prepare compile-time JS on the pool worker outside the catalog lock so
    // coordinator drain/release avoids serial DeferredUnit::prepare work.
    let prepared = if on_worker && let Ok((p, _)) = &result {
        Some(Arc::new(crate::emitter::DeferredUnit::prepare(p)))
    } else {
        None
    };
    guard.state = Some(match &result {
        Ok((p, w)) => AnalysisState::Analyzed(p.clone(), w.clone(), prepared),
        Err(e) => AnalysisState::Failed(e.clone(), diagnostic),
    });
    drop(guard);
    result
}
/// How many consecutive root registrations [`install_registrations`] gives
/// an engine at once, in blocks aligned to the catalog: an engine catching
/// up, such as a pool worker's, then installs a full block with the same
/// text as every other engine ([`MacroEvaluator::register_compiler_roots`]).
#[cfg(all(test, light_v8))]
const ROOT_BLOCK: usize = 64;
/// Install `registrations`, the catalog's from position `first`, in
/// `engine` in order; `before_last` runs in it before the last of them.
#[cfg(all(test, light_v8))]
fn install_registrations(
    engine: &dyn MacroEvaluator,
    first: usize,
    registrations: &[Registration],
    before_last: impl FnOnce(&dyn MacroEvaluator) -> Result<(), String>,
) -> Result<(), String> {
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
    let roots_view = view.clone();
    install_catalog_services(
        evaluator,
        view,
        binding.build.clone(),
        binding.dependency_log.clone(),
    )?;
    if roots_view
        .upgrade()
        .is_some_and(|view| view.uses_root_index())
    {
        evaluator
            .install_string_service(
                "__lightRootService",
                Rc::new(move |request| {
                    roots_view
                        .upgrade()
                        .map_or_else(String::new, |view| view.root_service(request))
                }),
            )
            .map_err(|e| e.to_string())?;
    }
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
    evaluator
        .install_build_object("globalThis.__lightBuildCaller")
        .map_err(|e| e.to_string())?;
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
    /// Whether this view backs `compiler_roots.js` with the catalog's
    /// Rust-owned [`RootIndex`] (`__lightRootService`) instead of eager replay.
    fn uses_root_index(&self) -> bool {
        false
    }
    /// Answer a `__lightRootService` lookup against this view's visibility
    /// cursor.
    fn root_service(&self, _request: &str) -> String {
        String::new()
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
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
    let mark_emitted = || {
        if !crate::macro_host::record_emitted(id) {
            shared.set_emitted(id);
        }
    };
    crate::reader_registration::in_callback(active, || {
        if let Some(prepared) = prepared {
            mark_emitted();
            register(prepared.tables())?;
            return Ok(prepared.code().to_owned());
        }
        match analyze() {
            Ok((program, _)) => {
                let code = { crate::emitter::emit_deferred_unit(&program) };
                mark_emitted();
                register(&crate::reader_registration::ReaderTables::of(&program))?;
                Ok(code)
            }
            // Another thread analyzed it and its application took the
            // program after this one looked; its JavaScript is prepared.
            Err(error) => match shared.prepared(id)? {
                Some(prepared) => {
                    mark_emitted();
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
        self.sync_catalog(Some(active))?;
        let code = compile_unit_in(&self.shared, active, id, || self.analyze(id, Some(active)))?;
        self.sync_catalog(Some(active))?;
        Ok(code)
    }
    fn unit_source(&self, id: usize) -> Option<u32> {
        self.shared.unit_source(id)
    }
    fn analysis_sources(&self) -> Vec<u32> {
        self.shared.analysis_sources(self.thread)
    }
    fn uses_root_index(&self) -> bool {
        true
    }
    fn root_service(&self, request: &str) -> String {
        if request.starts_with("r\0") || request.starts_with("c\0") {
            let pending = std::mem::take(&mut *self.deferred_engine_namespaces.borrow_mut());
            if pending.is_empty() {
                return String::new();
            }
            let mut out = String::from("[");
            for (i, ns) in pending.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                crate::source_map::json_string(&mut out, ns);
            }
            out.push(']');
            return out;
        }
        self.shared.serve_root_request(request, None)
    }
}
/// A worker thread's view of a [`SharedCatalog`]: the units it analyzes and
/// waits for are the compilation's, and the registrations it installs and
/// the roots it evaluates are its own engine's. Made, used and dropped on
/// the worker's thread.
pub(crate) struct WorkerView {
    shared: Arc<SharedCatalog>,
    thread: usize,
    /// The worker's host ([`MacroHost::bind`]), whose binding each unit's
    /// analysis frame is taken from.
    host: MacroHost,
    /// How many of the catalog's registrations the host's engine holds (or is
    /// synced to in [`RootIndex`]).
    installed: Cell<usize>,
    /// How many bundles and distinct extended protocols this view's V8
    /// engine has synced.
    synced_bundles: Cell<usize>,
    synced_ext_protocols: Cell<usize>,
    /// Whether its `light.build` requests go to the coordinator: a pool
    /// worker's ([`SharedCatalog::forward_build`]).
    forwards: Cell<bool>,
    /// Active parallel namespace overlay while this worker executes a
    /// top-level namespace pass (Stage C, light-cljs-1090).
    active_namespace: RefCell<Option<(Rc<CompilationSession>, u32)>>,
}

/// Guard restoring [`WorkerView::active_namespace`] on drop (including unwind).
pub(crate) struct NamespaceViewGuard<'a>(&'a WorkerView);
impl Drop for NamespaceViewGuard<'_> {
    fn drop(&mut self) {
        *self.0.active_namespace.borrow_mut() = None;
    }
}

/// Execution context handed to a `UnitJob::Namespace` closure on a pool worker.
pub(crate) struct NamespaceWorkerContext<'a> {
    pub(crate) shared: &'a Arc<SharedCatalog>,
    pub(crate) view: &'a Rc<WorkerView>,
    pub(crate) has_engine: bool,
}
#[cfg_attr(not(test), allow(dead_code))]
impl WorkerView {
    #[cfg(all(test, light_v8))]
    fn resolve_root(&self, name: &str) -> Result<usize, String> {
        self.shared.resolve_root(name, Some(self.installed.get()))
    }
    /// A view of `shared` for this thread, which analyzes in frames of
    /// `host`. If the host has an engine, the engine reaches the view
    /// through its services.
    pub(crate) fn new(shared: Arc<SharedCatalog>, mut host: MacroHost) -> Result<Rc<Self>, String> {
        let thread = shared.join();
        host.engine.catalog = Some(shared.clone());
        let view = Rc::new_cyclic(|view| {
            host.engine.worker = Some(view.clone());
            Self {
                shared,
                thread,
                host,
                installed: Cell::new(0),
                synced_bundles: Cell::new(0),
                synced_ext_protocols: Cell::new(0),
                forwards: Cell::new(false),
                active_namespace: RefCell::new(None),
            }
        });
        if let Some(bootstrap) = &view.host.engine.bootstrap {
            let weak: std::rc::Weak<dyn CatalogView> = Rc::downgrade(&view) as std::rc::Weak<Self>;
            prepare_engine(&bootstrap.evaluator, weak, &view.host.engine)?;
        }
        Ok(view)
    }
    /// Bind a parallel namespace overlay session to this worker view so
    /// `__lightRootService`, `__lightCompileUnit` and `light.build` route to it
    /// while the top-level pass runs.
    pub(crate) fn enter_namespace(
        &self,
        session: Rc<CompilationSession>,
        source_index: u32,
    ) -> NamespaceViewGuard<'_> {
        *self.active_namespace.borrow_mut() = Some((session, source_index));
        NamespaceViewGuard(self)
    }
    /// The worker view's engine binding frame (shared bootstrap, foreign macro
    /// cache, reader declarations, catalog and worker weak ref).
    pub(crate) fn engine_frame(&self) -> EngineBinding {
        self.host.engine.frame()
    }
    /// The worker view's catalog thread slot.
    pub(crate) fn thread_id(&self) -> usize {
        self.thread
    }
    /// Sync this worker view's V8 engine up to `limit` (exact
    /// `prefix_cursor`) without advancing past `limit`. Returns `Ok(Some(overlay_active))`
    /// when ready to run an overlay rooted at `limit` (including when the
    /// isolate is already at `installed > limit` and `limit..installed` is
    /// completely isolated from `< limit`, in which case `overlay_active` is
    /// `true`), or `Ok(None)` if `limit..installed` mutated prefix state.
    pub(crate) fn install_up_to(
        &self,
        limit: usize,
        prefix_bundles: usize,
    ) -> Result<Option<bool>, String> {
        let Some(bootstrap) = &self.host.engine.bootstrap else {
            return Ok(Some(false));
        };
        let installed = self.installed.get();
        if installed > limit {
            let isolated = {
                let c = self.shared.lock();
                c.roots
                    .is_isolated_suffix(limit, installed, &c.journal.registrations)
            };
            if !isolated {
                return Ok(None);
            }
            let tag = [
                "\0overlay".to_owned(),
                limit.to_string(),
                prefix_bundles.to_string(),
            ];
            bootstrap
                .evaluator
                .sync_catalog(limit, prefix_bundles, &tag)
                .map_err(|e| e.to_string())?;
            return Ok(Some(true));
        }
        if installed == limit {
            return Ok(Some(false));
        }
        let prev_ext = self.synced_ext_protocols.get();
        let (bundles, new_protocols) = {
            let c = self.shared.lock();
            let bundles = c.roots.bundles_before(limit);
            let (new_synced, protocols) = c.roots.new_extended_protocols(prev_ext, limit);
            self.synced_ext_protocols.set(new_synced);
            (bundles, protocols)
        };
        self.installed.set(limit);
        self.synced_bundles.set(bundles);
        bootstrap
            .evaluator
            .sync_catalog(limit, bundles, &new_protocols)
            .map_err(|e| e.to_string())?;
        Ok(Some(false))
    }
    /// Advance this thread's engine to the catalog's current registration
    /// frontier. On V8, updates the lightweight catalog cursor and
    /// extended-protocol set without locking or replaying individual roots or
    /// bundles (`__lightRootService` resolves them on demand from `RootIndex`).
    fn install(&self, active: Option<&dyn MacroEvaluator>) -> Result<(), String> {
        use std::sync::atomic::Ordering::Acquire;
        let Some(bootstrap) = &self.host.engine.bootstrap else {
            self.installed.set(self.shared.journal_len.load(Acquire));
            return Ok(());
        };
        let engine = active.unwrap_or(bootstrap.evaluator.as_ref());
        let total = self.shared.journal_len.load(Acquire);
        if total == self.installed.get() {
            return Ok(());
        }
        let prev_ext = self.synced_ext_protocols.get();
        let ext_count = self.shared.ext_protocol_count.load(Acquire);
        let (bundles, new_protocols) = if ext_count > prev_ext {
            let c = self.shared.lock();
            let bundles = c.roots.bundles_before(total);
            let (new_synced, protocols) = c.roots.new_extended_protocols(prev_ext, total);
            drop(c);
            self.synced_ext_protocols.set(new_synced);
            (bundles, protocols)
        } else {
            (self.shared.bundle_count.load(Acquire), Vec::new())
        };
        self.installed.set(total);
        self.synced_bundles.set(bundles);
        engine
            .sync_catalog(total, bundles, &new_protocols)
            .map_err(|e| e.to_string())
    }
    /// Whether the view's macro isolate took more headroom from the build's
    /// memory budget than a worker keeps between jobs
    /// ([`crate::evaluator::MemoryPool::keep`]), which only destroying it
    /// gives back: its worker retires it after the job, so what one unit
    /// needed does not stay taken for the rest of the build and decide
    /// which later units fit. What it holds up to that is its steady need,
    /// such as the build's macros, which a replacement would take again at
    /// once (light-cljs-459).
    fn holds_excess_headroom(&self) -> bool {
        let keep = self.shared.resources().map_or(0, |r| r.memory.keep());
        let macro_headroom = self
            .host
            .engine
            .bootstrap
            .as_ref()
            .map_or(0, |b| b.evaluator.headroom_held());
        macro_headroom > keep
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

        analyze_unit(
            &self.shared,
            self.thread,
            id,
            engine,
            Demand {
                help: None,
                queued: false,
                on_worker: true,
                held: false,
                server: None,
            },
            None,
            || (self.host.engine.frame(), None),
        )
    }
}
type NamespaceJob = Box<dyn FnOnce(Option<&NamespaceWorkerContext<'_>>) -> bool + Send>;

/// A job for a pool worker: either a deferred unit analysis or a parallel
/// top-level namespace pass (Stage C, light-cljs-1090).
enum UnitJob {
    Analyze(usize, usize, bool),
    Namespace {
        #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
        source_index: usize,
        phase: usize,
        needs_engine: bool,
        run: NamespaceJob,
    },
}
/// Phase layout for mixed pools (`0 < engine_workers < workers`):
/// 0: deferred units needing an engine (engine workers only)
/// 1: macro-free deferred units (any worker)
/// 2: top-level namespaces needing an engine (engine workers only)
/// 3: pure-Rust top-level namespaces (any worker)
/// Prioritizing deferred units over lookahead top-level namespaces keeps
/// `drain_front!` from stalling behind distant lookahead work (especially at
/// `--jobs 2`) while idle workers immediately run lookahead top-level passes.
const UNIT_ENGINE_PHASE: usize = 0;
const UNIT_PLAIN_PHASE: usize = 1;
const TOP_ENGINE_PHASE: usize = 2;
const TOP_PLAIN_PHASE: usize = 3;
/// Phase layout for unmixed pools (all workers equal):
/// 0: deferred units, 1: top-level namespaces.
const UNMIXED_UNIT_PHASE: usize = 0;
const UNMIXED_TOP_PHASE: usize = 1;
impl crate::scheduler::Job for UnitJob {
    fn phase(&self) -> usize {
        match self {
            Self::Analyze(_, phase, _) => *phase,
            Self::Namespace { phase, .. } => *phase,
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
            Self::Namespace { needs_engine, .. } => !*needs_engine,
        }
    }
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    fn key(&self) -> u64 {
        match self {
            Self::Analyze(id, _, _) => *id as u64,
            Self::Namespace { source_index, .. } => (1u64 << 60) | (*source_index as u64),
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
    /// Each worker with an engine owns one V8 isolate for the whole build.
    /// Workers beyond the isolate limit take only
    /// macro-free units (light-cljs-437).
    Persistent,
    /// Each unit is analyzed in a fresh isolate, the correctness reference
    /// of persistent ones (`LIGHT_CLJS_FRESH_UNIT_ISOLATES=1`).
    Fresh,
}
/// A `Send + Sync` handle for submitting pure-Rust CPU tasks (such as cache
/// artifact/index encoding and hashing) onto the active build's [`UnitPool`]
/// without spawning extra CPU threads outside `--jobs`.
#[derive(Clone)]
pub(crate) struct PoolTaskSubmitter {
    inbox: Arc<Inbox>,
    submitter: crate::scheduler::Submitter<UnitJob, ()>,
    mixed: bool,
}

impl PoolTaskSubmitter {
    /// Submit a pure-Rust CPU task onto the unit pool. Returns `false` if the
    /// pool has already closed or been cancelled.
    pub(crate) fn submit_plain(&self, key_index: usize, run: Box<dyn FnOnce() + Send>) -> bool {
        let mut state = self.inbox.lock();
        if state.closed || state.cancelled {
            return false;
        }
        let phase = if self.mixed {
            TOP_PLAIN_PHASE
        } else {
            UNMIXED_TOP_PHASE
        };
        #[cfg(not(any(debug_assertions, feature = "scheduling-stress")))]
        let _ = key_index;
        self.submitter
            .submit_batch(std::iter::once(UnitJob::Namespace {
                #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
                source_index: key_index,
                phase,
                needs_engine: false,
                run: Box::new(move |_| {
                    run();
                    false
                }),
            }));
        state.submissions += 1;
        drop(state);
        self.inbox.ready.notify_one();
        true
    }
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
    submitter: crate::scheduler::Submitter<UnitJob, ()>,
    mixed: bool,
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
    submissions: u64,
    closed: bool,
    cancelled: bool,
}
impl Inbox {
    fn lock(&self) -> MutexGuard<'_, InboxState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
    fn submissions(&self) -> u64 {
        self.lock().submissions
    }
    fn note_submitted(&self) {
        let mut state = self.lock();
        state.submissions += 1;
        drop(state);
        self.ready.notify_one();
    }
    /// Wait while the pool is quiescent until either more work was submitted
    /// (`true`) or the pool closed with no remaining submissions (`false`).
    fn wait_for_more(&self, seen: &mut u64) -> bool {
        let mut state = self.lock();
        loop {
            if state.cancelled {
                return false;
            }
            if state.submissions > *seen {
                *seen = state.submissions;
                return true;
            }
            if state.closed {
                return false;
            }
            state = self
                .ready
                .wait(state)
                .unwrap_or_else(PoisonError::into_inner);
        }
    }
    fn cancelled(&self) -> bool {
        self.lock().cancelled
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
        if unit >= LOCAL_UNIT_BASE && session.local_overlay.borrow().is_some() {
            return session.analyze_local(unit, engine, &host.engine);
        }
        return session.analyze(unit, engine);
    }
    let worker = host.engine.worker.as_ref().and_then(std::rc::Weak::upgrade);
    worker.expect("compilation session").analyze(unit, engine)
}
/// Give the analyzed `:test` unit `unit`, whose IR its definition's
/// appended, to its application owner
/// ([`CompilationSession::release_application_ir`]) from either thread.
pub(crate) fn release_definition_test(host: &MacroHost, unit: usize) {
    if unit >= LOCAL_UNIT_BASE {
        return;
    }
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
/// V8 isolate). The main isolate reserves what
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

/// Default cap on worker V8 isolates started for a build's unit pool.
/// Additional `--jobs` slots run as lightweight engine-less Rust workers taking
/// macro-free units, while the engine workers serve macro-expanding units first
/// and fall back to engine-less views for macro-free units (light-cljs-1090).
pub(crate) const MAX_POOL_WORKER_ISOLATES: usize = 3;

/// How many compile-time isolates (coordinator + workers) a V8 unit
/// pool sizes for `limit` and `slots`: bounded both by the memory budget
/// ([`isolate_count`]) and by `1 + MAX_POOL_WORKER_ISOLATES`.
pub(crate) fn pool_isolate_count(limit: usize, slots: crate::scheduler::Slots) -> usize {
    isolate_count(limit, slots).min(1 + MAX_POOL_WORKER_ISOLATES)
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
        // Beyond the main isolate's 64 MiB, reserve one isolate per 64 MiB.
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
        let mixed = engine_workers > 0 && engine_workers < slots.workers();
        let inbox = Arc::new(Inbox::default());
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
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
                    serve_pool(&inbox, tx, &shared, slots, engines, &environment, traced)
                })
                .map_err(|e| format!("cannot start the compiler's unit pool: {e}"))?
        };
        let submitter = rx
            .recv()
            .map_err(|_| "compiler unit pool failed before ready".to_owned())?;
        Ok(Self {
            inbox,
            submitter,
            mixed,
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
    pub(crate) fn submit(&self, units: Vec<(usize, bool)>) {
        if units.is_empty() {
            return;
        }
        (self.shared.counters.pooled).fetch_add(units.len(), std::sync::atomic::Ordering::Relaxed);
        if crate::wait_timeline::on() {
            for &(unit, _) in &units {
                crate::wait_timeline::instant(crate::wait_timeline::Kind::Dispatch, unit as u64, 0);
            }
        }
        let jobs = self.shared.unit_jobs(units, self.mixed);
        self.submitter.submit_batch(jobs);
        self.inbox.note_submitted();
    }
    /// Submit a parallel top-level namespace job directly into the scheduler,
    /// prioritized ahead of deferred unit work.
    pub(crate) fn submit_namespace(
        &self,
        source_index: usize,
        needs_engine: bool,
        run: NamespaceJob,
    ) {
        let phase = if self.mixed {
            if needs_engine {
                TOP_ENGINE_PHASE
            } else {
                TOP_PLAIN_PHASE
            }
        } else {
            UNMIXED_TOP_PHASE
        };
        #[cfg(not(any(debug_assertions, feature = "scheduling-stress")))]
        let _ = source_index;
        self.submitter
            .submit_batch(std::iter::once(UnitJob::Namespace {
                #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
                source_index,
                phase,
                needs_engine,
                run,
            }));
        self.inbox.note_submitted();
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
    let engine: Option<Rc<dyn crate::compiler_evaluator::CompilerEvaluator>> = match engines {
        PoolEngines::None => None,
        // A persistent slot borrows a task context another slot parked, if
        // one is idle (`WorkerView::park_context`).
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
        #[cfg(not(light_v8))]
        PoolEngines::Persistent | PoolEngines::Fresh => {
            return Err("compiler workers need an embedded engine".into());
        }
    };
    let engine_view = engine.is_some();
    let view = WorkerView::new(
        shared.clone(),
        MacroHost::bind(environment.clone(), engine)?,
    )?;
    view.forwards.set(true);
    if engine_view {
        let counters = &shared.counters;
        counters
            .isolates
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        PoolCounters::add(&counters.startup_ns, started);
    }
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
    if engine_view && let Some(perturbation) = shared.perturbation.get() {
        (perturbation.isolates).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
        UnitJob::Namespace {
            needs_engine, run, ..
        } => {
            let use_plain = !needs_engine && slot.engines != PoolEngines::None;
            #[allow(unused_mut)]
            let (mut kept, engines) = if use_plain {
                (slot.plain.take(), PoolEngines::None)
            } else {
                (slot.view.take(), slot.engines)
            };
            let view = match kept {
                Some(view) => view,
                None => match worker_view(shared, engines, environment) {
                    Ok(view) => view,
                    Err(_) => {
                        run(None);
                        return;
                    }
                },
            };
            if engines != PoolEngines::None {
                shared.counters.note_engine_participant(worker.slot());
            }
            let has_engine = view.host.engine.bootstrap.is_some();
            let ctx = NamespaceWorkerContext {
                shared,
                view: &view,
                has_engine,
            };
            let retire = run(Some(&ctx));
            if use_plain {
                slot.plain = Some(view);
            } else if !retire
                && slot.engines != PoolEngines::Fresh
                && !view.terminal()
                && !view.holds_excess_headroom()
            {
                slot.view = Some(view);
            }
            return;
        }
    };
    let _job = crate::wait_timeline::begin(
        crate::wait_timeline::Kind::Job,
        id as u64,
        macro_free as u64,
    );
    if !shared.admits(id) {
        return shared.skipped();
    }
    // Macro-free units never enter a compile-time engine on any backend
    // (`MacroFreeUnit` enforces this): run them on the slot's engine-less
    // view so an engine worker does not boot a V8 isolate or sync
    // catalog/frontier/gensym state just to analyze a macro-free unit.
    let plain = macro_free && slot.engines != PoolEngines::None;
    #[allow(unused_mut)]
    let (mut kept, engines) = if plain {
        (slot.plain.take(), PoolEngines::None)
    } else {
        (slot.view.take(), slot.engines)
    };
    // A native view parked its task context after its last job: it borrows
    // an idle one, or makes way for a view that boots one.
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
    if engines != PoolEngines::None {
        shared.counters.note_engine_participant(worker.slot());
    }
    let started = std::time::Instant::now();
    let analysis = view.analyze(id, None);
    if let Err(error) = analysis {
        shared.fail_unstarted(id, error);
        shared.note_job(id);
    }
    PoolCounters::add(&shared.counters.unit_ns, started);
    if slot.engines == PoolEngines::None || plain {
        shared
            .counters
            .engine_less_units
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    }
    if plain {
        slot.plain = Some(view);
    } else if slot.engines != PoolEngines::Fresh
        && !view.terminal()
        && !view.holds_excess_headroom()
    {
        slot.view = Some(view);
    }
}

fn serve_pool(
    inbox: &Inbox,
    submitter_tx: std::sync::mpsc::SyncSender<crate::scheduler::Submitter<UnitJob, ()>>,
    shared: &Arc<SharedCatalog>,
    slots: crate::scheduler::Slots,
    (engines, engine_workers): (PoolEngines, usize),
    environment: &MacroEnvironment,
    traced: bool,
) -> Result<(), String> {
    use crate::scheduler::{Config, Next};
    let started = std::time::Instant::now();
    let counters = &shared.counters;
    let relaxed = std::sync::atomic::Ordering::Relaxed;
    (counters.workers).store(slots.workers(), relaxed);
    (counters.engine_workers).store(engine_workers, relaxed);
    // With workers of both kinds, engine-requiring phases are served only by
    // the first `engine_workers` threads while pure-Rust phases are served by
    // every worker, and top-level phases take priority over deferred units.
    let mixed = engine_workers > 0 && engine_workers < slots.workers();
    let mut config = Config::new(slots, if mixed { 4 } else { 2 });
    if mixed {
        config.servers = vec![
            engine_workers,
            slots.workers(),
            engine_workers,
            slots.workers(),
        ];
    }
    // A worker analyzes as deep as the coordinator, whose main thread has
    // 8 MiB; V8's stack limit is set from where each engine entry starts.
    config.stack_size = Some(8 << 20);
    #[cfg(any(debug_assertions, feature = "scheduling-stress"))]
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
            let _ = submitter_tx.send(pool.submitter());
            let served = loop {
                let mut seen_submissions = inbox.submissions();
                match pool.next_outcome() {
                    Ok(Next::Outcome(())) => {
                        // The build ended early: what is queued is not needed.
                        if inbox.cancelled() {
                            pool.cancel();
                        }
                    }
                    Ok(Next::Quiescent) => {
                        if !inbox.wait_for_more(&mut seen_submissions) {
                            break Ok(());
                        }
                    }
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
    fn compile_unit(&self, active: &dyn MacroEvaluator, id: usize) -> Result<String, String> {
        let active_ns = self.active_namespace.borrow().clone();
        if let Some((ns_session, _)) = active_ns {
            if id >= LOCAL_UNIT_BASE {
                (self.shared.counters.initializations)
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                return ns_session.compile_local_unit(active, id, &self.host.engine);
            }
            // Demand of a prefix shared unit (< prefix_cursor): do NOT advance
            // this isolate's cursor past prefix_cursor.
            (self.shared.counters.initializations)
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            return compile_unit_in(&self.shared, active, id, || {
                analyze_unit(
                    &self.shared,
                    self.thread,
                    id,
                    Some(active),
                    Demand {
                        help: None,
                        queued: false,
                        on_worker: true,
                        held: false,
                        server: None,
                    },
                    None,
                    || (self.host.engine.frame(), None),
                )
            });
        }
        self.install(Some(active))?;
        (self.shared.counters.initializations).fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        compile_unit_in(&self.shared, active, id, || self.analyze(id, Some(active)))
    }
    fn unit_source(&self, id: usize) -> Option<u32> {
        if id >= LOCAL_UNIT_BASE
            && let Some((ns_session, _)) = self.active_namespace.borrow().as_ref()
        {
            return ns_session
                .local_overlay
                .borrow()
                .as_ref()
                .and_then(|o| o.units.get(id - LOCAL_UNIT_BASE))
                .map(|u| u.source);
        }
        self.shared.unit_source(id)
    }
    fn analysis_sources(&self) -> Vec<u32> {
        let mut sources = self.shared.analysis_sources(self.thread);
        if let Some((_, source_index)) = *self.active_namespace.borrow() {
            sources.insert(0, source_index);
        }
        sources
    }
    fn forward_build(&self, request: &str) -> Option<String> {
        self.forwards.get().then(|| {
            let (reply, log) = self.shared.forward_build(request, self.analysis_sources());
            // What it recorded is the unit's this worker records.
            if let Some(log) = log {
                crate::macro_host::record_observations(*log);
            }
            reply
        })
    }
    fn uses_root_index(&self) -> bool {
        true
    }
    fn root_service(&self, request: &str) -> String {
        if let Some((ns_session, _)) = self.active_namespace.borrow().as_ref() {
            return ns_session.serve_overlay_root_request(request);
        }
        self.shared
            .serve_root_request(request, Some(self.installed.get()))
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
                // `classify` reads this thread's expansion, never the coordinator's.
                if let Some(answer) = crate::build_services::serve_local(request) {
                    return answer;
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
                            window: analysis.first().copied().unwrap_or(log.window),
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
    charges: Option<(UnitCharges, std::time::Duration)>,
    note_failed: bool,
}
impl Drop for AnalysisGuard<'_> {
    fn drop(&mut self) {
        self.shared.settle(
            self.thread,
            self.id,
            self.state.take(),
            self.held,
            self.charges.take(),
            self.note_failed,
        );
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
    #[cfg(light_v8)]
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
    #[cfg(light_v8)]
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
    fn a_new_interactive_operation_clears_the_recorded_unit_budget() {
        use crate::{EvalOptions, dev_session::DevSession, source_provider::SourceInput};

        let dev = DevSession::new(&EvalOptions::default()).unwrap();
        let session = dev.compiler().host.session_handle().unwrap();
        let actions = dev.source(&SourceInput::new("<test>", "(let [] nil) (def a 1)"));
        assert!(matches!(
            actions.last(),
            Some(crate::dev_session::Action::Invoke(_))
        ));
        let (expansions, nodes) = session.shared.charged();
        assert!(expansions > 0 && nodes > 0, "{expansions}, {nodes}");

        dev.read_forms(&SourceInput::new("<test>", "nil")).unwrap();
        assert_eq!(session.shared.charged(), (0, 0));
    }

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

    /// A fresh compile-time engine made on this thread.
    #[cfg(light_v8)]
    fn engine(
        environment: &MacroEnvironment,
    ) -> Rc<dyn crate::compiler_evaluator::CompilerEvaluator> {
        let options = environment.evaluator_options().clone();
        Rc::new(crate::evaluator::Evaluator::new(options).unwrap())
    }

    /// The unit's prepared JavaScript as the main thread's session has it.
    #[cfg(light_v8)]
    fn main_code(session: &CompilationSession, id: usize) -> String {
        let prepare = |program: &Program| {
            crate::emitter::DeferredUnit::prepare(program)
                .code()
                .to_owned()
        };
        let state = session.shared.lock().units[id].state.clone();
        match state {
            AnalysisState::Prepared(_, _, prepared) => prepared.code().to_owned(),
            AnalysisState::Analyzed(program, ..) => prepare(&program),
            AnalysisState::Unstarted => {
                let (program, _) = session.analyze(id, None).unwrap();
                prepare(&program)
            }
            _ => panic!("unit {id} did not analyze"),
        }
    }

    #[cfg(light_v8)]
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
            let prepare = |program: &Program| {
                crate::emitter::DeferredUnit::prepare(program)
                    .code()
                    .to_owned()
            };
            let mut found: Vec<_> = jobs
                .into_iter()
                .map(|(id, form, snapshot, environment)| {
                    let engine = engine(&environment);
                    let mut host = MacroHost::bind(environment.clone(), Some(engine)).unwrap();
                    let (program, _) =
                        crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, None)
                            .unwrap();
                    let code = prepare(&program);
                    // A unit that expands no macro needs no engine at all.
                    if names[&id] == "cljs.user/plain" {
                        let mut host = MacroHost::bind(environment, None).unwrap();
                        let (program, _) = crate::analyzer::analyze_deferred_form(
                            &form, &mut host, &snapshot, None,
                        )
                        .unwrap();
                        assert_eq!(prepare(&program), code);
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
    #[cfg(light_v8)]
    struct Answers(Cell<Option<usize>>);
    #[cfg(light_v8)]
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

    #[cfg(light_v8)]
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

    #[cfg(light_v8)]
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
    #[cfg(light_v8)]
    struct Sources(RefCell<Vec<usize>>, Cell<bool>);
    #[cfg(light_v8)]
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

    #[cfg(light_v8)]
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

    #[test]
    fn failed_counts_exclude_units_prepared_after_the_chosen_failure() {
        let (session, units, _) =
            registered("(defn before [] 1) (defn bad [] (if)) (defn after [] 2)");
        let before = units["cljs.user/before"];
        let bad = units["cljs.user/bad"];
        let after = units["cljs.user/after"];
        // Warning-mode units may be prepared during their top-level pass,
        // before an earlier namespace reaches its failing drain point.
        let (early, _) = session.analyze(before, None).unwrap();
        session.release_application_ir(before);
        let (later, _) = session.analyze(after, None).unwrap();
        session.release_application_ir(after);
        // Merging speculative IR is not a namespace drain point.
        session.merged_ir(after);
        let counts = session.failed_unit_counts();
        assert_eq!([counts.analyzed, counts.prepared], [0, 0]);
        // Only the earlier namespace merged before the chosen failure.
        session.note_drained(before);
        session.merged_ir(before);
        let counts = session.failed_unit_counts();
        assert_eq!([counts.analyzed, counts.prepared], [1, 1]);
        assert!(session.analyze_drained(bad, false).is_err());
        let counts = session.failed_unit_counts();
        assert_eq!(
            [counts.registered, counts.analyzed, counts.prepared],
            [3, 2, 1]
        );
        let all = session.unit_counts();
        assert_eq!([all.analyzed, all.prepared], [3, 2]);
        drop((early, later));
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
            shared.settle(
                second,
                a,
                Some(AnalysisState::Failed("x".into(), None)),
                0,
                None,
                false,
            );
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
                None,
                false,
            );
            assert_eq!(waiter.join().unwrap().unwrap_err(), error);
            error
        });
        shared.settle(
            first,
            c,
            Some(AnalysisState::Failed(error, None)),
            0,
            None,
            false,
        );
        let c = shared.lock();
        assert!(
            c.threads
                .iter()
                .all(|t| t.stack.is_empty() && t.waiting.is_none())
        );
        assert_eq!(c.waiting, 0);
    }

    #[cfg(light_v8)]
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
                    assert_eq!(view.installed.get(), shared.lock().journal.registrations.len());
                })
                .join()
                .unwrap();
        });
        // Evaluated again in another engine, not analyzed again.
        assert_eq!(shared.lock().units[id].analyses, analyses);
    }

    #[cfg(light_v8)]
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

#[cfg(all(test, light_v8))]
mod root_index_tests {
    //! The catalog's [`RootIndex`] against the V8 registry (`compiler_roots.js`):
    //! both direct resolution (`resolve_root`) and lazy `__lightRootService`
    //! resolution must match eager V8 registration for every root/defonce/bundle
    //! combination and visibility cursor.
    use super::*;

    const BUNDLE: usize = 10_000;

    struct Recorder {
        asked: RefCell<Vec<usize>>,
        bundles: HashMap<usize, Vec<String>>,
    }
    impl CatalogView for Recorder {
        fn compile_unit(&self, _: &dyn MacroEvaluator, id: usize) -> Result<String, String> {
            self.asked.borrow_mut().push(id);
            if let Some(products) = self.bundles.get(&id) {
                let mut body = String::from("rt => {");
                for name in products {
                    body.push_str(&format!(
                        "globalThis.__lightCompilerBundleProducts[{name:?}] = {};",
                        BUNDLE + id
                    ));
                }
                body.push('}');
                Ok(body)
            } else {
                Ok(format!("rt => {id}"))
            }
        }
        fn unit_source(&self, _: usize) -> Option<u32> {
            None
        }
        fn analysis_sources(&self) -> Vec<u32> {
            Vec::new()
        }
    }

    fn root(name: &str, unit: Option<usize>, once: bool) -> Registration {
        Registration::Root(name.into(), unit, None, once, format!("{name}.cljs"))
    }
    fn bundle(products: &[&str], unit: usize, once: bool, group: bool) -> Registration {
        Registration::Bundle(Arc::new(BundleRecord {
            products: products.iter().map(|&name| name.into()).collect(),
            unit,
            extensions: Vec::new(),
            location: "bundle.cljs".into(),
            once,
            group,
        }))
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    enum Read {
        Unit(usize),
        Bundle(usize),
        Unavailable,
    }

    fn v8_reads(registrations: &[Registration], names: &[&str]) -> Vec<Read> {
        let evaluator: Rc<dyn crate::compiler_evaluator::CompilerEvaluator> =
            Rc::new(crate::v8_evaluator::V8Evaluator::new(Default::default()).unwrap());
        let bundles = registrations
            .iter()
            .filter_map(|registration| match registration {
                Registration::Bundle(bundle) => Some((bundle.unit, bundle.products.clone())),
                Registration::Root(..) => None,
            })
            .collect();
        let view = Rc::new(Recorder {
            asked: RefCell::new(Vec::new()),
            bundles,
        });
        let weak = Rc::downgrade(&view) as std::rc::Weak<Recorder>;
        prepare_engine(&evaluator, weak, &MacroHost::new().engine).unwrap();
        install_registrations(evaluator.as_ref(), 0, registrations, |_| Ok(())).unwrap();
        names
            .iter()
            .map(|name| {
                let value = evaluator
                    .evaluate_js(&format!(
                        "(() => {{ try {{ return globalThis.__lightCompilerRoots[{name:?}]; }} catch (error) {{ if (!String(error.message).includes('unavailable definition: ')) throw error; return -1; }} }})()"
                    ))
                    .unwrap();
                let n = evaluator.number(value).unwrap();
                evaluator.release(value).unwrap();
                let read = match n {
                    n if n < 0.0 => Read::Unavailable,
                    n if n as usize >= BUNDLE => Read::Bundle(n as usize - BUNDLE),
                    n => Read::Unit(n as usize),
                };
                if let Read::Unit(unit) | Read::Bundle(unit) = read {
                    assert!(view.asked.borrow().contains(&unit), "{name}: {read:?}");
                }
                read
            })
            .collect()
    }

    fn index_read(answer: Result<usize, String>, name: &str, bundle: Option<usize>) -> Read {
        match answer {
            Ok(unit) => Read::Unit(unit),
            Err(error) if error.contains("is a declaration-bundle product") => {
                Read::Bundle(bundle.unwrap_or(usize::MAX))
            }
            Err(error) => {
                assert!(
                    error.starts_with(&format!("compiler root is not registered: {name}")),
                    "{error}"
                );
                Read::Unavailable
            }
        }
    }

    fn record(session: &CompilationSession, registrations: &[Registration]) {
        for registration in registrations {
            session.record_registration(&mut session.shared.lock(), registration.clone());
        }
    }

    fn index_reads(
        names: &[&str],
        v8: &[Read],
        resolve: &dyn Fn(&str) -> Result<usize, String>,
    ) -> Vec<Read> {
        names
            .iter()
            .zip(v8)
            .map(|(name, v8)| {
                let bundle = match v8 {
                    Read::Bundle(unit) => Some(*unit),
                    _ => None,
                };
                index_read(resolve(name), name, bundle)
            })
            .collect()
    }

    struct LazyRecorder {
        inner: Recorder,
        shared: Arc<SharedCatalog>,
    }
    impl CatalogView for LazyRecorder {
        fn compile_unit(&self, active: &dyn MacroEvaluator, unit: usize) -> Result<String, String> {
            self.inner.compile_unit(active, unit)
        }
        fn unit_source(&self, id: usize) -> Option<u32> {
            self.inner.unit_source(id)
        }
        fn analysis_sources(&self) -> Vec<u32> {
            self.inner.analysis_sources()
        }
        fn uses_root_index(&self) -> bool {
            true
        }
        fn root_service(&self, request: &str) -> String {
            self.shared.serve_root_request(request, None)
        }
    }

    fn lazy_v8_reads(
        session: &CompilationSession,
        registrations: &[Registration],
        names: &[&str],
    ) -> Vec<Read> {
        let evaluator: Rc<dyn crate::compiler_evaluator::CompilerEvaluator> =
            Rc::new(crate::v8_evaluator::V8Evaluator::new(Default::default()).unwrap());
        let bundles = registrations
            .iter()
            .filter_map(|registration| match registration {
                Registration::Bundle(bundle) => Some((bundle.unit, bundle.products.clone())),
                Registration::Root(..) => None,
            })
            .collect();
        let view = Rc::new(LazyRecorder {
            inner: Recorder {
                asked: RefCell::new(Vec::new()),
                bundles,
            },
            shared: session.shared.clone(),
        });
        let weak = Rc::downgrade(&view) as std::rc::Weak<LazyRecorder>;
        prepare_engine(&evaluator, weak, &MacroHost::new().engine).unwrap();
        let (total, bundle_count, protocols) = {
            let c = session.shared.lock();
            let (_, protocols) = c
                .roots
                .new_extended_protocols(0, c.journal.registrations.len());
            (
                c.journal.registrations.len(),
                c.journal.bundle_count,
                protocols,
            )
        };
        evaluator
            .sync_catalog(total, bundle_count, &protocols)
            .unwrap();
        names
            .iter()
            .map(|name| {
                let value = evaluator
                    .evaluate_js(&format!(
                        "(() => {{ try {{ return globalThis.__lightCompilerRoots[{name:?}]; }} catch (error) {{ if (!String(error.message).includes('unavailable definition: ')) throw error; return -1; }} }})()"
                    ))
                    .unwrap();
                let n = evaluator.number(value).unwrap();
                evaluator.release(value).unwrap();
                let read = match n {
                    n if n < 0.0 => Read::Unavailable,
                    n if n as usize >= BUNDLE => Read::Bundle(n as usize - BUNDLE),
                    n => Read::Unit(n as usize),
                };
                if let Read::Unit(unit) | Read::Bundle(unit) = read {
                    assert!(
                        view.inner.asked.borrow().contains(&unit),
                        "{name}: {read:?}"
                    );
                }
                read
            })
            .collect()
    }

    fn agree(case: &str, registrations: &[Registration], expected: &[(&str, Read)]) {
        let names = expected.iter().map(|&(name, _)| name).collect::<Vec<_>>();
        let expected = expected.iter().map(|&(_, read)| read).collect::<Vec<_>>();
        let v8 = v8_reads(registrations, &names);
        assert_eq!(v8, expected, "{case}: eager V8");
        let session = CompilationSession::default();
        record(&session, registrations);
        let index = index_reads(&names, &v8, &|name| session.shared.resolve_root(name, None));
        assert_eq!(index, expected, "{case}: RootIndex");
        let lazy_v8 = lazy_v8_reads(&session, registrations, &names);
        assert_eq!(lazy_v8, expected, "{case}: lazy RootIndex V8");
    }

    #[test]
    fn the_root_index_resolves_as_the_v8_registry_does() {
        use Read::{Bundle, Unavailable, Unit};
        agree(
            "plain root",
            &[root("t/a", Some(1), false)],
            &[("t/a", Unit(1)), ("t/b", Unavailable)],
        );
        agree(
            "redefinition",
            &[root("t/a", Some(1), false), root("t/a", Some(2), false)],
            &[("t/a", Unit(2))],
        );
        agree(
            "defonce over a defined name",
            &[root("t/a", Some(1), false), root("t/a", Some(2), true)],
            &[("t/a", Unit(1))],
        );
        agree(
            "defonce over a declared name",
            &[root("t/a", None, false), root("t/a", Some(2), true)],
            &[("t/a", Unit(2))],
        );
        agree(
            "defonce over an absent name",
            &[root("t/a", Some(2), true)],
            &[("t/a", Unit(2))],
        );
        agree(
            "declaration without a unit",
            &[
                root("t/a", None, false),
                root("t/b", Some(1), false),
                root("t/b", None, false),
            ],
            &[("t/a", Unavailable), ("t/b", Unit(1))],
        );
        agree(
            "bundle product over a root",
            &[
                root("t/a", Some(1), false),
                bundle(&["t/a", "t/b"], 10, false, false),
            ],
            &[("t/a", Bundle(10)), ("t/b", Bundle(10))],
        );
        agree(
            "defonce over a bundle product",
            &[
                bundle(&["t/a"], 10, false, false),
                root("t/a", Some(2), true),
            ],
            &[("t/a", Bundle(10))],
        );
        agree(
            "group bundle",
            &[
                root("t/a", Some(1), false),
                bundle(&["t/a", "t/b"], 11, false, true),
            ],
            &[("t/a", Bundle(11)), ("t/b", Bundle(11))],
        );
        agree(
            "defonce over a group bundle product",
            &[
                bundle(&["t/a"], 11, false, true),
                root("t/a", Some(3), true),
            ],
            &[("t/a", Bundle(11))],
        );
        agree(
            "redefinition after a bundle",
            &[
                bundle(&["t/a"], 10, false, false),
                root("t/a", Some(2), false),
            ],
            &[("t/a", Unit(2))],
        );
        agree(
            "once bundle over defined products",
            &[
                root("t/a", Some(1), false),
                bundle(&["t/a"], 12, true, false),
            ],
            &[("t/a", Unit(1))],
        );
        agree(
            "once bundle over some undefined products",
            &[
                root("t/a", Some(1), false),
                root("t/b", None, false),
                bundle(&["t/a", "t/b"], 12, true, false),
            ],
            &[("t/a", Bundle(12)), ("t/b", Bundle(12))],
        );
    }

    #[test]
    fn a_worker_resolves_roots_at_its_cursor_and_the_coordinator_at_the_journal() {
        use Read::{Bundle, Unavailable, Unit};
        let before = [root("t/a", Some(1), false), root("t/b", Some(2), false)];
        let after = [
            root("t/a", Some(3), false),
            bundle(&["t/b"], 10, false, false),
            root("t/c", Some(4), false),
        ];
        let all = before.iter().chain(&after).cloned().collect::<Vec<_>>();
        let names = ["t/a", "t/b", "t/c"];
        let session = CompilationSession::default();
        record(&session, &before);
        let view = WorkerView::new(session.shared.clone(), MacroHost::new()).unwrap();
        view.install(None).unwrap();
        record(&session, &after);
        let expected_before = v8_reads(&before, &names);
        assert_eq!(expected_before, [Unit(1), Unit(2), Unavailable]);
        assert_eq!(
            index_reads(&names, &expected_before, &|name| view.resolve_root(name)),
            expected_before
        );
        let expected_all = v8_reads(&all, &names);
        assert_eq!(expected_all, [Unit(3), Bundle(10), Unit(4)]);
        assert_eq!(
            index_reads(&names, &expected_all, &|name| {
                session.shared.resolve_root(name, None)
            }),
            expected_all
        );
        // Advancing the worker's cursor brings it up to the full journal.
        view.install(None).unwrap();
        assert_eq!(
            index_reads(&names, &expected_all, &|name| view.resolve_root(name)),
            expected_all
        );
    }

    #[test]
    fn pool_isolate_count_caps_worker_isolates_while_preserving_memory_pool_invariants() {
        use crate::evaluator::MemoryPool;
        use crate::scheduler::Slots;
        let slots = |n| Slots::new(n).unwrap();
        assert_eq!(pool_isolate_count(64 << 20, slots(8)), 1);
        assert_eq!(pool_isolate_count(128 << 20, slots(8)), 2);
        assert_eq!(pool_isolate_count(256 << 20, slots(4)), 4);
        assert_eq!(pool_isolate_count(4_000_000_000, slots(2)), 2);
        assert_eq!(pool_isolate_count(4_000_000_000, slots(4)), 4);
        assert_eq!(
            pool_isolate_count(4_000_000_000, slots(8)),
            1 + MAX_POOL_WORKER_ISOLATES
        );
        for n in [2, 4, 8, 12, 16] {
            for limit in [64 << 20, 128 << 20, 256 << 20, 2 << 30] {
                let isolates = pool_isolate_count(limit, slots(n));
                let pool = MemoryPool::new(limit, isolates);
                assert_eq!(
                    pool.main_base() + (isolates - 1) * pool.base() + pool.headroom(),
                    limit,
                    "limit={limit} n={n}"
                );
            }
        }
    }
}
