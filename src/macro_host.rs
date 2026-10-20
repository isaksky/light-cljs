//! Compiler-owned macro requests and configuration. JavaScript expansion uses owned forms.
use std::collections::{HashMap, HashSet};

use crate::analyzer::ExprContext;
use crate::compiler_data::{Form, Name};
use crate::compiler_names::{core_macro, is_definition_macro};

/// Compiler import metadata. Filesystem imports share ordinary runtime aliases.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct MacroImport {
    pub namespace: String,
    pub alias: Option<String>,
    pub refers: Vec<String>,
}

/// Per-compilation expansion bounds, in addition to evaluator resource limits.
#[derive(Clone, Copy, Debug)]
pub struct MacroLimits {
    pub expansions: usize,
    pub generated_nodes: usize,
    pub expansion_depth: usize,
}
impl Default for MacroLimits {
    fn default() -> Self {
        Self {
            expansions: 1024,
            generated_nodes: 100_000,
            expansion_depth: 64,
        }
    }
}

/// The analysis nesting allowed under a macro expansion: deeper generated
/// runtime forms are an error.
pub(crate) const GENERATED_NESTING_LIMIT: usize = 128;

thread_local! {
    /// The macro-free unit this thread analyzes, if any ([`MacroFreeUnit`]).
    static MACRO_FREE_UNIT: std::cell::RefCell<Option<MacroFreeAnalysis>> =
        const { std::cell::RefCell::new(None) };
    /// How many JavaScript macro expansions run on this thread, nested.
    static EXPANDING: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

/// A native expansion in a macro-free unit's analysis: the nodes it
/// generated, and the operator as spelled and the span of the call, for the
/// limit error the expansion gives where the unit is charged.
#[derive(Clone, Debug)]
pub(crate) struct NativeExpansion {
    pub nodes: usize,
    pub operator: String,
    pub span: std::ops::Range<usize>,
}

/// What a macro-free unit's analysis would have charged to the
/// compilation, in order: a poll of the execution control at each form,
/// with the form's span, and each native expansion.
#[derive(Clone, Debug)]
pub(crate) enum UnitEvent {
    Poll(std::ops::Range<usize>),
    Expansion(NativeExpansion),
    /// A JavaScript macro's expansion in a unit a pool worker analyzed in
    /// its own engine (light-cljs-339), with the nodes it generated and
    /// the call's span, and the operator as spelled and the call's span
    /// where the analyzer expanded it ([`ExpansionSite`]), whose limit
    /// error names the operator.
    Script {
        nodes: usize,
        span: std::ops::Range<usize>,
        site: Option<(String, std::ops::Range<usize>)>,
    },
    /// While artifacts are captured (light-cljs-374): the namespace's node
    /// budget had to leave room for this many nodes beyond those generated
    /// so far ([`WorkMarks::node_peak`]).
    Room(usize),
    /// [`WorkMarks::conversion_peak`] reached this.
    Conversion(usize),
    /// A measurement failed ([`WorkMarks::unmeasured`]).
    Unmeasured,
    /// A source macro call expanded, for the artifact's provenance.
    Captured(Box<crate::compilation_session::CapturedExpansion>),
}

/// What a recorded unit's analysis records beyond its charges: what the
/// compilation records of every analysis, which a pool worker hands to the
/// coordinator (docs/compiler-concurrency.md, "As built (light-cljs-374)").
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct UnitRecording {
    /// A dependency manifest is recorded: the analysis's observations.
    pub observe: bool,
    /// Artifacts are captured: its source macro calls and its work marks.
    pub capture: bool,
}

struct MacroFreeAnalysis {
    name: String,
    /// Whether the unit was found macro-free, so that entering an engine
    /// is a bug; a unit a worker analyzes in its own engine records only.
    forbid: bool,
    events: Vec<UnitEvent>,
    /// The macro call the analyzer expands ([`ExpansionSite`]).
    site: Option<(String, std::ops::Range<usize>)>,
    capture: bool,
    /// Whether its form polls are recorded for its charge rather than
    /// polled: not for a unit the coordinator keeps at a dispatch, which
    /// polls its own control as it runs (light-cljs-391).
    polls: bool,
    /// What the analysis observed, while a manifest is recorded.
    observations: Option<crate::dependency_manifest::ObservationLog>,
}

/// What a recorded unit's analysis recorded, in order ([`MacroFreeUnit::finish`]).
pub(crate) struct RecordedUnit {
    pub events: Vec<UnitEvent>,
    pub observations: Option<Box<crate::dependency_manifest::ObservationLog>>,
}

/// While alive, the analysis on this thread is of a unit found macro-free
/// at registration (docs/compiler-concurrency.md, "As built
/// (light-cljs-338)"): an entry into a compile-time engine, or an effect on
/// the host that a pool worker would not hand back, is a bug in the
/// eligibility check and panics with the unit named
/// ([`forbid_engine_entry`]); and its polls and native expansions are
/// recorded instead of charged ([`record_poll`], [`record_native_expansion`]),
/// which [`Self::finish`] returns.
pub(crate) struct MacroFreeUnit(Option<MacroFreeAnalysis>);
impl MacroFreeUnit {
    /// Record the analysis of unit `name` on this thread; with `forbid`, it
    /// was found macro-free and must not enter an engine. `recording` is
    /// what the compilation records of it beyond its charges.
    pub(crate) fn enter(name: String, forbid: bool, recording: UnitRecording, polls: bool) -> Self {
        Self(MACRO_FREE_UNIT.with(|unit| {
            unit.replace(Some(MacroFreeAnalysis {
                name,
                forbid,
                polls,
                events: Vec::new(),
                site: None,
                capture: recording.capture,
                observations: recording.observe.then(Default::default),
            }))
        }))
    }
    /// Whether this thread records a unit's analysis, whose nested demands
    /// it then records too, in their place.
    pub(crate) fn active() -> bool {
        MACRO_FREE_UNIT.with(|unit| unit.borrow().is_some())
    }
    /// What the analysis recorded, in order.
    pub(crate) fn finish(mut self) -> RecordedUnit {
        let previous = self.0.take();
        let analysis = MACRO_FREE_UNIT.with(|unit| unit.replace(previous));
        let (events, observations) =
            analysis.map_or_else(Default::default, |a| (a.events, a.observations));
        RecordedUnit {
            events,
            observations: observations.map(Box::new),
        }
    }
    /// Whether this thread records a unit's analysis for a compilation that
    /// captures artifacts, or `None` if it records none.
    pub(crate) fn captures() -> Option<bool> {
        MACRO_FREE_UNIT.with(|unit| unit.borrow().as_ref().map(|analysis| analysis.capture))
    }
}
/// Log an observation for the dependency manifest in the unit this thread
/// records, if it records one: `record` back if it does not, for the
/// compilation's log. A unit recorded while no manifest is, logs nothing.
pub(crate) fn record_observation<F>(record: F) -> Option<F>
where
    F: FnOnce(&mut crate::dependency_manifest::ObservationLog),
{
    MACRO_FREE_UNIT.with(|unit| match &mut *unit.borrow_mut() {
        Some(analysis) => {
            if let Some(log) = &mut analysis.observations {
                record(log);
            }
            None
        }
        None => Some(record),
    })
}
/// Add `log`, what the coordinator recorded of a `light.build` request it
/// answered for this thread, to the unit this thread records.
pub(crate) fn record_observations(log: crate::dependency_manifest::ObservationLog) {
    MACRO_FREE_UNIT.with(|unit| {
        if let Some(analysis) = &mut *unit.borrow_mut()
            && let Some(observations) = &mut analysis.observations
        {
            observations.absorb(log);
        }
    });
}
/// Record `event`, one of the capture's, in the unit this thread records
/// for a capturing compilation: whether this thread records a unit.
fn record_capture(event: impl FnOnce() -> UnitEvent) -> bool {
    MACRO_FREE_UNIT.with(|unit| match &mut *unit.borrow_mut() {
        Some(analysis) => {
            if analysis.capture {
                analysis.events.push(event());
            }
            true
        }
        None => false,
    })
}
impl Drop for MacroFreeUnit {
    fn drop(&mut self) {
        let previous = self.0.take();
        MACRO_FREE_UNIT.with(|unit| *unit.borrow_mut() = previous);
    }
}
/// Panic if this thread analyzes a macro-free unit: `what` reached the
/// macro host.
pub(crate) fn forbid_engine_entry(what: &str) {
    MACRO_FREE_UNIT.with(|unit| {
        if let Some(analysis) = &*unit.borrow()
            && analysis.forbid
        {
            panic!(
                "compile unit {} was dispatched as macro-free, but its analysis reached the macro host ({what}): the eligibility check is wrong",
                analysis.name
            );
        }
    });
}
/// Record a native expansion if this thread analyzes a macro-free unit.
pub(crate) fn record_native_expansion(expansion: impl FnOnce() -> NativeExpansion) {
    MACRO_FREE_UNIT.with(|unit| {
        if let Some(analysis) = &mut *unit.borrow_mut() {
            analysis.events.push(UnitEvent::Expansion(expansion()));
        }
    });
}
/// Record a JavaScript macro's expansion if this thread records a unit's
/// analysis.
pub(crate) fn record_script_expansion(nodes: usize, span: impl FnOnce() -> std::ops::Range<usize>) {
    MACRO_FREE_UNIT.with(|unit| {
        if let Some(analysis) = &mut *unit.borrow_mut() {
            analysis.events.push(UnitEvent::Script {
                nodes,
                span: span(),
                site: analysis.site.clone(),
            });
        }
    });
}
/// While alive, the analyzer expands the macro call `operator` (as
/// spelled) spanning `span` on this thread, whose limit errors it reports
/// as `while expanding operator: ...` at that span; a recorded JavaScript
/// expansion keeps it, so that its charge fails with the same error
/// ([`UnitEvent::Script`]). It records nothing unless this thread records
/// a unit's analysis.
pub(crate) struct ExpansionSite(Option<Option<(String, std::ops::Range<usize>)>>);
impl ExpansionSite {
    pub(crate) fn enter(operator: &str, span: &std::ops::Range<usize>) -> Self {
        Self(MACRO_FREE_UNIT.with(|unit| {
            unit.borrow_mut()
                .as_mut()
                .map(|analysis| analysis.site.replace((operator.to_owned(), span.clone())))
        }))
    }
}
impl Drop for ExpansionSite {
    fn drop(&mut self) {
        if let Some(previous) = self.0.take() {
            MACRO_FREE_UNIT.with(|unit| {
                if let Some(analysis) = &mut *unit.borrow_mut() {
                    analysis.site = previous;
                }
            });
        }
    }
}
/// Record a poll at a form spanning `span` if this thread analyzes a
/// macro-free unit, which then does not poll: whether it recorded one.
pub(crate) fn record_poll(span: impl FnOnce() -> std::ops::Range<usize>) -> bool {
    MACRO_FREE_UNIT.with(|unit| match &mut *unit.borrow_mut() {
        Some(analysis) if analysis.polls => {
            analysis.events.push(UnitEvent::Poll(span()));
            true
        }
        _ => false,
    })
}

/// High-water marks of a compilation's logical work, shared by its analysis
/// frames like the expansion and node counters, which namespace capture
/// reads as each namespace's accounting (docs/compiler-cache.md, "Loading,
/// diagnostics, and storage"). Each depth is the value a limit check
/// compared, plus one, so a namespace that needs a depth of `n` is admitted
/// by a limit of at least `n`, and one that checked none needs nothing.
#[derive(Default)]
pub(crate) struct WorkMarks {
    /// The most generated nodes the budget had to leave room for: the
    /// nodes generated before an expansion plus the largest form one of its
    /// conversions took, which the remaining budget bounds as well.
    pub node_peak: std::cell::Cell<usize>,
    /// [`MacroLimits::expansion_depth`] as the analyzer checks it.
    pub macro_depth: std::cell::Cell<usize>,
    /// [`GENERATED_NESTING_LIMIT`].
    pub analysis_depth: std::cell::Cell<usize>,
    /// [`MacroLimits::expansion_depth`] as the session checks nested
    /// deferred-unit analyses.
    pub unit_depth: std::cell::Cell<usize>,
    /// The expansions installed artifacts charged, which count against
    /// [`MacroLimits::expansions`] but were not executed.
    pub charged_expansions: std::cell::Cell<usize>,
    /// The largest conversion of a compiler data reader's input or result:
    /// these take at most [`MacroLimits::generated_nodes`] each, not what is
    /// left of it.
    pub conversion_peak: std::cell::Cell<usize>,
    /// Whether expansions measure their conversions for `node_peak`, which
    /// only a capturing compilation needs.
    pub measuring: std::cell::Cell<bool>,
    /// A measurement failed, so the namespace's accounting is not recorded
    /// and its artifact is never installed.
    pub unmeasured: std::cell::Cell<bool>,
    /// The macro evaluator's `takeEncodePeak` ([`take_encode_peak`]).
    encode_peak: std::cell::Cell<Option<crate::evaluator::ValueId>>,
    /// The node counts of recent large environment entries, by identity;
    /// holding the datum keeps its address from being reused.
    sizes: std::cell::RefCell<Vec<(crate::compiler_data::Datum, usize)>>,
}

/// The compilation's counters where a namespace's work began
/// ([`MacroHost::begin_namespace_work`]).
#[derive(Clone, Copy, Debug)]
pub(crate) struct WorkStart {
    pub expansions: usize,
    pub generated_nodes: usize,
}

/// The most nodes that any encoding of compile-time data on `evaluator`
/// charged its node limit since the last call, which the JavaScript side of
/// a conversion counts per sequence cell and lazy realization step, so a
/// lazy or wide result costs more than the nodes it exports (the
/// compile-time runtime's `takeEncodePeak`, written by build.rs). The
/// function is evaluated once per JavaScript session and kept in `cached`.
/// None if it cannot be read.
fn take_encode_peak(
    evaluator: &dyn crate::evaluator::MacroEvaluator,
    cached: &std::cell::Cell<Option<crate::evaluator::ValueId>>,
) -> Option<usize> {
    use crate::compiler_data::{Datum, DatumKind};
    let read = |function| -> Result<Option<usize>, crate::evaluator::EvalError> {
        let value = evaluator.call(function, &[])?;
        let source = crate::compiler_data::Source::new("<encode peak>", "");
        let span = source.span(0..0).expect("an empty span");
        let call = Form::from_data(
            Datum::new(DatumKind::Nil),
            span.clone(),
            crate::compiler_data::Origin::SuppliedData,
        );
        let origin = crate::compiler_data::ExpansionOrigin {
            macro_name: Name::new(None, "encode-peak"),
            call: span,
            definition: None,
            helpers: Vec::new(),
        };
        let limits = crate::form_bridge::ConversionLimits { nodes: 1, depth: 1 };
        let exported = evaluator.export_form(value, &call, origin, limits);
        let _ = evaluator.release(value);
        Ok(match exported?.data().kind() {
            DatumKind::Integer(peak) => usize::try_from(*peak).ok(),
            DatumKind::Double(peak) if *peak >= 0.0 => Some(*peak as usize),
            _ => None,
        })
    };
    // A function of another session, such as an evaluator's that was
    // replaced, is refused without running anything: evaluate it again.
    if let Some(function) = cached.get()
        && let Ok(peak) = read(function)
    {
        return peak;
    }
    let function = evaluator
        .evaluate_unit("rt => rt.compilerForms.takeEncodePeak")
        .ok()?;
    cached.set(Some(function));
    read(function).ok().flatten()
}

impl WorkMarks {
    /// Raise `mark` to `needed`.
    pub(crate) fn note(mark: &std::cell::Cell<usize>, needed: usize) {
        if needed > mark.get() {
            mark.set(needed);
        }
    }

    /// The nodes converting `data` takes: one per datum, its metadata and
    /// compiler payloads included, as the form bridges count them.
    pub(crate) fn nodes(data: &crate::compiler_data::Datum) -> usize {
        use crate::compiler_data::DatumKind;
        let mut count = 1;
        if let Some(metadata) = data.metadata() {
            count += Self::nodes(&metadata);
        }
        match data.kind() {
            DatumKind::Collection { children, .. } => {
                count += children.iter().map(Self::nodes).sum::<usize>();
            }
            DatumKind::CompilerLiteral { payload, .. } => count += Self::nodes(payload),
            _ => {}
        }
        count
    }

    /// [`Self::nodes`] of a macro environment, whose large entries (the
    /// macro inventory, the namespace and the locals) are shared by the
    /// expansions that see them, so their counts are kept by identity.
    /// The declared names the reader context of `environment` leaves out
    /// because the engine holds them already (its `:declared` `:from`,
    /// [`MacroHost::reader_environment`]): the nodes its conversion would
    /// take more in an engine that holds none. Measures count them, so that
    /// they do not depend on the engine's history, which differs by isolate
    /// (light-cljs-374).
    pub(crate) fn declared_elsewhere(environment: &crate::compiler_data::Datum) -> usize {
        use crate::compiler_data::{DataBuilder, DatumKind};
        let key = |ns, name| DataBuilder.keyword(Name::new(ns, name));
        environment
            .get(&key(Some("light-cljs"), "reader-context"))
            .ok()
            .flatten()
            .and_then(|context| context.get(&key(None, "declared")).ok().flatten())
            .and_then(|declared| declared.get(&key(None, "from")).ok().flatten())
            .and_then(|from| match from.kind() {
                DatumKind::Integer(from) => usize::try_from(*from).ok(),
                _ => None,
            })
            .unwrap_or(0)
    }

    fn environment_nodes(&self, environment: &crate::compiler_data::Datum) -> usize {
        use crate::compiler_data::DatumKind;
        let DatumKind::Collection { children, .. } = environment.kind() else {
            return Self::nodes(environment);
        };
        if environment.metadata().is_some() {
            return Self::nodes(environment);
        }
        let mut sizes = self.sizes.borrow_mut();
        let mut count = 1;
        for child in children {
            let identity = child.identity();
            count += match sizes.iter().find(|(data, _)| data.identity() == identity) {
                Some(&(_, size)) => size,
                None => {
                    let size = Self::nodes(child);
                    if size >= 64 {
                        if sizes.len() == 16 {
                            sizes.remove(0);
                        }
                        sizes.push((child.clone(), size));
                    }
                    size
                }
            };
        }
        count
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub(crate) enum SourceBindingKind {
    Value,
    Macro,
}

#[derive(PartialEq)]
struct NamespaceEnvironmentKey {
    namespace: String,
    aliases: Vec<(String, String)>,
    requires: Vec<String>,
    imports: Vec<MacroImport>,
    renames: Vec<(String, String, String)>,
    namespace_imports: Vec<(String, String)>,
    symbol: Option<usize>,
    doc: Option<usize>,
}
struct NamespaceEnvironment {
    key: NamespaceEnvironmentKey,
    data: crate::compiler_data::Datum,
    // The data owns symbol/doc, so their addresses cannot be reused while cached.
}

type LocalDescription = (String, usize, &'static str, Option<usize>);
struct LocalEnvironment {
    locals: Vec<LocalDescription>,
    // Per binding, including shadowed ones, so extending or leaving a lexical
    // scope can reuse the unchanged prefix without retaining a map per prefix.
    rows: Vec<(crate::compiler_data::Datum, crate::compiler_data::Datum)>,
    data: crate::compiler_data::Datum,
}

#[derive(PartialEq)]
struct ReaderEnvironmentKey {
    refers: Vec<(String, String, String)>,
    excludes: Vec<String>,
    generation: u64,
    from: usize,
    names: Vec<String>,
    visible: usize,
    bindings: Vec<String>,
}
struct ReaderEnvironment {
    key: ReaderEnvironmentKey,
    data: crate::compiler_data::Datum,
}

/// Macro configuration and compiler metadata. Each clean compilation owns a fresh
/// QuickJS bootstrap session; reusing this host never reuses JavaScript macro roots.
///
/// A host has two parts (docs/compiler-concurrency.md, "Thread safety
/// today"). Its settings and [`MacroTables`] are sendable data, which a
/// worker receives as a [`MacroEnvironment`]; the settings stay fields of
/// the host, so that callers can set them apart. Its [`EngineBinding`] is
/// the per-thread state bound to one compile-time engine, which never
/// leaves the thread that made it. A worker builds its own host from an
/// environment and its own evaluator ([`Self::bind`]).
#[derive(Default)]
pub struct MacroHost {
    pub limits: MacroLimits,
    /// Configuration only: JS roots belong to each fresh compilation session.
    pub evaluator_options: crate::evaluator::EvaluatorOptions,
    /// Give compile-time heaps an opaque JavaScript package registry: every
    /// package, export, call and construction answers an inert stub, so the
    /// initializers that macro expansion demands (such as an extension whose
    /// target type a package builds) run without the host's packages, which
    /// only the application has.
    pub opaque_packages: bool,
    pub(crate) tables: MacroTables,
    pub(crate) engine: EngineBinding,
}

/// The sendable part of a [`MacroHost`], which a worker receives: the
/// host's settings and tables as an analysis frame sees them
/// ([`MacroHost::environment`]).
#[derive(Clone)]
pub(crate) struct MacroEnvironment {
    limits: MacroLimits,
    evaluator_options: crate::evaluator::EvaluatorOptions,
    opaque_packages: bool,
    tables: MacroTables,
}
impl MacroEnvironment {
    /// The options a worker's own engine is made with.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn evaluator_options(&self) -> &crate::evaluator::EvaluatorOptions {
        &self.evaluator_options
    }
    /// The limits the host was made with.
    pub(crate) fn limits(&self) -> &MacroLimits {
        &self.limits
    }
}

/// The layered tables of declared imports, macros and bindings, the flags
/// and the sources of the compiler data readers. They hold no engine value
/// that could be used from another thread: a source macro's function is a
/// value ID scoped to the engine that evaluated it, which another engine
/// refuses.
#[derive(Clone, Default)]
pub(crate) struct MacroTables {
    data_readers: std::sync::Arc<DataReaderSources>,
    imports: crate::layered::Layered<String, Vec<MacroImport>>,
    renames: crate::layered::Layered<String, Vec<(String, String, String)>>,
    namespace_symbols: crate::layered::Layered<String, crate::compiler_data::Datum>,
    namespace_docs: crate::layered::Layered<String, crate::compiler_data::Datum>,
    /// By `namespace/name`, whether the Var is private.
    runtime_vars: crate::layered::Layered<String, bool>,
    namespace_imports: crate::layered::Layered<String, Vec<(String, String)>>,
    assertions_disabled: bool,
    pub(crate) interactive: bool,
    pub(crate) name_policy: Option<crate::eval_compiler::NamePolicy>,
    pub(crate) bootstrap_seed: bool,
    pub(crate) javascript_macros:
        crate::layered::Layered<String, crate::compiler_bootstrap::CompiledMacro>,
    /// The names of `javascript_macros` by namespace, sorted
    /// ([`MacroHost::source_macro_names`]); change both through
    /// [`MacroHost::insert_javascript_macro`].
    macro_names: crate::layered::Layered<String, Vec<String>>,
    pub(crate) source_bindings: crate::layered::Layered<String, SourceBindingKind>,
    /// Which inventory of `javascript_macros` this is, unique in the
    /// process ([`next_macro_version`]), which a macro environment cache
    /// is made from.
    pub(crate) macro_version: u64,
    /// The compilation whose declared bindings the session holds.
    pub(crate) reader_generation: u64,
    /// The build configuration `light.build/config` answers
    /// ([`MacroHost::set_build_configuration`]).
    pub(crate) configuration: std::sync::Arc<crate::build_services::BuildConfiguration>,
}

/// The per-thread part of a [`MacroHost`]: its compile-time engines, the
/// session they serve, and the caches and counters its analysis frames
/// share. Neither `Send` nor `Sync`: it is made, used and dropped on one
/// thread.
#[derive(Default)]
pub(crate) struct EngineBinding {
    /// The data-reader engine and the reader functions evaluated in it.
    data_readers: std::rc::Rc<std::cell::RefCell<DataReaderEngine>>,
    pub(crate) interactive_control: Option<crate::evaluator::ExecutionControl>,
    pub(crate) bootstrap: Option<crate::compiler_bootstrap::Bootstrap>,
    /// Shared by frames with the same macro inventory; replaced on declaration.
    pub(crate) macro_environment:
        std::rc::Rc<std::cell::RefCell<Option<(u64, crate::compiler_data::Datum)>>>,
    namespace_environment: std::rc::Rc<std::cell::RefCell<Option<NamespaceEnvironment>>>,
    // Shared by analysis frames, but bounded to one complete lexical view.
    local_environment: std::rc::Rc<std::cell::RefCell<Option<LocalEnvironment>>>,
    reader_environment: std::rc::Rc<std::cell::RefCell<Option<ReaderEnvironment>>>,
    pub(crate) session: Option<std::rc::Rc<crate::compilation_session::CompilationSession>>,
    pub(crate) session_ref: std::rc::Weak<crate::compilation_session::CompilationSession>,
    pub(crate) expansions: std::rc::Rc<std::cell::Cell<usize>>,
    pub(crate) generated_nodes: std::rc::Rc<std::cell::Cell<usize>>,
    /// The compilation's high-water marks and charges, shared like the
    /// counters ([`WorkMarks`]).
    pub(crate) work: std::rc::Rc<WorkMarks>,
    /// The engine clean compilations run their macros in instead of the
    /// built-in ones ([`MacroHost::set_script_host`]).
    pub(crate) script_host: Option<std::rc::Rc<dyn crate::host_evaluator::ScriptHost>>,
    /// Per namespace, the globals registered as its declared bindings in the
    /// compile-time session: the position after the last one registered and
    /// how many of the namespace's own globals were among them. Shared by
    /// the analysis frames of one compilation.
    pub(crate) reader_declared: std::rc::Rc<std::cell::RefCell<HashMap<String, (usize, usize)>>>,
    /// While a dependency manifest is recorded, what analysis observes
    /// beyond the `ns` forms; shared by the analysis frames of the
    /// compilation ([`crate::dependency_manifest`]).
    pub(crate) dependency_log:
        std::rc::Rc<std::cell::RefCell<Option<crate::dependency_manifest::ObservationLog>>>,
    /// The compilation `light.build` serves, shared by the analysis frames
    /// and the services installed in its heaps ([`crate::build_services`]).
    pub(crate) build: crate::build_services::SharedBuild,
    /// Whether the engine is a worker's ([`MacroHost::bind`]), in which the
    /// source macros declared in the coordinator's engine are evaluated
    /// again, once each, into `foreign_macros`, by the identity of their
    /// code (docs/compiler-concurrency.md, "As built (light-cljs-339)").
    pub(crate) foreign: bool,
    pub(crate) foreign_macros:
        std::rc::Rc<std::cell::RefCell<HashMap<usize, crate::evaluator::ValueId>>>,
    /// A worker's catalog, which has no session: how many extension
    /// bundles it registered is the bootstrap macros' frontier.
    pub(crate) catalog: Option<std::sync::Arc<crate::compilation_session::SharedCatalog>>,
    /// A worker's view, through which a definition's `:test` unit is
    /// analyzed there ([`crate::compilation_session::analyze_definition_test`]).
    pub(crate) worker: Option<std::rc::Weak<crate::compilation_session::WorkerView>>,
    /// Keeps the binding on its thread whatever its fields become.
    _thread: std::marker::PhantomData<std::rc::Rc<()>>,
}

// A worker receives a `MacroEnvironment`; an `EngineBinding` never moves
// (docs/compiler-concurrency.md, "Thread safety today"). Both hold in every
// build, not only test builds.
const _: () = {
    const fn send_sync<T: Send + Sync>() {}
    send_sync::<MacroEnvironment>();
};
const _: fn() = || {
    // Ambiguous, and so an error, if the binding were `Send`, or `Sync`.
    trait AmbiguousIfSend<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSend<()> for T {}
    struct IsSend;
    impl<T: ?Sized + Send> AmbiguousIfSend<IsSend> for T {}
    let _ = <EngineBinding as AmbiguousIfSend<_>>::check;
    trait AmbiguousIfSync<A> {
        fn check() {}
    }
    impl<T: ?Sized> AmbiguousIfSync<()> for T {}
    struct IsSync;
    impl<T: ?Sized + Sync> AmbiguousIfSync<IsSync> for T {}
    let _ = <EngineBinding as AmbiguousIfSync<_>>::check;
};

impl EngineBinding {
    /// What an analysis frame shares with the binding it was taken from:
    /// its engines, caches and counters, and its session, which the frame
    /// reaches weakly.
    pub(crate) fn frame(&self) -> Self {
        Self {
            data_readers: self.data_readers.clone(),
            interactive_control: self.interactive_control.clone(),
            bootstrap: self.bootstrap.clone(),
            macro_environment: self.macro_environment.clone(),
            namespace_environment: self.namespace_environment.clone(),
            local_environment: self.local_environment.clone(),
            reader_environment: self.reader_environment.clone(),
            session: None,
            session_ref: self
                .session_handle()
                .map_or_else(std::rc::Weak::new, |s| std::rc::Rc::downgrade(&s)),
            expansions: self.expansions.clone(),
            generated_nodes: self.generated_nodes.clone(),
            work: self.work.clone(),
            script_host: self.script_host.clone(),
            reader_declared: self.reader_declared.clone(),
            dependency_log: self.dependency_log.clone(),
            build: self.build.clone(),
            foreign: self.foreign,
            foreign_macros: self.foreign_macros.clone(),
            catalog: self.catalog.clone(),
            worker: self.worker.clone(),
            _thread: std::marker::PhantomData,
        }
    }
    pub(crate) fn session_handle(
        &self,
    ) -> Option<std::rc::Rc<crate::compilation_session::CompilationSession>> {
        self.session.clone().or_else(|| self.session_ref.upgrade())
    }
    /// How many extension bundles the compilation registered, from its
    /// session or, on a worker, its catalog; 0 outside a compilation.
    pub(crate) fn extension_frontier(&self) -> usize {
        match (&self.catalog, self.session_handle()) {
            (_, Some(session)) => session.extension_frontier(),
            (Some(catalog), None) => catalog.extension_frontier(),
            (None, None) => 0,
        }
    }
}
/// A namespace's entries in the macro host's tables, borrowed.
pub(crate) struct NamespaceDeclarations<'a> {
    pub imports: &'a [MacroImport],
    pub renames: &'a [(String, String, String)],
    pub namespace_imports: &'a [(String, String)],
    pub doc: Option<&'a crate::compiler_data::Datum>,
}

/// A name's namespace and name as text, for a cache key.
fn name_parts(name: &Name) -> (Option<String>, String) {
    (
        name.namespace().map(String::from_utf16_lossy),
        String::from_utf16_lossy(name.name()),
    )
}

/// Every compilation in the process gets its own reader generation, so a
/// fresh host never reuses the declarations an earlier one registered.
/// Namespace artifacts never retain it ([`crate::generated_names`]).
static READER_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// A version for a changed macro inventory ([`MacroTables::macro_version`]).
fn next_macro_version() -> u64 {
    static VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// The declarations a macro environment names for its session, recorded with
/// [`MacroHost::register_reader_declarations`] once the environment reached it.
#[derive(Clone, Debug)]
pub(crate) struct ReaderRegistration {
    namespace: String,
    /// The globals up to this position are registered.
    position: usize,
    /// How many of them the namespace declared.
    visible: usize,
}

/// The compiler data readers' emitted units, by tag, and the default one's.
#[derive(Clone, Default)]
struct DataReaderSources {
    units: HashMap<Name, String>,
    default_unit: Option<String>,
}
/// The data-reader heap of one binding and the functions evaluated in it.
#[derive(Default)]
struct DataReaderEngine {
    default_function: Option<crate::evaluator::ValueId>,
    evaluator: Option<std::rc::Rc<crate::evaluator::Evaluator>>,
    functions: HashMap<Name, crate::evaluator::ValueId>,
    /// The evaluator's `takeEncodePeak` ([`take_encode_peak`]).
    encode_peak: std::cell::Cell<Option<crate::evaluator::ValueId>>,
}

/// The compile-time package registry of [`MacroHost::opaque_packages`]: a Proxy
/// whose packages are stubs. A stub is a function whose every property is a
/// stub (its `prototype` is a real object, so extensions can install methods),
/// whose calls and constructions return stubs, and which prints as text.
const OPAQUE_PACKAGES_PRELUDE: &str = r#"rt => {
  const stub = () => {
    const target = function lightCljsPackageStub() {};
    return new Proxy(target, {
      get: (t, p) => {
        if (p === "prototype") return t.prototype;
        if (p === Symbol.toPrimitive || p === "toString" || p === "valueOf") return () => "[light-cljs compile-time package stub]";
        if (typeof p === "symbol") return undefined;
        return stub();
      },
      apply: () => stub(),
      construct: () => stub(),
      has: () => true,
    });
  };
  globalThis.lightCljsPackages = new Proxy({}, { get: (t, p) => typeof p === "symbol" ? undefined : stub(), has: () => true });
  return null;
}"#;

impl MacroHost {
    /// Collect the garbage of the host's compile-time engines in full,
    /// between operations: its macro engine, once started, and the engine
    /// of its compiler data readers, once the macro engine has released the
    /// functions of macros no declaration holds.
    pub(crate) fn collect_garbage(&self) {
        if let Some(bootstrap) = &self.engine.bootstrap {
            bootstrap.release_dropped();
            bootstrap.evaluator.collect_garbage();
        }
        if let Some(evaluator) = &self.engine.data_readers.borrow().evaluator {
            evaluator.collect_garbage();
        }
    }
    /// The values the host's macro engine roots for their `ValueId`s
    /// ([`crate::compiler_evaluator::CompilerEvaluator::value_roots`]), 0
    /// before it starts, once it has released the functions of macros no
    /// declaration holds.
    pub(crate) fn value_roots(&self) -> Result<usize, crate::evaluator::EvalError> {
        match &self.engine.bootstrap {
            Some(bootstrap) => {
                bootstrap.release_dropped();
                bootstrap.evaluator.value_roots()
            }
            None => Ok(0),
        }
    }
    pub(crate) fn new_compiler_evaluator(
        &self,
    ) -> Result<
        std::rc::Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
        crate::evaluator::EvalError,
    > {
        if let Some(host) = &self.engine.script_host
            && !self.tables.interactive
        {
            let evaluator =
                crate::host_evaluator::HostEvaluator::new(host.clone(), &self.evaluator_options)?;
            self.prepare_compiler_evaluator(&evaluator)?;
            return Ok(std::rc::Rc::new(evaluator));
        }
        #[cfg(light_v8)]
        if !self.tables.interactive {
            // The main isolate of a build with several unit slots shares
            // the build's deadline and memory with its workers.
            let session = self.session_handle();
            let evaluator = match session.as_ref().and_then(|s| s.shared().resources()) {
                Some(resources) => crate::v8_evaluator::V8Evaluator::new_for_build(
                    self.evaluator_options.clone(),
                    resources,
                    &resources.control,
                )?,
                None => crate::v8_evaluator::V8Evaluator::new(self.evaluator_options.clone())?,
            };
            self.prepare_compiler_evaluator(&evaluator)?;
            return Ok(std::rc::Rc::new(evaluator));
        }
        Ok(std::rc::Rc::new(self.new_evaluator()?))
    }
    /// Give a fresh compile-time engine this host's settings: its
    /// `*assert*` and, with [`MacroEnvironment::opaque_packages`], the
    /// opaque package registry.
    fn prepare_compiler_evaluator(
        &self,
        evaluator: &dyn crate::evaluator::MacroEvaluator,
    ) -> Result<(), crate::evaluator::EvalError> {
        if self.tables.assertions_disabled {
            let done =
                evaluator.evaluate_unit("rt => { rt.core.assertions = false; return null; }")?;
            evaluator.release(done)?;
        }
        if self.opaque_packages {
            let done = evaluator.evaluate_unit(OPAQUE_PACKAGES_PRELUDE)?;
            evaluator.release(done)?;
        }
        Ok(())
    }
    /// Allocate a fresh synchronous heap for a clean compilation. Its dynamic
    /// `*assert*` follows [`Self::set_assertions`], so the macros, functions and
    /// data readers it runs read the setting, as upstream's `:elide-asserts`
    /// binds `clojure.core/*assert*` while compiling.
    pub fn new_evaluator(
        &self,
    ) -> Result<crate::evaluator::Evaluator, crate::evaluator::EvalError> {
        let evaluator = if let Some(control) = &self.engine.interactive_control {
            crate::evaluator::Evaluator::interactive_with_control(
                self.evaluator_options.clone(),
                control.clone(),
            )
        } else if self.tables.interactive {
            crate::evaluator::Evaluator::interactive(self.evaluator_options.clone())
        } else {
            crate::evaluator::Evaluator::new(self.evaluator_options.clone())
        }?;
        self.prepare_compiler_evaluator(&evaluator)?;
        Ok(evaluator)
    }

    /// Execute a compiler-emitted macro function with hidden &form/&env arguments.
    /// Function handles are generation-scoped IDs, never engine values in Program.
    pub fn expand_javascript(
        &mut self,
        evaluator: &dyn crate::evaluator::MacroEvaluator,
        function: crate::evaluator::ValueId,
        call: &Form,
        environment: &crate::compiler_data::Datum,
        origin: crate::compiler_data::ExpansionOrigin,
    ) -> Result<Form, String> {
        if self.engine.expansions.get() >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        self.engine.expansions.set(self.engine.expansions.get() + 1);
        // The conversions below take at most the node limit each, not
        // what the compilation has left of it, so that whether they fit
        // depends on the expansion alone and not on the units charged
        // before it, wherever it runs (docs/compiler-concurrency.md, "As
        // built (light-cljs-341)"); what they generate is charged after.
        // The cache's accounting keeps the stricter measure: a namespace
        // needs room for the largest of its inputs too.
        let measuring = self.measures_work();
        if measuring {
            let input = WorkMarks::nodes(&call.data()).max(
                self.engine.work.environment_nodes(environment)
                    + WorkMarks::declared_elsewhere(environment),
            );
            self.note_node_room(input);
            // What the engine's conversions charged before this expansion,
            // outside every other one, is not its own: the engine's history,
            // which differs by isolate (light-cljs-374).
            if EXPANDING.get() == 0
                && take_encode_peak(evaluator, &self.engine.work.encode_peak).is_none()
            {
                self.note_unmeasured();
            }
        }
        let limits = crate::form_bridge::ConversionLimits {
            nodes: self.limits.generated_nodes,
            depth: 128,
        };
        let run = || -> Result<Form, crate::evaluator::EvalError> {
            // Its `gensym` names belong to the unit that expands it.
            crate::generated_names::sync_gensym_scope(evaluator)?;
            let mut args = Vec::new();
            let mut result = None;
            let expanded = (|| {
                args.push(evaluator.import_macro_form(call, limits)?);
                args.push(evaluator.import_environment(environment, limits)?);
                args.extend(evaluator.macro_arguments(args[0], call, limits)?);
                let value = evaluator.call(function, &args)?;
                result = Some(value);
                evaluator.export_form(value, call, origin.clone(), limits)
            })();
            // Root release is best effort after terminal engine failure.
            // Ordinary exceptions must not leave successfully imported args
            // rooted for the remainder of an interactive compilation.
            if let Some(value) = result {
                let _ = evaluator.release(value);
            }
            for id in args {
                let _ = evaluator.release(id);
            }
            expanded
        };
        let expanded = {
            EXPANDING.set(EXPANDING.get() + 1);
            let expanded = run();
            EXPANDING.set(EXPANDING.get() - 1);
            expanded
        };
        // What the conversions charged on the JavaScript side, which bounds
        // them by the same remaining budget.
        if measuring && expanded.is_ok() {
            match take_encode_peak(evaluator, &self.engine.work.encode_peak) {
                Some(peak) => self.note_node_room(peak),
                None => self.note_unmeasured(),
            }
        }
        let expanded = expanded.map_err(|e| {
            let location = crate::compiler_data::DiagnosticLocation::from_span(&origin.call);
            let mut message = format!(
                "{}:{}:{}: macro {}/{}: {}",
                location.source,
                location.line,
                location.column,
                String::from_utf16_lossy(origin.macro_name.namespace().unwrap_or(&[])),
                String::from_utf16_lossy(origin.macro_name.name()),
                e
            );
            if let Some(definition) = &origin.definition {
                let d = crate::compiler_data::DiagnosticLocation::from_span(definition);
                message.push_str(&format!(
                    "; definition {}:{}:{}",
                    d.source, d.line, d.column
                ));
            }
            for helper in &origin.helpers {
                message.push_str(&format!(
                    "; helper {}/{}",
                    String::from_utf16_lossy(helper.namespace().unwrap_or(&[])),
                    String::from_utf16_lossy(helper.name())
                ));
            }
            message
        })?;
        record_script_expansion(expanded.nodes().len(), || origin.call.bytes());
        self.engine.generated_nodes.set(
            self.engine
                .generated_nodes
                .get()
                .saturating_add(expanded.nodes().len()),
        );
        if self.engine.generated_nodes.get() > self.limits.generated_nodes {
            return Err("compiler macro generated-node limit exceeded".into());
        }
        Ok(expanded)
    }

    pub fn new() -> Self {
        Self::default()
    }
    /// Run the macros of clean compilations in `host`'s engine instead of
    /// the built-in one (light-cljs-307, a prototype for the browser
    /// playground; docs/browser-macros.md). Interactive sessions and data
    /// readers keep the built-in QuickJS.
    pub fn set_script_host(&mut self, host: std::rc::Rc<dyn crate::host_evaluator::ScriptHost>) {
        self.engine.script_host = Some(host);
    }
    /// Control assertion emission for this host (enabled by default). Disabled
    /// assertions omit both condition and message, including function pre/post checks.
    pub fn set_assertions(&mut self, enabled: bool) {
        self.tables.assertions_disabled = !enabled;
    }
    pub fn assertions_enabled(&self) -> bool {
        !self.tables.assertions_disabled
    }
    /// Read immutable compiler-owned data without initializing an evaluator.
    pub fn read_compiler_form(
        &mut self,
        source: &crate::compiler_data::Source,
    ) -> Result<Option<Form>, crate::compiler_data::ReadError> {
        let mut parser = crate::Parser::new(source.text()).retaining_discards();
        let root = parser
            .read_form()
            .map_err(|error| crate::compiler_data::ReadError {
                message: error.to_string(),
                span: source.span(error.span).unwrap(),
                related: error.related.map(|span| source.span(span).unwrap()),
            })?;
        let mut discards = parser.discarded().to_vec();
        let syntax = crate::SyntaxTree {
            nodes: parser.into_nodes(),
            roots: root.into_iter().collect(),
        };
        discards.sort_by_key(|id| syntax.nodes[id.0].span.start);
        let Some(root) = root else {
            // Discard-only input still executes explicit tag handlers once.
            let mut end = 0;
            for &discard in &discards {
                let span = &syntax.nodes[discard.0].span;
                if span.start < end {
                    continue;
                }
                self.lower_form_in_view(
                    source,
                    &syntax,
                    discard,
                    &crate::compiler_reader::NamespaceView::new("cljs.user", source),
                    &discards,
                    None,
                )?;
                end = span.end;
            }
            return Ok(None);
        };
        self.lower_form_in_view(
            source,
            &syntax,
            root,
            &crate::compiler_reader::NamespaceView::new("cljs.user", source),
            &discards,
            None,
        )
        .map(Some)
    }
    /// Give a data reader heap `lightCljsBuild`, whose reads are recorded on
    /// the namespace whose form is being read.
    fn install_reader_build_services(
        &self,
        evaluator: &crate::evaluator::Evaluator,
    ) -> Result<(), crate::evaluator::EvalError> {
        use crate::compiler_evaluator::CompilerEvaluator;
        let (build, log) = (
            self.engine.build.clone(),
            self.engine.dependency_log.clone(),
        );
        evaluator.install_string_service(
            "__lightBuildService",
            std::rc::Rc::new(move |request| {
                crate::build_services::serve_request(&build, &log, request, |_| None, Vec::new())
            }),
        )?;
        let done = evaluator.evaluate_js(&format!(
            "(globalThis.lightCljsBuild = ({})(globalThis.__lightBuildService, {}), null)",
            crate::build_services::BUILD_OBJECT,
            crate::build_services::READER_CALLER
        ))?;
        crate::evaluator::MacroEvaluator::release(evaluator, done)
    }
    /// Register an explicit compiler-emitted function over immutable reader data.
    /// It executes in canonical JavaScript; only finite form-domain results cross
    /// the typed bridge. Registration grants no stream or filesystem capability
    /// beyond the `lightCljsBuild` services, which read resources across the
    /// compilation's source roots and record them on the namespace being read
    /// (docs/compiler.md, "Compile-time services: light.build").
    pub fn register_compiler_data_reader(
        &mut self,
        tag: Name,
        function_source: &str,
    ) -> Result<(), String> {
        let syntax = crate::parse(function_source).map_err(|e| e.to_string())?;
        let program = crate::analyze(function_source, &syntax).map_err(|e| e.to_string())?;
        let unit = crate::emitter::emit_js_unit(&program);
        std::sync::Arc::make_mut(&mut self.tables.data_readers)
            .units
            .insert(tag.clone(), unit);
        let mut readers = self.engine.data_readers.borrow_mut();
        if let Some(id) = readers.functions.remove(&tag)
            && let Some(evaluator) = &readers.evaluator
        {
            let _ = evaluator.release(id);
        }
        Ok(())
    }
    /// Fallback for unknown tags, called with the tag symbol and immutable value.
    /// Named readers take precedence; built-in literal tags retain their reader.
    pub fn register_compiler_default_data_reader(
        &mut self,
        function_source: &str,
    ) -> Result<(), String> {
        let syntax = crate::parse(function_source).map_err(|e| e.to_string())?;
        let program = crate::analyze(function_source, &syntax).map_err(|e| e.to_string())?;
        std::sync::Arc::make_mut(&mut self.tables.data_readers).default_unit =
            Some(crate::emitter::emit_js_unit(&program));
        let mut readers = self.engine.data_readers.borrow_mut();
        if let Some(id) = readers.default_function.take()
            && let Some(evaluator) = &readers.evaluator
        {
            let _ = evaluator.release(id);
        }
        Ok(())
    }
    /// Add the compiler data readers to a cache key: each tag with its
    /// reader's emitted unit, in tag order, then the default reader's.
    pub(crate) fn fingerprint_data_readers(&self, fingerprint: &mut crate::sha256::Fingerprint) {
        let readers = &self.tables.data_readers;
        let mut units: Vec<_> = readers
            .units
            .iter()
            .map(|(tag, unit)| (name_parts(tag), unit))
            .collect();
        units.sort_unstable();
        fingerprint.count(units.len());
        for ((namespace, name), unit) in units {
            fingerprint
                .optional_text(namespace.as_deref())
                .text(&name)
                .text(unit);
        }
        fingerprint.optional_text(readers.default_unit.as_deref());
    }
    /// Add an interactive host's name policy to a cache key.
    pub(crate) fn fingerprint_name_policy(&self, fingerprint: &mut crate::sha256::Fingerprint) {
        let Some(policy) = &self.tables.name_policy else {
            fingerprint.absent();
            return;
        };
        let sorted = |names: &mut dyn Iterator<Item = &Name>| {
            let mut names: Vec<_> = names.map(name_parts).collect();
            names.sort_unstable();
            names
        };
        fingerprint.present().flag(policy.unrestricted);
        for names in [
            policy.allow.as_ref().map(|allow| sorted(&mut allow.iter())),
            Some(sorted(&mut policy.deny.iter())),
        ] {
            let Some(names) = names else {
                fingerprint.absent();
                continue;
            };
            fingerprint.present().count(names.len());
            for (namespace, name) in &names {
                fingerprint.optional_text(namespace.as_deref()).text(name);
            }
        }
    }
    fn call_compiler_data_reader(
        &self,
        tag: &Name,
        value: &crate::compiler_data::Datum,
    ) -> Result<Option<crate::compiler_data::Datum>, String> {
        forbid_engine_entry("a data reader");
        let run =
            || -> Result<Option<crate::compiler_data::Datum>, String> {
                let (evaluator, function, fallback) = {
                    let sources = &self.tables.data_readers;
                    let mut readers = self.engine.data_readers.borrow_mut();
                    let fallback = !sources.units.contains_key(tag);
                    let unit = if fallback {
                        if tag.namespace().is_none()
                            && matches!(
                                String::from_utf16_lossy(tag.name()).as_ref(),
                                "inst" | "uuid" | "js" | "queue"
                            )
                        {
                            return Ok(None);
                        }
                        sources.default_unit.clone()
                    } else {
                        sources.units.get(tag).cloned()
                    };
                    let Some(unit) = unit else {
                        return Ok(None);
                    };
                    if readers.evaluator.is_none() {
                        let evaluator = self.new_evaluator().map_err(|e| e.to_string())?;
                        self.install_reader_build_services(&evaluator)
                            .map_err(|e| e.to_string())?;
                        readers.evaluator = Some(std::rc::Rc::new(evaluator));
                    }
                    let evaluator = readers.evaluator.as_ref().unwrap().clone();
                    let cached = if fallback {
                        readers.default_function
                    } else {
                        readers.functions.get(tag).copied()
                    };
                    let function = if let Some(function) = cached {
                        function
                    } else {
                        // In a scope of the reader's own, so that a `gensym` its
                        // definition calls gives one name whichever namespaces
                        // are hits and whichever reader runs first.
                        let key = match (fallback, name_parts(tag)) {
                            (true, _) => "#".to_owned(),
                            (false, (Some(namespace), name)) => format!("#{namespace}/{name}"),
                            (false, (None, name)) => format!("#{name}"),
                        };
                        let _scope = crate::generated_names::enter_reader_definition(&key, &unit);
                        crate::generated_names::sync_reader_gensym_scope(&*evaluator)
                            .map_err(|e| e.to_string())?;
                        let function = evaluator.evaluate_unit(&unit).map_err(|e| e.to_string())?;
                        if fallback {
                            readers.default_function = Some(function);
                        } else {
                            readers.functions.insert(tag.clone(), function);
                        }
                        function
                    };
                    (evaluator, function, fallback)
                };
                let limits = crate::form_bridge::ConversionLimits {
                    nodes: self.limits.generated_nodes,
                    depth: 128,
                };
                // Each conversion takes at most the node limit itself, so a
                // namespace needs a limit of at least its largest.
                let measuring = self.measures_work();
                if measuring {
                    self.note_conversion(WorkMarks::nodes(value));
                }
                let argument = evaluator
                    .import_data(value, limits)
                    .map_err(|e| e.to_string())?;
                let tag_argument = if fallback {
                    match evaluator.import_data(
                        &crate::compiler_data::DataBuilder.symbol(tag.clone()),
                        limits,
                    ) {
                        Ok(value) => Some(value),
                        Err(error) => {
                            let _ = evaluator.release(argument);
                            return Err(error.to_string());
                        }
                    }
                } else {
                    None
                };
                // Its `gensym` names belong to the namespace being read.
                let result = crate::generated_names::sync_reader_gensym_scope(&*evaluator)
                    .and_then(|()| match tag_argument {
                        Some(tag_argument) => evaluator.call(function, &[tag_argument, argument]),
                        None => evaluator.call(function, &[argument]),
                    });
                let _ = evaluator.release(argument);
                if let Some(tag_argument) = tag_argument {
                    let _ = evaluator.release(tag_argument);
                }
                let result = result.map_err(|e| e.to_string())?;
                let data = evaluator
                    .export_data(result, limits)
                    .map_err(|e| e.to_string());
                let _ = evaluator.release(result);
                if measuring && data.is_ok() {
                    let readers = self.engine.data_readers.borrow();
                    match take_encode_peak(&*evaluator, &readers.encode_peak) {
                        Some(peak) => self.note_conversion(peak),
                        None => self.note_unmeasured(),
                    }
                }
                data.map(Some)
            };
        let result = run();
        if result.is_err() {
            let mut readers = self.engine.data_readers.borrow_mut();
            readers.evaluator = None;
            readers.functions.clear();
            readers.default_function = None;
            readers.encode_peak.set(None);
        }
        result
    }
    pub(crate) fn lower_form_in_view(
        &self,
        source: &crate::compiler_data::Source,
        tree: &crate::SyntaxTree,
        root: crate::NodeId,
        view: &crate::compiler_reader::NamespaceView,
        discards: &[crate::NodeId],
        bindings: Option<&crate::compiler_reader::BindingResolver<'_>>,
    ) -> Result<Form, crate::compiler_data::ReadError> {
        let reader = |tag: &Name,
                      value: &crate::compiler_data::Datum,
                      location: &crate::compiler_data::SourceSpan| {
            let value = self.call_compiler_data_reader(tag, value)?;
            if let Some(value) = &value {
                let size = Form::from_data(
                    value.clone(),
                    location.clone(),
                    crate::compiler_data::Origin::SuppliedData,
                )
                .nodes()
                .len();
                let generated = self.engine.generated_nodes.get().saturating_add(size);
                if generated > self.limits.generated_nodes {
                    return Err("compiler reader generated-node limit exceeded".into());
                }
                self.engine.generated_nodes.set(generated);
            }
            Ok(value)
        };
        crate::compiler_reader::lower_form_with_reader(
            source,
            tree,
            root,
            view,
            discards,
            Some(&reader),
            bindings,
        )
    }
    /// Configure imports without loading or evaluating code. Filesystem compilation
    /// registers source dependencies first.
    pub(crate) fn set_interactive_imports(
        &mut self,
        caller: &str,
        imports: Vec<MacroImport>,
    ) -> Result<(), String> {
        let current = self.tables.imports.entry_or_default(caller.to_owned());
        for import in imports {
            if let Some(alias) = &import.alias
                && current.iter().any(|old| {
                    old.alias.as_ref() == Some(alias) && old.namespace != import.namespace
                })
            {
                return Err(format!("conflicting namespace alias: {alias}"));
            }
            if let Some(old) = current
                .iter_mut()
                .find(|old| old.namespace == import.namespace && old.alias == import.alias)
            {
                for name in import.refers {
                    if !old.refers.contains(&name) {
                        old.refers.push(name);
                    }
                }
            } else {
                current.push(import);
            }
        }
        Ok(())
    }
    /// Configure `caller`'s imports as [`Self::set_interactive_imports`]
    /// does, replacing the ones it had: a session's `ns` form declares them
    /// all. On an error the previous imports stay.
    pub(crate) fn replace_interactive_imports(
        &mut self,
        caller: &str,
        imports: Vec<MacroImport>,
    ) -> Result<(), String> {
        let previous = self.tables.imports.remove(caller);
        let result = self.set_interactive_imports(caller, imports);
        if result.is_err() {
            match previous {
                Some(previous) => self.tables.imports.insert(caller.to_owned(), previous),
                None => self.tables.imports.remove(caller),
            };
        }
        result
    }
    pub fn set_imports(&mut self, caller: &str, imports: Vec<MacroImport>) -> Result<(), String> {
        let mut aliases = HashMap::new();
        let mut refers = HashSet::new();
        for import in &imports {
            for name in &import.refers {
                if !refers.insert(name.clone()) {
                    return Err(format!("duplicate macro refer: {name}"));
                }
            }
            if let Some(alias) = &import.alias {
                if matches!(alias.as_str(), "cljs.core" | "clojure.core")
                    && alias != &import.namespace
                {
                    return Err(format!("reserved core macro alias: {alias}"));
                }
                if let Some(previous) = aliases.insert(alias.clone(), import.namespace.clone())
                    && previous != import.namespace
                {
                    return Err(format!("conflicting macro alias: {alias}"));
                }
                if alias != &import.namespace
                    && (alias == caller || imports.iter().any(|i| &i.namespace == alias))
                {
                    return Err(format!(
                        "macro alias {alias} conflicts with macro namespace qualifier"
                    ));
                }
            }
        }
        for import in &imports {
            for name in &import.refers {
                if matches!(import.namespace.as_str(), "cljs.core" | "clojure.core")
                    && core_macro(name)
                {
                    continue;
                }
                if !self.has_macro(&import.namespace, name)? {
                    return Err(format!(
                        "macro import {}/{name} does not name a public macro",
                        import.namespace
                    ));
                }
            }
        }
        self.tables.imports.insert(caller.to_owned(), imports);
        self.tables.renames.remove(caller);
        self.tables.namespace_symbols.remove(caller);
        self.tables.namespace_docs.remove(caller);
        Ok(())
    }
    pub(crate) fn set_namespace_symbol(
        &mut self,
        namespace: &str,
        symbol: crate::compiler_data::Datum,
        doc: Option<crate::compiler_data::Datum>,
    ) {
        self.tables
            .namespace_symbols
            .insert(namespace.to_owned(), symbol);
        if let Some(doc) = doc {
            self.tables.namespace_docs.insert(namespace.to_owned(), doc);
        }
    }
    /// `namespace`'s symbol as its `ns` form named it, with the form's
    /// metadata, docstring and attribute map as its metadata.
    pub(crate) fn namespace_symbol(&self, namespace: &str) -> Option<&crate::compiler_data::Datum> {
        self.tables.namespace_symbols.get(namespace)
    }
    /// Replace `namespace`'s symbol and docstring in `&env` ([`Self::set_namespace_symbol`]),
    /// or forget them: a session's `ns` form without metadata has none.
    pub(crate) fn set_namespace_metadata(
        &mut self,
        namespace: &str,
        symbol: Option<(
            crate::compiler_data::Datum,
            Option<crate::compiler_data::Datum>,
        )>,
    ) {
        self.tables.namespace_symbols.remove(namespace);
        self.tables.namespace_docs.remove(namespace);
        if let Some((symbol, doc)) = symbol {
            self.set_namespace_symbol(namespace, symbol, doc);
        }
    }
    /// What `prepare_source` and analysis declared for `namespace`: its
    /// imports, renamed macros and `:import`ed host names, and its `ns`
    /// form's docstring ([`crate::artifact_capture`]).
    pub(crate) fn namespace_declarations(&self, namespace: &str) -> NamespaceDeclarations<'_> {
        NamespaceDeclarations {
            imports: self
                .tables
                .imports
                .get(namespace)
                .map_or(&[], Vec::as_slice),
            renames: self
                .tables
                .renames
                .get(namespace)
                .map_or(&[], Vec::as_slice),
            namespace_imports: self
                .tables
                .namespace_imports
                .get(namespace)
                .map_or(&[], Vec::as_slice),
            doc: self.tables.namespace_docs.get(namespace),
        }
    }
    /// Restore what `prepare_source` and analysis declared for a namespace
    /// installed from its artifact (`crate::artifact_install`), as
    /// [`Self::namespace_declarations`] and [`Self::namespace_symbol`] read
    /// them back. Its imports were checked when it was compiled.
    pub(crate) fn restore_namespace_declarations(
        &mut self,
        namespace: &str,
        imports: Vec<MacroImport>,
        renames: Vec<(String, String, String)>,
        namespace_imports: Vec<(String, String)>,
        symbol: Option<crate::compiler_data::Datum>,
        doc: Option<crate::compiler_data::Datum>,
    ) {
        self.tables.imports.insert(namespace.to_owned(), imports);
        self.tables.renames.insert(namespace.to_owned(), renames);
        self.tables
            .namespace_imports
            .insert(namespace.to_owned(), namespace_imports);
        self.tables.namespace_symbols.remove(namespace);
        self.tables.namespace_docs.remove(namespace);
        if let Some(symbol) = symbol {
            self.tables
                .namespace_symbols
                .insert(namespace.to_owned(), symbol);
        }
        if let Some(doc) = doc {
            self.tables.namespace_docs.insert(namespace.to_owned(), doc);
        }
    }
    /// Declare a macro of an installed namespace (`crate::artifact_install`)
    /// for later expansions, without evaluating its function yet.
    pub(crate) fn install_javascript_macro(
        &mut self,
        key: String,
        declaration: crate::compiler_bootstrap::CompiledMacro,
    ) {
        self.insert_javascript_macro(key, declaration);
        self.engine.macro_environment = Default::default();
    }
    /// Whether `namespace/name` is a Var of the compile-time session, and
    /// whether it is private.
    pub(crate) fn runtime_var(&self, key: &str) -> Option<bool> {
        self.tables.runtime_vars.get(key).copied()
    }
    pub(crate) fn set_renames(&mut self, caller: &str, renames: Vec<(String, String, String)>) {
        self.tables.renames.insert(caller.to_owned(), renames);
    }
    /// A frame for analysis on this thread: this host's environment as it
    /// is now ([`Self::environment`]) with the binding's engines, caches,
    /// counters and session ([`EngineBinding`]).
    pub(crate) fn macro_analysis_frame(&self) -> Self {
        Self::from_parts(self.environment(), self.engine.frame())
    }
    /// The sendable part of an analysis frame of this host: what was
    /// declared up to now, and nothing declared after. A worker binds it to
    /// an engine of its own ([`Self::bind`]).
    pub(crate) fn environment(&self) -> MacroEnvironment {
        MacroEnvironment {
            limits: self.limits,
            evaluator_options: self.evaluator_options.clone(),
            opaque_packages: self.opaque_packages,
            tables: MacroTables {
                bootstrap_seed: true,
                ..self.tables.clone()
            },
        }
    }
    /// The host's two parts as they are, which [`Self::from_parts`] puts
    /// back together: the compilation catalog keeps a unit's environment,
    /// from which any thread may analyze it, and the thread that registered
    /// the unit keeps the binding (docs/compiler-concurrency.md, "As built
    /// (light-cljs-335)").
    pub(crate) fn into_parts(self) -> (MacroEnvironment, EngineBinding) {
        let Self {
            limits,
            evaluator_options,
            opaque_packages,
            tables,
            engine,
        } = self;
        let environment = MacroEnvironment {
            limits,
            evaluator_options,
            opaque_packages,
            tables,
        };
        (environment, engine)
    }
    pub(crate) fn from_parts(environment: MacroEnvironment, engine: EngineBinding) -> Self {
        let MacroEnvironment {
            limits,
            evaluator_options,
            opaque_packages,
            tables,
        } = environment;
        Self {
            limits,
            evaluator_options,
            opaque_packages,
            tables,
            engine,
        }
    }
    /// A host that analyzes on this thread from `environment`, which may
    /// have been made on another, bound to `evaluator`, a compile-time
    /// engine made on this thread. The evaluator gets the host's settings
    /// and the process-wide bootstrap units, which no thread compiles a
    /// second time, as the engine of a fresh compilation would; without
    /// one, the host analyzes only what runs no macro in an engine.
    ///
    /// The binding starts with no session, empty environment caches and
    /// zero counters. Source macros declared in another engine are not
    /// callable in this one: their functions are values of the engine that
    /// evaluated them, which this one refuses (docs/compiler-concurrency.md,
    /// "As built (light-cljs-334)").
    // Tests bind worker hosts today; the pool's workers will (light-cljs-338, 339).
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn bind(
        environment: MacroEnvironment,
        evaluator: Option<std::rc::Rc<dyn crate::compiler_evaluator::CompilerEvaluator>>,
    ) -> Result<Self, String> {
        let mut host = Self::from_parts(
            environment,
            EngineBinding {
                foreign: true,
                ..Default::default()
            },
        );
        if let Some(evaluator) = evaluator {
            host.prepare_compiler_evaluator(evaluator.as_ref())
                .map_err(|e| e.to_string())?;
            host.engine.bootstrap = Some(crate::compiler_bootstrap::bind(evaluator)?);
        }
        Ok(host)
    }
    /// The build configuration `light.build/config` answers in the
    /// compilations this host analyzes, as `--config KEY=EDN` gives it.
    /// Every namespace they record in a dependency manifest depends on its
    /// fingerprint.
    pub fn set_build_configuration(
        &mut self,
        configuration: crate::build_services::BuildConfiguration,
    ) {
        self.tables.configuration = std::sync::Arc::new(configuration);
    }
    pub fn build_configuration(&self) -> &crate::build_services::BuildConfiguration {
        &self.tables.configuration
    }
    /// Log an observation for the dependency manifest, if one is recorded.
    pub(crate) fn observe(
        &self,
        record: impl FnOnce(&mut crate::dependency_manifest::ObservationLog),
    ) {
        forbid_engine_entry("a dependency observation");
        // A unit a worker analyzes logs in its record, which the
        // coordinator adds to the compilation's log where it charges it.
        let Some(record) = record_observation(record) else {
            return;
        };
        if let Some(log) = self.engine.dependency_log.borrow_mut().as_mut() {
            record(log);
        }
    }
    /// Whether this analysis measures its logical work for an artifact's
    /// accounting ([`WorkMarks`]): in a unit this thread records, whether
    /// the compilation captures; otherwise whether its marks measure.
    pub(crate) fn measures_work(&self) -> bool {
        MacroFreeUnit::captures().unwrap_or_else(|| self.engine.work.measuring.get())
    }
    /// Note that the namespace's node budget had to leave room for `extra`
    /// nodes beyond those generated so far ([`WorkMarks::node_peak`]), or
    /// record it in the unit this thread records, where it is charged.
    pub(crate) fn note_node_room(&self, extra: usize) {
        if !record_capture(|| UnitEvent::Room(extra)) {
            WorkMarks::note(
                &self.engine.work.node_peak,
                self.engine.generated_nodes.get().saturating_add(extra),
            );
        }
    }
    /// [`WorkMarks::conversion_peak`], as [`Self::note_node_room`].
    pub(crate) fn note_conversion(&self, nodes: usize) {
        if !record_capture(|| UnitEvent::Conversion(nodes)) {
            WorkMarks::note(&self.engine.work.conversion_peak, nodes);
        }
    }
    /// [`WorkMarks::unmeasured`], as [`Self::note_node_room`].
    pub(crate) fn note_unmeasured(&self) {
        if !record_capture(|| UnitEvent::Unmeasured) {
            self.engine.work.unmeasured.set(true);
        }
    }
    /// Whether the source macro calls this analysis expands are captured
    /// for an artifact's provenance ([`Self::capture_expansion`]).
    pub(crate) fn captures_expansions(&self) -> bool {
        MacroFreeUnit::captures()
            .unwrap_or_else(|| self.session_handle().is_some_and(|s| s.capturing()))
    }
    /// Capture a source macro call expanded: in the session, or in the unit
    /// this thread records, whose charge captures it in the session in
    /// unit order (light-cljs-374).
    pub(crate) fn capture_expansion(
        &self,
        expansion: crate::compilation_session::CapturedExpansion,
    ) {
        let mut expansion = Some(expansion);
        if !record_capture(|| UnitEvent::Captured(Box::new(expansion.take().unwrap())))
            && let (Some(session), Some(expansion)) = (self.session_handle(), expansion)
        {
            session.capture_expansion(expansion);
        }
    }
    pub(crate) fn defers_source_units(&self) -> bool {
        !self.tables.bootstrap_seed
    }
    /// Begin measuring the next namespace's logical work
    /// ([`WorkMarks`]): its counts start here, and its marks from nothing.
    pub(crate) fn begin_namespace_work(&self) -> WorkStart {
        let work = &self.engine.work;
        work.measuring.set(true);
        work.node_peak.set(self.engine.generated_nodes.get());
        work.macro_depth.set(0);
        work.analysis_depth.set(0);
        work.unit_depth.set(0);
        work.conversion_peak.set(0);
        work.unmeasured.set(false);
        WorkStart {
            expansions: self.engine.expansions.get(),
            generated_nodes: self.engine.generated_nodes.get(),
        }
    }
    /// The macro calls the compilation executed: its expansions less those
    /// installed artifacts charged.
    pub(crate) fn executed_expansions(&self) -> usize {
        self.engine.expansions.get() - self.engine.work.charged_expansions.get()
    }
    /// Why the compilation's compile-time work cannot go on: its session's
    /// execution control, or `control`, is cancelled or terminal. Checked
    /// without spending the poll budget ([`crate::evaluator::ExecutionControl::check`]).
    pub(crate) fn halted(
        &self,
        control: Option<&crate::evaluator::ExecutionControl>,
    ) -> Option<String> {
        let session = self.engine.interactive_control.clone().or_else(|| {
            self.engine
                .bootstrap
                .as_ref()
                .map(|b| b.evaluator.control())
        });
        control
            .into_iter()
            .chain(session.as_ref())
            .find_map(|control| control.check().err())
            .map(|error| error.0)
    }
    pub(crate) fn session_handle(
        &self,
    ) -> Option<std::rc::Rc<crate::compilation_session::CompilationSession>> {
        self.engine.session_handle()
    }
    pub(crate) fn has_source_value(&self, namespace: &str, name: &str) -> bool {
        self.tables
            .source_bindings
            .get(&format!("{namespace}/{name}"))
            == Some(&SourceBindingKind::Value)
    }
    pub(crate) fn begin_compilation(&mut self) {
        if !self.tables.bootstrap_seed {
            let mut readers = self.engine.data_readers.borrow_mut();
            readers.evaluator = None;
            readers.functions.clear();
            readers.default_function = None;
            drop(readers);
            self.engine.bootstrap = None;
            self.tables.javascript_macros.clear();
            self.tables.macro_version = next_macro_version();
            self.tables.macro_names.clear();
            self.engine.macro_environment = Default::default();
            self.engine.namespace_environment = Default::default();
            self.engine.local_environment = Default::default();
            self.engine.reader_environment = Default::default();
            self.tables.source_bindings.clear();
            self.engine.session = Some(crate::compilation_session::CompilationSession::new());
            self.engine.session_ref = std::rc::Weak::new();
        }
        self.engine.expansions = Default::default();
        self.engine.generated_nodes = Default::default();
        self.engine.work = Default::default();
        self.engine.reader_declared = Default::default();
        self.tables.reader_generation =
            READER_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.tables.runtime_vars.clear();
        self.tables.namespace_imports.clear();
    }
    pub(crate) fn register_source_binding(
        &mut self,
        namespace: &str,
        name: &str,
        kind: SourceBindingKind,
    ) -> Result<(), String> {
        let key = format!("{namespace}/{name}");
        if self
            .tables
            .source_bindings
            .get(&key)
            .is_some_and(|previous| *previous != kind)
        {
            return Err(format!(
                "binding {key} cannot change between an ordinary Var and a compile-time macro; use distinct names for the two phases"
            ));
        }
        self.tables.source_bindings.insert(key, kind);
        Ok(())
    }
    pub(crate) fn register_runtime_var(
        &mut self,
        namespace: &str,
        name: &str,
        private: bool,
    ) -> Result<(), String> {
        forbid_engine_entry("a nested definition");
        self.register_source_binding(namespace, name, SourceBindingKind::Value)?;
        self.tables
            .runtime_vars
            .insert(format!("{namespace}/{name}"), private);
        Ok(())
    }
    pub(crate) fn source_namespace<'a>(
        &'a self,
        caller: &'a str,
        qualifier: &'a str,
    ) -> Option<&'a str> {
        if caller == qualifier {
            return Some(caller);
        }
        self.tables
            .imports
            .get(caller)
            .into_iter()
            .flatten()
            .find(|i| i.alias.as_deref() == Some(qualifier) || i.namespace == qualifier)
            .map(|i| i.namespace.as_str())
    }
    pub(crate) fn source_binding_private(&self, namespace: &str, name: &str) -> bool {
        if self
            .tables
            .runtime_vars
            .get(&format!("{namespace}/{name}"))
            .copied()
            .unwrap_or(false)
        {
            return true;
        }
        self.tables
            .javascript_macros
            .get(&format!("{namespace}/{name}"))
            .and_then(|m| m.metadata.as_ref())
            .and_then(|m| {
                m.get(&crate::compiler_data::DataBuilder.keyword(Name::new(None, "private")))
                    .ok()
                    .flatten()
            })
            .is_some_and(|v| {
                !matches!(
                    v.inspect(),
                    crate::compiler_data::DatumView::Nil
                        | crate::compiler_data::DatumView::Boolean(false)
                )
            })
    }
    pub(crate) fn add_namespace_imports(
        &mut self,
        namespace: &str,
        imports: Vec<(String, String)>,
    ) {
        let current = self
            .tables
            .namespace_imports
            .entry_or_default(namespace.to_owned());
        for import in imports {
            if !current.contains(&import) {
                current.push(import);
            }
        }
    }
    pub(crate) fn set_namespace_imports(
        &mut self,
        namespace: &str,
        imports: Vec<(String, String)>,
    ) {
        self.tables
            .namespace_imports
            .insert(namespace.to_owned(), imports);
    }
    pub(crate) fn definition_macro(&self, caller: &str, operator: &str) -> Option<String> {
        let imports = self
            .tables
            .imports
            .get(caller)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        let target = if let Some((qualifier, name)) = operator.split_once('/') {
            imports
                .iter()
                .find(|i| i.namespace == qualifier || i.alias.as_deref() == Some(qualifier))
                .map(|i| (i.namespace.as_str(), name))
        } else if let Some((_, ns, name)) = self
            .tables
            .renames
            .get(caller)
            .into_iter()
            .flatten()
            .find(|(local, _, _)| local == operator)
        {
            Some((ns.as_str(), name.as_str()))
        } else {
            imports
                .iter()
                .find(|i| i.refers.iter().any(|n| n == operator))
                .map(|i| (i.namespace.as_str(), operator))
        };
        target
            .filter(|(ns, name)| is_definition_macro(ns, name))
            .map(|(_, name)| name.into())
    }

    pub(crate) fn has_macro(&mut self, namespace: &str, name: &str) -> Result<bool, String> {
        if matches!(namespace, "cljs.core" | "clojure.core") {
            return Ok(core_macro(name));
        }
        if self
            .tables
            .javascript_macros
            .contains_key(&format!("{namespace}/{name}"))
        {
            return Ok(!self.source_binding_private(namespace, name));
        }
        Ok(false)
    }
    pub(crate) fn resolves_macro(
        &mut self,
        caller: &str,
        operator: &str,
        shadow_core: bool,
    ) -> Result<bool, String> {
        match self.macro_name(caller, operator, shadow_core) {
            Some((namespace, name)) => self.has_macro(&namespace, &name),
            None => Ok(false),
        }
    }

    pub(crate) fn macro_name(
        &self,
        caller: &str,
        operator: &str,
        shadow_core: bool,
    ) -> Option<(String, String)> {
        let imports = self
            .tables
            .imports
            .get(caller)
            .map(Vec::as_slice)
            .unwrap_or(&[]);
        if let Some((qualifier, name)) = operator.split_once('/') {
            if qualifier == "cljs.core" || qualifier == "clojure.core" {
                core_macro(name).then(|| ("clojure.core".to_owned(), name.to_owned()))
            } else {
                imports
                    .iter()
                    .find(|i| i.namespace == qualifier || i.alias.as_deref() == Some(qualifier))
                    .and_then(|i| {
                        // An alias of the core namespace names a macro only when
                        // the core name is one: `c/concat` is the function.
                        if i.namespace == "cljs.core" || i.namespace == "clojure.core" {
                            core_macro(name).then(|| ("clojure.core".to_owned(), name.to_owned()))
                        } else {
                            Some((i.namespace.clone(), name.to_owned()))
                        }
                    })
            }
        } else if let Some((_, namespace, name)) = self
            .tables
            .renames
            .get(caller)
            .into_iter()
            .flatten()
            .find(|(local, _, _)| local == operator)
        {
            Some((namespace.clone(), name.clone()))
        } else {
            imports
                .iter()
                .find(|i| i.refers.iter().any(|n| n == operator))
                .map(|i| (i.namespace.clone(), operator.to_owned()))
                .or_else(|| {
                    (!shadow_core && core_macro(operator))
                        .then(|| ("clojure.core".to_owned(), operator.to_owned()))
                })
        }
    }
    /// `site`, the operator as spelled and the call's span, is recorded
    /// with the attempt in a macro-free unit ([`record_native_expansion`]).
    pub(crate) fn expand_compiler_seed(
        &mut self,
        form: &Form,
        name: &Name,
        site: Option<(&str, &std::ops::Range<usize>)>,
    ) -> Result<Option<Form>, String> {
        let builder = crate::compiler_data::DataBuilder;
        let extended = crate::compiler_destructuring::expand(&builder, form, name)?;
        let parts = form.data().elements()?;
        let operation = String::from_utf16_lossy(name.name());
        let minimum = match operation.as_ref() {
            "let" | "loop" | "letfn" | "fn" => 1,
            "defn" | "defn-" => 2,
            _ => 0,
        };
        if parts.len() < minimum + 1 {
            return Err(format!("{operation} requires arguments"));
        }
        let mut args = vec![builder.nil(), builder.nil()];
        args.extend_from_slice(&parts[1..]);
        // `(^:async fn ...)` keeps its metadata on the `fn*` it expands to,
        // where analysis reads it, as upstream's `fn*` reads its symbol's.
        let asynchronous =
            operation == "fn" && crate::compiler_binding_macros::asynchronous(&builder, &parts[0])?;
        let expanded = if let Some(expanded) = extended {
            Some(if asynchronous {
                let data =
                    crate::compiler_binding_macros::mark_asynchronous(&builder, &expanded.data())?;
                Form::from_expansion(data, form, name.clone())
            } else {
                expanded
            })
        } else {
            use crate::compiler_binding_macros as seed;
            let data = match String::from_utf16_lossy(name.name()).as_str() {
                "let" => Some(seed::let_macro(&mut { builder }, &args)),
                "loop" => Some(seed::loop_macro(&mut { builder }, &args)),
                "fn" => Some(seed::fn_macro(&mut { builder }, &args)),
                "letfn" => Some(seed::letfn_macro(&mut { builder }, &args)),
                "defn" => Some(seed::defn_macro(&mut { builder }, &args)),
                "defn-" => Some(seed::private_defn(&mut { builder }, &args)),
                "assert" => {
                    if !(2..=3).contains(&parts.len()) {
                        return Err("assert requires a condition and optional message".into());
                    }
                    Some(seed::call(&builder, "compiler-assert*", &parts[1..]))
                }
                _ => None,
            };
            data.transpose()?
                .map(|data| {
                    if asynchronous {
                        seed::mark_asynchronous(&builder, &data)
                    } else {
                        Ok(data)
                    }
                })
                .transpose()?
                .map(|data| Form::from_expansion(data, form, name.clone()))
        };
        let expanded = match expanded {
            Some(form) => Some(form),
            None => self.expand_namespace_reflection(form, name)?,
        };
        if let Some(expanded) = &expanded {
            if let Some((operator, span)) = site {
                record_native_expansion(|| NativeExpansion {
                    nodes: expanded.nodes().len(),
                    operator: operator.to_owned(),
                    span: span.clone(),
                });
            }
            if self.engine.expansions.get() >= self.limits.expansions {
                return Err("compiler macro expansion limit exceeded".into());
            }
            self.engine.expansions.set(self.engine.expansions.get() + 1);
            self.engine.generated_nodes.set(
                self.engine
                    .generated_nodes
                    .get()
                    .saturating_add(expanded.nodes().len()),
            );
            if self.engine.generated_nodes.get() > self.limits.generated_nodes {
                return Err("compiler macro generated-node limit exceeded".into());
            }
        }
        Ok(expanded)
    }
    /// Macro identity/metadata is source reflection; its executable closure remains private.
    pub(crate) fn reflected_macro(
        &self,
        caller: &str,
        requested: &str,
    ) -> Option<(String, String, Option<crate::compiler_data::Datum>)> {
        if let Some(key) = self.source_macro_key(caller, requested)
            && let Some(declaration) = self.tables.javascript_macros.get(&key)
        {
            let (namespace, name) = key.split_once('/')?;
            return Some((namespace.into(), name.into(), declaration.metadata.clone()));
        }
        let (namespace, name) = requested
            .split_once('/')
            .unwrap_or(("cljs.core", requested));
        if matches!(namespace, "cljs.core" | "clojure.core")
            && !self.has_source_value(caller, requested)
            && crate::CoreFnId::resolve(name).is_none()
            && (core_macro(name) || is_definition_macro(namespace, name))
        {
            return Some(("cljs.core".into(), name.into(), None));
        }
        None
    }
    fn expand_namespace_reflection(
        &self,
        form: &Form,
        name: &Name,
    ) -> Result<Option<Form>, String> {
        let operation = String::from_utf16_lossy(name.name());
        let operation = operation.strip_prefix("compiler-").unwrap_or(&operation);
        if !matches!(
            operation,
            "ns-publics" | "ns-interns" | "ns-imports" | "ns-refers"
        ) {
            return Ok(None);
        }
        let run = || -> Result<_, Box<dyn std::error::Error>> {
            let parts = form.data().elements()?;
            let quoted = if parts.len() == 2 {
                parts[1].elements().unwrap_or_default()
            } else {
                vec![]
            };
            if quoted.len() != 2
                || !matches!(quoted[0].inspect(), crate::compiler_data::DatumView::Symbol(n) if n == Name::new(None,"quote"))
                || !matches!(
                    quoted[1].inspect(),
                    crate::compiler_data::DatumView::Symbol(_)
                )
            {
                return Err(format!("Argument to {operation} must be a quoted symbol").into());
            }
            let crate::compiler_data::DatumView::Symbol(namespace) = quoted[1].inspect() else {
                unreachable!()
            };
            let namespace = String::from_utf16(namespace.name())?;
            if !matches!(namespace.as_str(), "cljs.core" | "clojure.core") {
                self.observe(|log| {
                    log.named(&namespace, crate::dependency_manifest::EdgeKinds::OBSERVED)
                });
            }
            let mut declarations = if operation == "ns-refers" {
                self.reader_refers(&namespace)
                    .into_iter()
                    .map(|(local, ns, original)| (local, format!("{ns}/{original}")))
                    .collect()
            } else if operation == "ns-imports" {
                self.tables
                    .namespace_imports
                    .get(&namespace)
                    .cloned()
                    .unwrap_or_default()
            } else {
                self.tables
                    .runtime_vars
                    .iter()
                    .filter_map(|(key, private)| {
                        let (owner, local) = key.split_once('/')?;
                        (owner == namespace && (operation == "ns-interns" || !*private))
                            .then(|| (local.to_owned(), key.clone()))
                    })
                    .collect()
            };
            if matches!(operation, "ns-interns" | "ns-publics") {
                if matches!(namespace.as_str(), "cljs.core" | "clojure.core") {
                    declarations.extend(
                        crate::compiler_reader::core_public_bindings()
                            .filter(|(local, _)| {
                                crate::CoreFnId::resolve(local).is_some()
                                    || crate::compiler_names::core_macro(local)
                                    || crate::compiler_names::is_definition_macro(
                                        "cljs.core",
                                        local,
                                    )
                            })
                            .map(|(local, _)| (local.into(), format!("cljs.core/{local}"))),
                    );
                }
                declarations.extend(self.tables.javascript_macros.keys().filter_map(|key| {
                    let (ns, local) = key.split_once('/')?;
                    (ns == namespace
                        && (operation == "ns-interns" || !self.source_binding_private(ns, local)))
                    .then(|| (local.into(), key.clone()))
                }));
            }
            declarations.sort();
            declarations.dedup();
            let c = crate::compiler_data::DataBuilder;
            let symbol = |s: &str| c.symbol(Name::new(None, s));
            let mut entries = Vec::new();
            for (local, qualified) in declarations {
                let key = c.list(&[symbol("quote"), symbol(&local)])?;
                let value = if operation == "ns-imports" {
                    c.symbol(Name::new(Some("js"), &qualified))
                } else {
                    let (namespace, local) = qualified.split_once('/').unwrap();
                    c.list(&[symbol("var"), c.symbol(Name::new(Some(namespace), local))])?
                };
                entries.push((key, value));
            }
            Ok(Form::from_expansion(c.map(&entries)?, form, name.clone()))
        };
        run().map(Some).map_err(|e| e.to_string())
    }
    pub(crate) fn register_macro_view(&self, view: &mut crate::compiler_reader::NamespaceView) {
        let mut names = self.tables.javascript_macros.keys().collect::<Vec<_>>();
        names.sort();
        for (index, key) in names.into_iter().enumerate() {
            let (namespace, local) = key.split_once('/').unwrap();
            let name = Name::new(Some(namespace), local);
            let binding = crate::compiler_reader::ResolvedBinding {
                id: crate::compiler_reader::BindingId(usize::MAX / 2 - index),
                kind: crate::compiler_reader::BindingKind::Macro,
                name: name.clone(),
            };
            view.bindings.insert(name.clone(), binding.clone());
            if view.namespace == Name::new(None, namespace) {
                view.bindings
                    .insert(Name::new(None, local), binding.clone());
            }
            for (local, target) in &view.refers {
                if target == &name {
                    view.bindings.insert(local.clone(), binding.clone());
                }
            }
        }
    }
    pub(crate) fn reader_refers(&self, namespace: &str) -> Vec<(String, String, String)> {
        let mut names = self
            .tables
            .imports
            .get(namespace)
            .into_iter()
            .flatten()
            .flat_map(|import| {
                import
                    .refers
                    .iter()
                    .map(|name| (name.clone(), import.namespace.clone(), name.clone()))
            })
            .collect::<Vec<_>>();
        names.extend(
            self.tables
                .renames
                .get(namespace)
                .into_iter()
                .flatten()
                .cloned(),
        );
        names
    }
    pub(crate) fn reader_aliases(
        &self,
        namespace: &str,
        runtime: &[(String, String)],
        requires: &[String],
    ) -> Vec<(String, String)> {
        let mut aliases = requires
            .iter()
            .map(|n| (n.clone(), n.clone()))
            .collect::<Vec<_>>();
        aliases.extend_from_slice(runtime);
        if let Some(imports) = self.tables.imports.get(namespace) {
            for import in imports {
                aliases.push((import.namespace.clone(), import.namespace.clone()));
                if let Some(alias) = &import.alias {
                    aliases.push((alias.clone(), import.namespace.clone()));
                }
            }
        }
        aliases
    }
    /// The names of `namespace`'s source macros, sorted, from the index
    /// rather than every macro.
    pub(crate) fn source_macro_names(&self, namespace: &str) -> Vec<String> {
        self.tables
            .macro_names
            .get(namespace)
            .cloned()
            .unwrap_or_default()
    }
    /// Declare the source macro `key` (`namespace/name`).
    pub(crate) fn insert_javascript_macro(
        &mut self,
        key: String,
        declaration: crate::compiler_bootstrap::CompiledMacro,
    ) {
        if !self.tables.javascript_macros.contains_key(&key)
            && let Some((namespace, name)) = key.split_once('/')
        {
            let names = self
                .tables
                .macro_names
                .entry_or_default(namespace.to_owned());
            if let Err(position) = names.binary_search_by(|old| old.as_str().cmp(name)) {
                names.insert(position, name.to_owned());
            }
        }
        self.tables.javascript_macros.insert(key, declaration);
        self.tables.macro_version = next_macro_version();
    }

    /// Add the reader context of `namespace` to a macro's environment. The
    /// first `shared` globals are the compilation's, in its order, of which
    /// `namespace_globals` identifies the members of `namespace`: the session registers each of those
    /// once, so a context names only the ones registered since, and how many
    /// it sees. The globals after `shared` are this analysis's own.
    pub(crate) fn reader_environment(
        &self,
        environment: crate::compiler_data::Datum,
        namespace: &str,
        resolution: &crate::analyzer::NamespaceResolution,
        globals: &crate::arena::Arena<crate::GlobalBinding>,
        shared: usize,
        namespace_globals: &crate::arena::Arena<crate::analyzer::GlobalId>,
    ) -> Result<(crate::compiler_data::Datum, Option<ReaderRegistration>), String> {
        let visible = namespace_globals.len();
        let c = crate::compiler_data::DataBuilder;
        let string = |s: &str| c.string(s.encode_utf16().collect::<Vec<_>>());
        let key = |s| c.keyword(Name::new(None, s));
        let mut registration = None;
        let (position, count) = self
            .engine
            .reader_declared
            .borrow()
            .get(namespace)
            .copied()
            .unwrap_or((0, 0));
        let mut names = Vec::new();
        if position <= shared {
            names = namespace_globals
                .range(count..visible)
                .map(|id| globals[id.0].name.clone())
                .collect();
            debug_assert_eq!(count + names.len(), visible);
            registration = Some(ReaderRegistration {
                namespace: namespace.to_owned(),
                position: shared,
                visible,
            });
        }
        let context_key = ReaderEnvironmentKey {
            refers: resolution
                .refers
                .iter()
                .cloned()
                .chain(self.reader_refers(namespace))
                .collect(),
            excludes: resolution.excludes.clone(),
            generation: self.tables.reader_generation,
            from: count,
            names,
            visible,
            bindings: globals
                .range(shared..globals.len())
                .filter(|g| g.namespace == namespace)
                .map(|g| g.name.clone())
                .chain(self.source_macro_names(namespace))
                .collect(),
        };
        let context = {
            let mut cache = self.engine.reader_environment.borrow_mut();
            if let Some(cached) = cache.as_ref().filter(|c| c.key == context_key) {
                cached.data.clone()
            } else {
                let strings = |names: &[String]| {
                    c.vector(&names.iter().map(|s| string(s)).collect::<Vec<_>>())
                };
                let declared = c.map(&[
                    (key("generation"), c.integer(context_key.generation as i64)),
                    (key("from"), c.integer(context_key.from as i64)),
                    (key("names"), strings(&context_key.names)?),
                ])?;
                let context = c.map(&[
                    (
                        key("refers"),
                        c.vector(
                            &context_key
                                .refers
                                .iter()
                                .map(|(local, ns, name)| {
                                    c.vector(&[string(local), string(ns), string(name)])
                                })
                                .collect::<Result<Vec<_>, _>>()?,
                        )?,
                    ),
                    (key("excludes"), strings(&context_key.excludes)?),
                    (key("declared"), declared),
                    (key("visible"), c.integer(context_key.visible as i64)),
                    (key("bindings"), strings(&context_key.bindings)?),
                ])?;
                *cache = Some(ReaderEnvironment {
                    key: context_key,
                    data: context.clone(),
                });
                context
            }
        };
        let environment = c.assoc(
            &environment,
            &c.keyword(Name::new(Some("light-cljs"), "reader-context")),
            &context,
        )?;
        Ok((environment, registration))
    }

    /// Record that the macro session holds the declarations an environment
    /// from [`Self::reader_environment`] named, once it has reached the
    /// session: an environment that was not sent registers nothing.
    pub(crate) fn register_reader_declarations(&self, registration: Option<ReaderRegistration>) {
        let Some(registration) = registration else {
            return;
        };
        let mut declared = self.engine.reader_declared.borrow_mut();
        let entry = declared.entry(registration.namespace).or_insert((0, 0));
        if registration.position >= entry.0 {
            *entry = (registration.position, registration.visible);
        }
    }

    pub fn compiler_environment(
        &self,
        namespace: &str,
        context: ExprContext,
        locals: &[LocalDescription],
        aliases: &[(String, String)],
        requires: &[String],
        location: (usize, usize),
    ) -> Result<crate::compiler_data::Datum, String> {
        let imports = self
            .tables
            .imports
            .get(namespace)
            .cloned()
            .unwrap_or_default();
        let namespace_symbol = self.tables.namespace_symbols.get(namespace).cloned();
        let namespace_doc = self.tables.namespace_docs.get(namespace).cloned();
        let namespace_imports = self
            .tables
            .namespace_imports
            .get(namespace)
            .cloned()
            .unwrap_or_default();
        let c = crate::compiler_data::DataBuilder;
        let key = |s: &str| {
            let name = s.split_once('/').map_or_else(
                || Name::new(None, s),
                |(namespace, name)| Name::new(Some(namespace), name),
            );
            c.keyword(name)
        };
        let sym = |s: &str| c.symbol(Name::new(None, s));
        let macro_environment = {
            let mut cache = self.engine.macro_environment.borrow_mut();
            // A worker's binding serves units of many snapshots, so the
            // cache names the macro inventory it was made from.
            let version = self.tables.macro_version;
            if let Some((_, data)) = cache.as_ref().filter(|(v, _)| *v == version) {
                data.clone()
            } else {
                let data = c.map(&{
                    let mut declarations = self.tables.javascript_macros.iter().collect::<Vec<_>>();
                    declarations.sort_by(|a, b| a.0.cmp(b.0));
                    declarations
                        .into_iter()
                        .map(|(name, declaration)| {
                            let (ns, local) = name.split_once('/').unwrap();
                            (
                                c.symbol(Name::new(Some(ns), local)),
                                declaration.metadata.clone().unwrap_or_else(|| c.nil()),
                            )
                        })
                        .collect::<Vec<_>>()
                })?;
                *cache = Some((version, data.clone()));
                data
            }
        };
        let namespace_key = NamespaceEnvironmentKey {
            namespace: namespace.to_owned(),
            aliases: aliases.to_vec(),
            requires: requires.to_vec(),
            imports: imports.clone(),
            renames: self
                .tables
                .renames
                .get(namespace)
                .cloned()
                .unwrap_or_default(),
            namespace_imports: namespace_imports.clone(),
            symbol: namespace_symbol.as_ref().map(|d| d.identity()),
            doc: namespace_doc.as_ref().map(|d| d.identity()),
        };
        let namespace_environment = {
            let mut cache = self.engine.namespace_environment.borrow_mut();
            if let Some(cached) = cache.as_ref().filter(|c| c.key == namespace_key) {
                cached.data.clone()
            } else {
                let mut macro_aliases = Vec::new();
                let mut macro_refers = Vec::new();
                for import in &imports {
                    macro_aliases.push((sym(&import.namespace), sym(&import.namespace)));
                    if let Some(alias) = &import.alias {
                        macro_aliases.push((sym(alias), sym(&import.namespace)));
                    }
                    for name in &import.refers {
                        macro_refers.push((sym(name), sym(&import.namespace)));
                    }
                }
                for (local, target, _) in self.tables.renames.get(namespace).into_iter().flatten() {
                    macro_refers.push((sym(local), sym(target)));
                }
                let mut runtime = requires
                    .iter()
                    .map(|n| (sym(n), sym(n)))
                    .collect::<Vec<_>>();
                runtime.extend(aliases.iter().map(|(a, n)| (sym(a), sym(n))));
                runtime.extend(macro_aliases);
                let import_names = namespace_imports
                    .iter()
                    .map(|(a, n)| (sym(a), sym(n)))
                    .collect::<Vec<_>>();
                runtime.extend(import_names.iter().cloned());
                // Repeated ordinary requires and shared phase aliases have one
                // identity in the metadata map, just as in name resolution.
                let mut seen_names = HashSet::new();
                runtime.retain(|(key, _)| match key.inspect() {
                    crate::compiler_data::DatumView::Symbol(name) => seen_names.insert(name),
                    _ => unreachable!("namespace map keys are symbols"),
                });
                let data = c.map(&[
                    (
                        key("name"),
                        namespace_symbol.clone().unwrap_or_else(|| sym(namespace)),
                    ),
                    (key("doc"), namespace_doc.clone().unwrap_or_else(|| c.nil())),
                    (key("requires"), c.map(&runtime)?),
                    (key("imports"), c.map(&import_names)?),
                    (key("uses"), c.map(&macro_refers)?),
                    (key("require-macros"), c.map(&[])?),
                    (key("use-macros"), c.map(&[])?),
                ])?;
                *cache = Some(NamespaceEnvironment {
                    key: namespace_key,
                    data: data.clone(),
                });
                data
            }
        };
        let build = || -> Result<crate::compiler_data::Datum, String> {
            let local_map = {
                let mut cache = self.engine.local_environment.borrow_mut();
                if let Some(cached) = cache.as_ref().filter(|c| c.locals == locals) {
                    cached.data.clone()
                } else {
                    let prefix = cache.as_ref().map_or(0, |cached| {
                        cached
                            .locals
                            .iter()
                            .zip(locals)
                            .take_while(|(a, b)| a == b)
                            .count()
                    });
                    let mut rows = cache
                        .as_ref()
                        .map_or_else(Vec::new, |cached| cached.rows[..prefix].to_vec());
                    let mut entries: Vec<(
                        crate::compiler_data::Datum,
                        crate::compiler_data::Datum,
                    )> = Vec::new();
                    let mut positions: HashMap<&str, usize> = HashMap::new();
                    for ((name, ..), (symbol, details)) in locals.iter().zip(&rows) {
                        if let Some(&index) = positions.get(name.as_str()) {
                            entries[index].1 = details.clone();
                        } else {
                            positions.insert(name.as_str(), entries.len());
                            entries.push((symbol.clone(), details.clone()));
                        }
                    }
                    for (local, id, kind, argument) in locals.iter().skip(prefix) {
                        let position = positions.get(local.as_str()).copied();
                        let name = sym(local);
                        let mut details = vec![
                            (key("name"), name.clone()),
                            (key("form"), name.clone()),
                            (key("op"), key("binding")),
                            (key("local"), key(kind)),
                            (key("light-cljs/id"), c.integer(*id as i64)),
                        ];
                        if *kind != "fn" {
                            details.push((key("binding-form?"), c.boolean(true)));
                        }
                        if let Some(argument) = argument {
                            details.push((key("arg-id"), c.integer(*argument as i64)));
                        }
                        if let Some(index) = position {
                            details.push((key("shadow"), entries[index].1.clone()));
                        }
                        let details = c.map(&details)?;
                        rows.push((name.clone(), details.clone()));
                        if let Some(index) = position {
                            entries[index].1 = details;
                        } else {
                            positions.insert(local.as_str(), entries.len());
                            entries.push((name, details));
                        }
                    }
                    let local_map = c.map(&entries)?;
                    *cache = Some(LocalEnvironment {
                        locals: locals.to_vec(),
                        rows,
                        data: local_map.clone(),
                    });
                    local_map
                }
            };
            c.map(&[
                (key("locals"), local_map),
                // Descriptive compiler bindings only: no runtime Var or callable
                // macro value crosses this metadata service.
                (
                    c.keyword(Name::new(Some("light-cljs"), "macros")),
                    macro_environment.clone(),
                ),
                (
                    key("context"),
                    key(match context {
                        ExprContext::Value => "expr",
                        ExprContext::Discard => "statement",
                        ExprContext::Return => "return",
                    }),
                ),
                (key("ns"), namespace_environment.clone()),
                (key("line"), c.integer(location.0 as i64)),
                (key("column"), c.integer(location.1 as i64)),
            ])
        };
        build().map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod compiler_pure_tests {
    use super::*;
    use crate::compiler_data::{DataBuilder, Datum, DatumKind};

    fn field(data: &Datum, key: &str) -> Datum {
        let (ns, name) = key
            .split_once('/')
            .map_or((None, key), |(ns, name)| (Some(ns), name));
        data.get(&DataBuilder.keyword(Name::new(ns, name)))
            .unwrap()
            .unwrap()
    }

    #[test]
    fn environment_reuse_preserves_local_shadow_chains_and_location() {
        let host = MacroHost::new();
        let mut locals = vec![
            ("x".to_owned(), 1, "let", None),
            ("x".to_owned(), 2, "arg", Some(0)),
        ];
        let env = |locals: &[LocalDescription], location| {
            host.compiler_environment("demo", ExprContext::Value, locals, &[], &[], location)
                .unwrap()
        };
        let first = env(&locals, (1, 2));
        let repeated = env(&locals, (3, 4));
        assert_eq!(
            field(&first, "locals").identity(),
            field(&repeated, "locals").identity()
        );
        assert!(matches!(
            field(&repeated, "line").kind(),
            DatumKind::Integer(3)
        ));
        let local = |env: &Datum| {
            field(env, "locals")
                .get(&DataBuilder.symbol(Name::new(None, "x")))
                .unwrap()
                .unwrap()
        };
        assert!(matches!(
            field(&local(&first), "arg-id").kind(),
            DatumKind::Integer(0)
        ));
        assert!(matches!(
            field(&field(&local(&first), "shadow"), "light-cljs/id").kind(),
            DatumKind::Integer(1)
        ));
        locals[1] = ("x".to_owned(), 3, "fn", None);
        let changed = env(&locals, (5, 6));
        assert_ne!(
            field(&first, "locals").identity(),
            field(&changed, "locals").identity()
        );
        assert!(
            local(&changed)
                .get(&DataBuilder.keyword(Name::new(None, "binding-form?")))
                .unwrap()
                .is_none()
        );
        assert!(
            local(&changed)
                .get(&DataBuilder.keyword(Name::new(None, "arg-id")))
                .unwrap()
                .is_none()
        );
        assert!(matches!(
            field(&local(&changed), "light-cljs/id").kind(),
            DatumKind::Integer(3)
        ));
        let mut nested = locals.clone();
        nested.push(("x".into(), 4, "let", None));
        nested.push(("y".into(), 5, "let", None));
        let inner = env(&nested, (5, 7));
        let outer = env(&locals[..1], (5, 8));
        assert!(matches!(
            field(&field(&local(&inner), "shadow"), "light-cljs/id").kind(),
            DatumKind::Integer(3)
        ));
        assert!(matches!(
            field(&local(&outer), "light-cljs/id").kind(),
            DatumKind::Integer(1)
        ));
        assert!(
            local(&outer)
                .get(&DataBuilder.keyword(Name::new(None, "shadow")))
                .unwrap()
                .is_none()
        );
        // Sharing the cache across demand-analysis frames must not mix their
        // lexical views, or mutate an earlier environment saved by a macro.
        let frame = host.macro_analysis_frame();
        let empty = frame
            .compiler_environment("demo", ExprContext::Return, &[], &[], &[], (7, 8))
            .unwrap();
        assert!(
            matches!(field(&empty, "locals").kind(), DatumKind::Collection { children, .. } if children.is_empty())
        );
        assert!(matches!(
            field(&local(&first), "light-cljs/id").kind(),
            DatumKind::Integer(2)
        ));
        assert_eq!(
            field(&local(&env(&locals, (9, 10))), "local").inspect(),
            DataBuilder.keyword(Name::new(None, "fn")).inspect()
        );
    }

    #[test]
    fn reader_environment_reuse_obeys_declaration_acknowledgement_and_old_views() {
        let mut host = MacroHost::new();
        host.begin_compilation();
        let mut globals = crate::arena::Arena::default();
        for name in ["a", "b", "c"] {
            globals.push(crate::GlobalBinding {
                namespace: "demo".into(),
                name: name.into(),
                type_descriptor: false,
                declared: true,
                constant: false,
                define: false,
                boolean_tag: false,
                metadata: None,
                span: 0..0,
                source_index: 0,
            });
        }
        let mut resolution = crate::analyzer::NamespaceResolution::default();
        let context = |host: &MacroHost,
                       resolution: &crate::analyzer::NamespaceResolution,
                       shared,
                       visible| {
            let ids = globals
                .range(0..shared)
                .enumerate()
                .filter_map(|(index, global)| {
                    (global.namespace == "demo").then_some(crate::analyzer::GlobalId(index))
                })
                .collect::<crate::arena::Arena<_>>();
            assert_eq!(ids.len(), visible);
            let (env, registration) = host
                .reader_environment(
                    DataBuilder.map(&[]).unwrap(),
                    "demo",
                    resolution,
                    &globals,
                    shared,
                    &ids,
                )
                .unwrap();
            (field(&env, "light-cljs/reader-context"), registration)
        };
        let (first, ack) = context(&host, &resolution, 2, 2);
        let (repeated, _) = context(&host, &resolution, 2, 2);
        assert_eq!(first.identity(), repeated.identity());
        assert_eq!(
            field(&field(&first, "declared"), "names")
                .elements()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(field(&first, "bindings").elements().unwrap().len(), 1);
        host.register_reader_declarations(ack);
        let (registered, _) = context(&host, &resolution, 2, 2);
        assert_ne!(first.identity(), registered.identity());
        assert!(
            field(&field(&registered, "declared"), "names")
                .elements()
                .unwrap()
                .is_empty()
        );
        let (older, ack) = context(&host, &resolution, 1, 1);
        assert!(ack.is_none());
        assert!(matches!(
            field(&older, "visible").kind(),
            DatumKind::Integer(1)
        ));
        assert_eq!(field(&older, "bindings").elements().unwrap().len(), 2);
        resolution
            .refers
            .push(("renamed".into(), "other".into(), "original".into()));
        resolution.excludes.push("map".into());
        let (changed, _) = context(&host, &resolution, 2, 2);
        assert_ne!(registered.identity(), changed.identity());
        assert_eq!(field(&changed, "refers").elements().unwrap().len(), 1);
        assert_eq!(field(&changed, "excludes").elements().unwrap().len(), 1);
        host.begin_compilation();
        let (fresh, _) = context(&host, &resolution, 2, 2);
        assert_ne!(
            field(&field(&first, "declared"), "generation").inspect(),
            field(&field(&fresh, "declared"), "generation").inspect()
        );
        assert_eq!(
            field(&field(&fresh, "declared"), "names")
                .elements()
                .unwrap()
                .len(),
            2
        );
    }

    #[test]
    fn reader_seed_declarations_and_metadata_use_owned_forms() {
        let mut host = MacroHost::new();
        for text in [
            "(let [{:keys [x] :or {x 4}} {}] (assert (= x 4)) x)",
            "(defn f [x] {:pre [(number? x)]} (+ x 1)) (f 2)",
            "(loop [x 2] (if (= x 0) x (recur (dec x))))",
            "(letfn [(f [x] (if (= x 0) x (f (dec x))))] (f 2))",
            "(defprotocol P (value [this])) (deftype T [] P (value [this] 3)) (value (T.))",
            "(defrecord R [x]) (:x (->R 3))",
            "(meta #'+)",
            "(def x 1) (ns-publics 'cljs.user)",
            "`[+ unknown ~(+ 1 2) ~@[3]]",
        ] {
            crate::compile_with_macros(text, &mut host).unwrap_or_else(|e| panic!("{text}: {e}"));
        }
        host.set_imports(
            "cljs.user",
            vec![MacroImport {
                namespace: "cljs.core".into(),
                alias: Some("c".into()),
                refers: vec!["let".into(), "ns-publics".into()],
            }],
        )
        .unwrap();
        crate::compile_with_macros("(c/let [x 3] x)", &mut host).unwrap();
        let source = crate::compiler_data::Source::new("reader.cljs", "^:kept #:x{:z [1] :a 2}");
        let form = host.read_compiler_form(&source).unwrap().unwrap();
        assert_eq!(form.children()[0].span().text(), ":z");
        assert!(form.metadata().is_some());
    }
}

impl MacroHost {
    pub(crate) fn check_macro_policy(
        &self,
        namespace: &str,
        target: &str,
        shadow_core: bool,
    ) -> Result<(), String> {
        let Some(policy) = &self.tables.name_policy else {
            return Ok(());
        };
        let name = self.source_macro_key(namespace, target).or_else(|| {
            self.macro_name(namespace, target, shadow_core)
                .map(|(ns, name)| format!("{ns}/{name}"))
        });
        if let Some(name) = name {
            let (ns, member) = name.split_once('/').unwrap();
            if !policy.permits(&Name::new(Some(ns), member), false) {
                return Err(format!("name policy denies {name}"));
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod compiler_default_reader_tests {
    use super::*;
    use crate::compiler_data::{DatumKind, Source};

    #[test]
    fn default_reader_is_compiled_typed_and_preserves_reader_precedence() {
        fn number(form: &Form) -> f64 {
            match form.data().kind() {
                DatumKind::Integer(n) => *n as f64,
                DatumKind::Double(n) => *n,
                _ => panic!("expected number"),
            }
        }
        let mut host = MacroHost::new();
        host.register_compiler_default_data_reader("(fn [tag value] [(str tag) value])")
            .unwrap();
        host.register_compiler_data_reader(Name::new(None, "named"), "(fn [value] (+ value 1))")
            .unwrap();
        let read = |host: &mut MacroHost, text: &str| {
            host.read_compiler_form(&Source::new("reader.cljs", text))
                .unwrap()
                .unwrap()
        };
        let form = read(&mut host, "#demo/missing 7");
        let children = form.children();
        assert!(
            matches!(children[0].data().kind(), DatumKind::String(text) if String::from_utf16_lossy(text) == "demo/missing")
        );
        assert_eq!(number(&children[1]), 7.0);
        assert_eq!(number(&read(&mut host, "#named 7")), 8.0);
        assert!(matches!(
            read(&mut host, "#inst \"2020-01-01\"").data().kind(),
            DatumKind::CompilerLiteral { .. }
        ));
        host.begin_compilation();
        assert!(host.engine.data_readers.borrow().default_function.is_none());
        assert_eq!(read(&mut host, "#unknown 9").children().len(), 2);
        host.register_compiler_default_data_reader("(fn [tag value] 42)")
            .unwrap();
        assert_eq!(number(&read(&mut host, "#unknown 9")), 42.0);
    }
}
