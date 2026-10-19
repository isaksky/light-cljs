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
}
impl UnitCharges {
    /// Add the charges to `binding`'s counters and marks, and spend the
    /// unit's polls from `control`, the execution control its analysis on
    /// the charging thread would have polled; the unit was analyzed at
    /// `depth` of that thread's stack. The first poll or native expansion
    /// that fails fails the unit, with the message and diagnostic its
    /// analysis there would have given.
    fn charge(
        &self,
        binding: &EngineBinding,
        depth: usize,
        control: Option<&crate::evaluator::ExecutionControl>,
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
                UnitEvent::Poll(span) => {
                    if let Some(Err(error)) = control.map(|control| control.poll()) {
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
enum Registration {
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
    /// By the slot each view joined with ([`SharedCatalog::join`]).
    threads: Vec<AnalysisThread>,
    /// How many threads wait for a unit another thread analyzes.
    waiting: usize,
}
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
    #[cfg(test)]
    on_claim: Mutex<Option<ClaimHook>>,
    /// Runs on a waiting thread each time its wait ends, without the lock.
    #[cfg(test)]
    on_wake: Mutex<Option<ClaimHook>>,
}
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<SharedCatalog>();
};
/// What a demand for a unit gets from the catalog.
enum Claim {
    /// The demanding thread analyzes the unit, at this depth of its stack;
    /// with its name if the unit is macro-free.
    Analyze(Box<UnitInputs>, usize, Option<String>),
    /// The unit was too deep in the thread's stack and has failed.
    TooDeep(Box<UnitInputs>, usize),
    /// Another analysis's result.
    Reuse(Arc<Program>, Vec<AnalysisWarning>),
}
/// Runs on the thread that claimed a unit, before it analyzes the unit.
#[cfg(test)]
type ClaimHook = Box<dyn Fn(usize) + Send>;
const DEPTH_LIMIT: &str = "compiler macro expansion depth limit exceeded in deferred units";
impl SharedCatalog {
    fn lock(&self) -> MutexGuard<'_, Catalog> {
        self.catalog.lock().unwrap_or_else(PoisonError::into_inner)
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
    fn claim(&self, thread: usize, id: usize) -> Result<Claim, String> {
        // Dropped after the lock is released.
        let mut _released = None;
        let mut guard = self.lock();
        loop {
            let c = &mut *guard;
            let state = &c.units.get(id).ok_or("unknown compilation unit")?.state;
            if let AnalysisState::Analyzing(owner) = *state
                && owner != thread
            {
                if c.threads[thread].waiting.is_none() {
                    if let Some(path) = c.wait_cycle(thread, id) {
                        return Err(format!("compile dependency cycle: {path}"));
                    }
                    c.threads[thread].waiting = Some(id);
                    c.waiting += 1;
                    c.units[id].waiters += 1;
                }
                guard = self
                    .settled
                    .wait(guard)
                    .unwrap_or_else(PoisonError::into_inner);
                #[cfg(test)]
                if let Some(hook) = &*self.on_wake.lock().unwrap() {
                    drop(guard);
                    hook(id);
                    guard = self.lock();
                }
                continue;
            }
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
                return Ok(Claim::TooDeep(inputs, depth));
            }
            unit.state = AnalysisState::Analyzing(thread);
            unit.analyses += 1;
            let macro_free = unit.macro_free.then(|| unit.name.clone());
            // The request has stable identity even if a newer same-kind source
            // generation is activated while this frame is suspended.
            debug_assert!(unit.request.generation > 0);
            debug_assert_eq!(c.bindings[&unit.name].0, unit.request.var);
            c.threads[thread].stack.push(id);
            return Ok(Claim::Analyze(inputs, depth, macro_free));
        }
    }
    /// End the analysis of unit `id` that the thread in slot `thread` claimed
    /// with `state`, or as interrupted, and wake the threads waiting for it.
    fn settle(&self, thread: usize, id: usize, state: Option<AnalysisState>) {
        let (previous, waiting) = {
            let mut c = self.lock();
            let c = &mut *c;
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
        drop(previous);
        if waiting {
            self.settled.notify_all();
        }
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
    /// Fail unit `id`, analyzed, as its charges did.
    fn fail(&self, id: usize, message: String, diagnostic: Option<crate::AnalysisError>) {
        let previous = std::mem::replace(
            &mut self.lock().units[id].state,
            AnalysisState::Failed(message, diagnostic),
        );
        drop(previous);
    }
    /// Whether unit `id` is macro-free.
    pub(crate) fn macro_free(&self, id: usize) -> bool {
        self.lock()
            .units
            .get(id)
            .is_some_and(|unit| unit.macro_free)
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
}
/// What capture needs beyond the catalog, recorded only while it is on.
#[derive(Default)]
struct SessionCapture {
    /// By unit ID from `first_unit` on.
    units: Vec<UnitCapture>,
    first_unit: usize,
    macros: Vec<CapturedMacro>,
    expansions: Vec<CapturedExpansion>,
}
/// A source macro call analysis expanded, for an artifact's provenance.
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
    /// The program globals the merge of unit `unit` appended
    /// ([`Self::capture_added_globals`]), taken.
    pub fn take_added_globals(&self, unit: usize) -> Vec<usize> {
        self.capture
            .borrow_mut()
            .as_mut()
            .and_then(|capture| {
                let index = unit.checked_sub(capture.first_unit)?;
                capture.units.get_mut(index)
            })
            .map(|entry| std::mem::take(&mut entry.added))
            .unwrap_or_default()
    }
    /// The units from ID `from` on.
    pub fn captured_units(&self, from: usize) -> Result<Vec<UnitRecord>, String> {
        let units = self
            .shared
            .lock()
            .units
            .iter()
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
    /// The registrations from position `from` on.
    pub fn captured_registrations(&self, from: usize) -> Vec<RegistrationRecord> {
        self.shared
            .lock()
            .registrations
            .iter()
            .skip(from)
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
        install_registrations(bootstrap.evaluator.as_ref(), &pending, before_last)
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
        match self.bootstrap() {
            Some(bootstrap) => Self::take_gensyms_from(bootstrap.evaluator.as_ref()).map(drop),
            None => Ok(()),
        }
    }
    /// Where `gensym`'s counter stands, and what it returned since the last
    /// call or since [`Self::log_gensyms`]; before the session starts, the
    /// counter it will start from and nothing.
    pub fn take_gensyms(&self) -> Result<(u64, Vec<Vec<u16>>), String> {
        match self.bootstrap() {
            Some(bootstrap) => Self::take_gensyms_from(bootstrap.evaluator.as_ref()),
            None => Ok((self.gensym_floor.get(), Vec::new())),
        }
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
            counts.macro_free_time += unit.time;
            counts.analyzed += usize::from(unit.analyses > 0);
            counts.prepared += usize::from(matches!(unit.state, AnalysisState::Prepared(..)));
            counts.executed += usize::from(unit.emitted);
        }
        counts
    }
    pub fn extension_frontier(&self) -> usize {
        self.shared.lock().bundle_count
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
    pub fn diagnostic(&self, id: usize) -> Option<crate::AnalysisError> {
        match &self.shared.lock().units.get(id)?.state {
            AnalysisState::Failed(_, diagnostic) => diagnostic.clone(),
            _ => None,
        }
    }
    /// Called after the analyzed body has been copied into its application
    /// owner. Preparing source is pure: execution and registration stay lazy.
    pub fn release_application_ir(&self, id: usize) {
        let program = match &self.shared.lock().units[id].state {
            AnalysisState::Analyzed(program, _) => program.clone(),
            _ => return,
        };
        // Prepared outside the lock; the analyzed program is dropped by its
        // last holder, outside it too.
        let prepared = Arc::new(crate::emitter::DeferredUnit::prepare(&program));
        let previous = {
            let mut c = self.shared.lock();
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
        let bootstrap = self.bootstrap();
        // A macro-free unit's analysis is charged to the counters of the
        // binding it was registered with, a frame of the compilation's.
        let counters = self
            .shared
            .macro_free(id)
            .then(|| {
                self.unit_bindings
                    .borrow()
                    .get(id)
                    .and_then(Option::as_ref)
                    .map(|binding| binding.frame())
            })
            .flatten();
        let analysis = analyze_unit(&self.shared, self.thread, id, engine, || {
            let binding = self.take_unit_binding(id).expect("unstarted unit host");
            (*binding, bootstrap)
        });
        // Analyzed on another thread, the unit's binding is no longer needed.
        drop(self.take_unit_binding(id));
        if let Some(binding) = counters
            && let Some(charges) = self.shared.take_charges(id)
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
            if let Err((message, diagnostic)) = charges.charge(&binding, depth, control.as_ref()) {
                self.shared.fail(id, message.clone(), Some(diagnostic));
                return Err(message);
            }
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
    binding: impl FnOnce() -> (EngineBinding, Option<Bootstrap>),
) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
    let (inputs, depth, macro_free) = match shared.claim(thread, id)? {
        Claim::Reuse(program, warnings) => return Ok((program, warnings)),
        Claim::TooDeep(inputs, depth) => {
            let (binding, _) = binding();
            crate::macro_host::WorkMarks::note(&binding.work.unit_depth, depth + 1);
            drop((inputs, binding));
            return Err(DEPTH_LIMIT.to_owned());
        }
        Claim::Analyze(inputs, depth, macro_free) => (inputs, depth, macro_free),
    };
    // From here on, however the analysis ends, even by unwinding, the unit
    // settles and its waiters wake.
    let mut guard = AnalysisGuard {
        shared,
        thread,
        id,
        state: None,
    };
    #[cfg(test)]
    if let Some(hook) = &*shared
        .on_claim
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
    {
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
    // Declared before the frontier guard, so the frontier's transitions
    // count in the unit's frame (`--stats`, [`crate::compile_trace`]).
    let _traced = crate::compile_trace::enter_unit(
        || crate::compile_trace::UnitInfo {
            id,
            source: shared.unit_source(id).unwrap_or_default(),
            name: shared.unit_name(id),
            span: form.span(),
            depth,
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
    let analysis = if let Some(name) = macro_free {
        let started = std::time::Instant::now();
        // Counted where the unit is charged ([`UnitCharges`]).
        host.engine.expansions = Default::default();
        host.engine.generated_nodes = Default::default();
        host.engine.work = Default::default();
        let loc = crate::compiler_data::DiagnosticLocation::from_span(&form.span());
        let location = format!("{}:{}:{}: {name}", loc.source, loc.line, loc.column);
        let free = crate::macro_host::MacroFreeUnit::enter(name);
        let analysis = crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine);
        let charges = UnitCharges {
            events: free.finish(),
            macro_depth: host.engine.work.macro_depth.get(),
            analysis_depth: host.engine.work.analysis_depth.get(),
            limits: (host.limits.expansions, host.limits.generated_nodes),
            depth_limit: host.limits.expansion_depth.min(64),
            location,
        };
        shared.record_charges(id, charges, started.elapsed());
        analysis
    } else {
        crate::analyzer::analyze_deferred_form(&form, &mut host, &snapshot, engine)
    };
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
    guard.state = Some(match &result {
        Ok((p, w)) => AnalysisState::Analyzed(p.clone(), w.clone()),
        Err(e) => AnalysisState::Failed(e.clone(), diagnostic),
    });
    drop(guard);
    result
}
/// Install `registrations` in `engine` in order; `before_last` runs in it
/// before the last of them.
fn install_registrations(
    engine: &dyn MacroEvaluator,
    registrations: &[Registration],
    before_last: impl FnOnce(&dyn MacroEvaluator) -> Result<(), String>,
) -> Result<(), String> {
    let mut before_last = Some(before_last);
    for (index, registration) in registrations.iter().enumerate() {
        if index + 1 == registrations.len()
            && let Some(before_last) = before_last.take()
        {
            before_last(engine)?;
        }
        match registration {
            Registration::Root(name, unit, test, once, location) => engine
                .register_compiler_root(name, *unit, *once, location, *test)
                .map_err(|e| e.to_string())?,
            Registration::Bundle(bundle) => {
                let value = engine
                    .evaluate_unit(&bundle.source())
                    .map_err(|e| e.to_string())?;
                engine.release(value).map_err(|e| e.to_string())?;
            }
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
    install_catalog_services(
        evaluator,
        view,
        binding.build.clone(),
        binding.dependency_log.clone(),
    )?;
    // Registry and root states stay in JS, so ready reads never cross Rust.
    let value = evaluator
        .evaluate_js(include_str!("compiler_roots.js"))
        .map_err(|e| e.to_string())?;
    evaluator.release(value).map_err(|e| e.to_string())?;
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
    /// `__lightCompileUnit`: unit `id`'s initializer, analyzed first if it
    /// is not yet, with the registration it needs evaluated in `active`.
    fn compile_unit(&self, active: &dyn MacroEvaluator, id: usize) -> Result<String, String>;
    /// `light.build`: the source index of unit `id`, if the catalog can tell.
    fn unit_source(&self, id: usize) -> Option<u32>;
    /// `light.build`: the source indexes of the units being analyzed, the
    /// innermost last.
    fn analysis_sources(&self) -> Vec<u32>;
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
    let prepared = shared.prepared(id)?;
    crate::compile_trace::executed(id, prepared.is_some());
    let (registration, code) = if let Some(prepared) = prepared {
        prepared.emit()
    } else {
        match analyze() {
            Ok((program, _)) => crate::emitter::emit_deferred_unit(&program),
            // Another thread analyzed it and its application took the
            // program after this one looked; its JavaScript is prepared.
            Err(error) => match shared.prepared(id)? {
                Some(prepared) => prepared.emit(),
                None => return Err(error),
            },
        }
    };
    shared.set_emitted(id);
    active
        .evaluate_registration(&registration.source, &registration.chunks)
        .map_err(|e| e.to_string())?;
    Ok(code)
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
    shared: Arc<SharedCatalog>,
    thread: usize,
    /// The worker's host ([`MacroHost::bind`]), whose binding each unit's
    /// analysis frame is taken from.
    host: MacroHost,
    /// How many of the catalog's registrations the host's engine holds.
    installed: Cell<usize>,
}
#[cfg_attr(not(test), allow(dead_code))]
impl WorkerView {
    /// A view of `shared` for this thread, which analyzes in frames of
    /// `host`. If the host has an engine, the engine reaches the view
    /// through its services.
    pub(crate) fn new(shared: Arc<SharedCatalog>, host: MacroHost) -> Result<Rc<Self>, String> {
        let thread = shared.join();
        let view = Rc::new(Self {
            shared,
            thread,
            host,
            installed: Cell::new(0),
        });
        if let Some(bootstrap) = &view.host.engine.bootstrap {
            let weak: std::rc::Weak<dyn CatalogView> = Rc::downgrade(&view) as std::rc::Weak<Self>;
            prepare_engine(&bootstrap.evaluator, weak, &view.host.engine)?;
        }
        Ok(view)
    }
    /// Install in this thread's engine the registrations made since it last
    /// looked, through `active` while the engine runs code that demands.
    fn install(&self, active: Option<&dyn MacroEvaluator>) -> Result<(), String> {
        let Some(bootstrap) = &self.host.engine.bootstrap else {
            return Ok(());
        };
        let pending = {
            let c = self.shared.lock();
            if c.registrations.len() == self.installed.get() {
                return Ok(());
            }
            let pending = c.registrations[self.installed.get()..].to_vec();
            self.installed.set(c.registrations.len());
            pending
        };
        let engine = active.unwrap_or(bootstrap.evaluator.as_ref());
        install_registrations(engine, &pending, |_| Ok(()))
    }
    /// Unit `id`'s analysis, by this thread or another.
    pub(crate) fn analyze(
        &self,
        id: usize,
        engine: Option<&dyn MacroEvaluator>,
    ) -> Result<(Arc<Program>, Vec<AnalysisWarning>), String> {
        self.install(engine)?;
        analyze_unit(&self.shared, self.thread, id, engine, || {
            (self.host.engine.frame(), None)
        })
    }
}
/// A macro-free unit for a pool worker to analyze.
struct UnitJob(usize);
impl crate::scheduler::Job for UnitJob {
    fn phase(&self) -> usize {
        0
    }
}
/// Run `coordinate` on this thread while the workers of `slots` analyze the
/// macro-free `units` of `shared`, each with no compile-time engine
/// (docs/compiler-concurrency.md, "As built (light-cljs-338)"). The
/// analyses go to the catalog, where the coordinator's own demands claim,
/// wait for or reuse them, in its order: nothing here decides an order, and
/// a unit the coordinator reaches first it analyzes itself. Returns once
/// every job has run. A worker's panic, such as the eligibility check's,
/// panics the build.
pub(crate) fn with_unit_pool<R>(
    shared: &Arc<SharedCatalog>,
    slots: crate::scheduler::Slots,
    units: Vec<usize>,
    coordinate: impl FnOnce() -> R,
) -> R {
    use crate::scheduler::{Config, Next};
    let pooled = crate::scheduler::run(
        &Config::new(slots, 1),
        |_slot| WorkerView::new(shared.clone(), MacroHost::default()),
        |view, UnitJob(id), _worker| {
            // The outcome is in the catalog; a failed unit is reported by
            // the coordinator when it reaches it, as a serial drain would.
            if let Ok(view) = view {
                let _ = view.analyze(id, None);
            }
        },
        |pool| {
            pool.submit_batch(units.into_iter().map(UnitJob));
            let result = coordinate();
            loop {
                match pool.next_outcome() {
                    Ok(Next::Outcome(())) => {}
                    Ok(Next::Quiescent) => break,
                    Ok(Next::Stalled { .. }) => unreachable!("unit jobs hold no demands"),
                    Err(panic) => panic!("{panic}"),
                }
            }
            result
        },
    );
    pooled.unwrap_or_else(|error| panic!("{error}"))
}
impl CatalogView for WorkerView {
    fn compile_unit(&self, active: &dyn MacroEvaluator, id: usize) -> Result<String, String> {
        self.install(Some(active))?;
        compile_unit_in(&self.shared, active, id, || self.analyze(id, Some(active)))
    }
    fn unit_source(&self, id: usize) -> Option<u32> {
        self.shared.unit_source(id)
    }
    fn analysis_sources(&self) -> Vec<u32> {
        self.shared.analysis_sources(self.thread)
    }
}
/// Install in `evaluator` the services through which its code reaches the
/// compilation: `__lightCompileUnit`, which demands units from `view`, and
/// `light.build`'s requests, which `build` serves and `log` records. The
/// view is weak: the catalog may own the engine.
pub(crate) fn install_catalog_services(
    evaluator: &Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
    view: std::rc::Weak<dyn CatalogView>,
    build: crate::build_services::SharedBuild,
    log: Rc<RefCell<Option<crate::dependency_manifest::ObservationLog>>>,
) -> Result<(), String> {
    // `light.build`: a request names the units whose initializers run; the
    // catalog knows the units being analyzed.
    let requests = view.clone();
    evaluator
        .install_string_service(
            "__lightBuildService",
            Rc::new(move |request| {
                let view = requests.upgrade();
                let analysis = view
                    .as_ref()
                    .map_or_else(Vec::new, |view| view.analysis_sources());
                let units = |unit| view.as_ref()?.unit_source(unit);
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
}
impl Drop for AnalysisGuard<'_> {
    fn drop(&mut self) {
        self.shared.settle(self.thread, self.id, self.state.take());
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
                assert!(!code.emit().1.is_empty());
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
        #[cfg(not(light_v8))]
        Rc::new(crate::evaluator::Evaluator::new(options).unwrap())
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
        RETAINED.set(Some(Vec::new()));
        let mut host = MacroHost::new();
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

    /// Hold each claimed unit's analysis until `waiters` threads wait for
    /// a unit.
    fn hold_claims(shared: &Arc<SharedCatalog>, waiters: usize) {
        let catalog = Arc::downgrade(shared);
        *shared.on_claim.lock().unwrap() = Some(Box::new(move |_| {
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
    fn a_wait_that_would_close_a_cycle_across_threads_reports_it_from_the_lowest_unit() {
        let (session, units, _) = registered("(defn a [] 1) (defn b [] 2) (defn c [] 3)");
        let (a, c) = (units["cljs.user/a"], units["cljs.user/c"]);
        assert!(a < c);
        let shared = session.shared().clone();
        let (first, second) = (shared.join(), shared.join());
        assert!(matches!(shared.claim(first, c), Ok(Claim::Analyze(..))));
        assert!(matches!(shared.claim(second, a), Ok(Claim::Analyze(..))));
        let error = std::thread::scope(|scope| {
            // `c`'s analysis demands `a`, which the second thread analyzes.
            let waiter = scope.spawn(|| shared.claim(first, a).map(drop));
            while shared.lock().waiting == 0 {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            // `a`'s analysis demands `c`: that wait would close the cycle,
            // which reads from `a`, the lower unit, whichever thread asks.
            let error = shared.claim(second, c).map(drop).unwrap_err();
            assert_eq!(
                error,
                "compile dependency cycle: cljs.user/a -> cljs.user/c -> cljs.user/a"
            );
            // The error fails `a`, and the waiting thread shares the failure.
            shared.settle(second, a, Some(AnalysisState::Failed(error.clone(), None)));
            assert_eq!(waiter.join().unwrap().unwrap_err(), error);
            error
        });
        shared.settle(first, c, Some(AnalysisState::Failed(error, None)));
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
        let source = (0..3)
            .map(|i| format!("(defn u{i} [x] (+ x {i}))"))
            .collect::<String>();
        let (session, units, environment) = registered(&source);
        let shared = session.shared().clone();
        hold_claims(&shared, 1);
        // A waiter wakes only once the application has taken the program.
        let catalog = Arc::downgrade(&shared);
        *shared.on_wake.lock().unwrap() = Some(Box::new(move |id| {
            let catalog = catalog.upgrade().unwrap();
            while !matches!(catalog.lock().units[id].state, AnalysisState::Prepared(..)) {
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
        }));
        for i in 0..3 {
            let id = units[&format!("cljs.user/u{i}")];
            let waited = std::thread::scope(|scope| {
                let waiter = scope.spawn(|| {
                    let host = MacroHost::bind(environment.clone(), None).unwrap();
                    let view = WorkerView::new(shared.clone(), host).unwrap();
                    view.analyze(id, None).map(|(program, _)| program)
                });
                // The coordinator analyzes the unit while the worker waits,
                // then its application takes the program and drops it
                // before the worker wakes.
                let (program, _) = session.analyze(id, None).unwrap();
                let code = crate::emitter::DeferredUnit::prepare(&program)
                    .code()
                    .to_owned();
                session.release_application_ir(id);
                drop(program);
                (waiter.join().unwrap(), code)
            });
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
        *shared.on_claim.lock().unwrap() = Some(Box::new(move |_| {
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
