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
#[derive(Default)]
pub struct MacroHost {
    compiler_data_readers: std::rc::Rc<std::cell::RefCell<CompilerDataReaders>>,
    imports: crate::layered::Layered<String, Vec<MacroImport>>,
    renames: crate::layered::Layered<String, Vec<(String, String, String)>>,
    namespace_symbols: crate::layered::Layered<String, crate::compiler_data::Datum>,
    namespace_docs: crate::layered::Layered<String, crate::compiler_data::Datum>,
    /// By `namespace/name`, whether the Var is private.
    runtime_vars: crate::layered::Layered<String, bool>,
    namespace_imports: crate::layered::Layered<String, Vec<(String, String)>>,
    pub limits: MacroLimits,
    /// Configuration only: JS roots belong to each fresh compilation session.
    pub evaluator_options: crate::evaluator::EvaluatorOptions,
    assertions_disabled: bool,
    pub(crate) interactive: bool,
    pub(crate) interactive_control: Option<crate::evaluator::ExecutionControl>,
    pub(crate) name_policy: Option<crate::eval_compiler::NamePolicy>,
    pub(crate) bootstrap: Option<crate::compiler_bootstrap::Bootstrap>,
    pub(crate) bootstrap_seed: bool,
    pub(crate) javascript_macros:
        crate::layered::Layered<String, crate::compiler_bootstrap::CompiledMacro>,
    /// The names of `javascript_macros` by namespace, sorted
    /// ([`Self::source_macro_names`]); change both through
    /// [`Self::insert_javascript_macro`].
    macro_names: crate::layered::Layered<String, Vec<String>>,
    /// Shared by frames with the same macro inventory; replaced on declaration.
    pub(crate) macro_environment:
        std::rc::Rc<std::cell::RefCell<Option<crate::compiler_data::Datum>>>,
    namespace_environment: std::rc::Rc<std::cell::RefCell<Option<NamespaceEnvironment>>>,
    // Shared by analysis frames, but bounded to one complete lexical view.
    local_environment: std::rc::Rc<std::cell::RefCell<Option<LocalEnvironment>>>,
    reader_environment: std::rc::Rc<std::cell::RefCell<Option<ReaderEnvironment>>>,
    pub(crate) source_bindings: crate::layered::Layered<String, SourceBindingKind>,
    pub(crate) session: Option<std::rc::Rc<crate::compilation_session::CompilationSession>>,
    pub(crate) session_ref: std::rc::Weak<crate::compilation_session::CompilationSession>,
    pub(crate) expansions: std::rc::Rc<std::cell::Cell<usize>>,
    pub(crate) generated_nodes: std::rc::Rc<std::cell::Cell<usize>>,
    /// The compilation's high-water marks and charges, shared like the
    /// counters ([`WorkMarks`]).
    pub(crate) work: std::rc::Rc<WorkMarks>,
    /// Give compile-time heaps an opaque JavaScript package registry: every
    /// package, export, call and construction answers an inert stub, so the
    /// initializers that macro expansion demands (such as an extension whose
    /// target type a package builds) run without the host's packages, which
    /// only the application has.
    pub opaque_packages: bool,
    /// The engine clean compilations run their macros in instead of the
    /// built-in ones ([`Self::set_script_host`]).
    pub(crate) script_host: Option<std::rc::Rc<dyn crate::host_evaluator::ScriptHost>>,
    /// Per namespace, the globals registered as its declared bindings in the
    /// compile-time session: the position after the last one registered and
    /// how many of the namespace's own globals were among them. Shared by
    /// the analysis frames of one compilation.
    pub(crate) reader_declared: std::rc::Rc<std::cell::RefCell<HashMap<String, (usize, usize)>>>,
    /// The compilation whose declared bindings the session holds.
    pub(crate) reader_generation: u64,
    /// While a dependency manifest is recorded, what analysis observes
    /// beyond the `ns` forms; shared by the analysis frames of the
    /// compilation ([`crate::dependency_manifest`]).
    pub(crate) dependency_log:
        std::rc::Rc<std::cell::RefCell<Option<crate::dependency_manifest::ObservationLog>>>,
    /// The compilation `light.build` serves, shared by the analysis frames
    /// and the services installed in its heaps ([`crate::build_services`]).
    pub(crate) build: crate::build_services::SharedBuild,
    /// The build configuration `light.build/config` answers
    /// ([`Self::set_build_configuration`]).
    pub(crate) configuration: std::rc::Rc<crate::build_services::BuildConfiguration>,
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

#[derive(Default)]
struct CompilerDataReaders {
    units: HashMap<Name, String>,
    default_unit: Option<String>,
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
    pub(crate) fn new_compiler_evaluator(
        &self,
    ) -> Result<
        std::rc::Rc<dyn crate::compiler_evaluator::CompilerEvaluator>,
        crate::evaluator::EvalError,
    > {
        if let Some(host) = &self.script_host
            && !self.interactive
        {
            use crate::evaluator::MacroEvaluator;
            let evaluator =
                crate::host_evaluator::HostEvaluator::new(host.clone(), &self.evaluator_options)?;
            if self.assertions_disabled {
                let done = evaluator
                    .evaluate_unit("rt => { rt.core.assertions = false; return null; }")?;
                evaluator.release(done)?;
            }
            if self.opaque_packages {
                let done = evaluator.evaluate_unit(OPAQUE_PACKAGES_PRELUDE)?;
                evaluator.release(done)?;
            }
            return Ok(std::rc::Rc::new(evaluator));
        }
        #[cfg(light_v8)]
        if !self.interactive {
            use crate::evaluator::MacroEvaluator;
            let evaluator = crate::v8_evaluator::V8Evaluator::new(self.evaluator_options.clone())?;
            if self.assertions_disabled {
                let done = evaluator
                    .evaluate_unit("rt => { rt.core.assertions = false; return null; }")?;
                evaluator.release(done)?;
            }
            if self.opaque_packages {
                let done = evaluator.evaluate_unit(OPAQUE_PACKAGES_PRELUDE)?;
                evaluator.release(done)?;
            }
            return Ok(std::rc::Rc::new(evaluator));
        }
        Ok(std::rc::Rc::new(self.new_evaluator()?))
    }
    /// Allocate a fresh synchronous heap for a clean compilation. Its dynamic
    /// `*assert*` follows [`Self::set_assertions`], so the macros, functions and
    /// data readers it runs read the setting, as upstream's `:elide-asserts`
    /// binds `clojure.core/*assert*` while compiling.
    pub fn new_evaluator(
        &self,
    ) -> Result<crate::evaluator::Evaluator, crate::evaluator::EvalError> {
        let evaluator = if let Some(control) = &self.interactive_control {
            crate::evaluator::Evaluator::interactive_with_control(
                self.evaluator_options.clone(),
                control.clone(),
            )
        } else if self.interactive {
            crate::evaluator::Evaluator::interactive(self.evaluator_options.clone())
        } else {
            crate::evaluator::Evaluator::new(self.evaluator_options.clone())
        }?;
        if self.assertions_disabled {
            let done =
                evaluator.evaluate_unit("rt => { rt.core.assertions = false; return null; }")?;
            evaluator.release(done)?;
        }
        if self.opaque_packages {
            let done = evaluator.evaluate_unit(OPAQUE_PACKAGES_PRELUDE)?;
            evaluator.release(done)?;
        }
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
        if self.expansions.get() >= self.limits.expansions {
            return Err("compiler macro expansion limit exceeded".into());
        }
        self.expansions.set(self.expansions.get() + 1);
        // The conversions below take at most the remaining node budget each,
        // so a namespace needs room for the largest of its inputs too.
        if self.work.measuring.get() {
            let input =
                WorkMarks::nodes(&call.data()).max(self.work.environment_nodes(environment));
            WorkMarks::note(
                &self.work.node_peak,
                self.generated_nodes.get().saturating_add(input),
            );
        }
        let limits = crate::form_bridge::ConversionLimits {
            nodes: self
                .limits
                .generated_nodes
                .saturating_sub(self.generated_nodes.get()),
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
        let expanded = run();
        // What the conversions charged on the JavaScript side, which bounds
        // them by the same remaining budget.
        if self.work.measuring.get() && expanded.is_ok() {
            match take_encode_peak(evaluator, &self.work.encode_peak) {
                Some(peak) => WorkMarks::note(
                    &self.work.node_peak,
                    self.generated_nodes.get().saturating_add(peak),
                ),
                None => self.work.unmeasured.set(true),
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
        self.generated_nodes.set(
            self.generated_nodes
                .get()
                .saturating_add(expanded.nodes().len()),
        );
        if self.generated_nodes.get() > self.limits.generated_nodes {
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
        self.script_host = Some(host);
    }
    /// Control assertion emission for this host (enabled by default). Disabled
    /// assertions omit both condition and message, including function pre/post checks.
    pub fn set_assertions(&mut self, enabled: bool) {
        self.assertions_disabled = !enabled;
    }
    pub fn assertions_enabled(&self) -> bool {
        !self.assertions_disabled
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
        let (build, log) = (self.build.clone(), self.dependency_log.clone());
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
        let mut readers = self.compiler_data_readers.borrow_mut();
        readers.units.insert(tag.clone(), unit);
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
        let mut readers = self.compiler_data_readers.borrow_mut();
        readers.default_unit = Some(crate::emitter::emit_js_unit(&program));
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
        let readers = self.compiler_data_readers.borrow();
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
        let Some(policy) = &self.name_policy else {
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
        let run = || -> Result<Option<crate::compiler_data::Datum>, String> {
            let (evaluator, function, fallback) = {
                let mut readers = self.compiler_data_readers.borrow_mut();
                let fallback = !readers.units.contains_key(tag);
                let unit = if fallback {
                    if tag.namespace().is_none()
                        && matches!(
                            String::from_utf16_lossy(tag.name()).as_ref(),
                            "inst" | "uuid" | "js" | "queue"
                        )
                    {
                        return Ok(None);
                    }
                    readers.default_unit.clone()
                } else {
                    readers.units.get(tag).cloned()
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
            let measuring = self.work.measuring.get();
            if measuring {
                WorkMarks::note(&self.work.conversion_peak, WorkMarks::nodes(value));
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
            let result = if let Some(tag_argument) = tag_argument {
                evaluator.call(function, &[tag_argument, argument])
            } else {
                evaluator.call(function, &[argument])
            };
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
                let readers = self.compiler_data_readers.borrow();
                match take_encode_peak(&*evaluator, &readers.encode_peak) {
                    Some(peak) => WorkMarks::note(&self.work.conversion_peak, peak),
                    None => self.work.unmeasured.set(true),
                }
            }
            data.map(Some)
        };
        let result = run();
        if result.is_err() {
            let mut readers = self.compiler_data_readers.borrow_mut();
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
                let generated = self.generated_nodes.get().saturating_add(size);
                if generated > self.limits.generated_nodes {
                    return Err("compiler reader generated-node limit exceeded".into());
                }
                self.generated_nodes.set(generated);
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
        let current = self.imports.entry_or_default(caller.to_owned());
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
        let previous = self.imports.remove(caller);
        let result = self.set_interactive_imports(caller, imports);
        if result.is_err() {
            match previous {
                Some(previous) => self.imports.insert(caller.to_owned(), previous),
                None => self.imports.remove(caller),
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
        self.imports.insert(caller.to_owned(), imports);
        self.renames.remove(caller);
        self.namespace_symbols.remove(caller);
        self.namespace_docs.remove(caller);
        Ok(())
    }
    pub(crate) fn set_namespace_symbol(
        &mut self,
        namespace: &str,
        symbol: crate::compiler_data::Datum,
        doc: Option<crate::compiler_data::Datum>,
    ) {
        self.namespace_symbols.insert(namespace.to_owned(), symbol);
        if let Some(doc) = doc {
            self.namespace_docs.insert(namespace.to_owned(), doc);
        }
    }
    /// `namespace`'s symbol as its `ns` form named it, with the form's
    /// metadata, docstring and attribute map as its metadata.
    pub(crate) fn namespace_symbol(&self, namespace: &str) -> Option<&crate::compiler_data::Datum> {
        self.namespace_symbols.get(namespace)
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
        self.namespace_symbols.remove(namespace);
        self.namespace_docs.remove(namespace);
        if let Some((symbol, doc)) = symbol {
            self.set_namespace_symbol(namespace, symbol, doc);
        }
    }
    /// What `prepare_source` and analysis declared for `namespace`: its
    /// imports, renamed macros and `:import`ed host names, and its `ns`
    /// form's docstring ([`crate::artifact_capture`]).
    pub(crate) fn namespace_declarations(&self, namespace: &str) -> NamespaceDeclarations<'_> {
        NamespaceDeclarations {
            imports: self.imports.get(namespace).map_or(&[], Vec::as_slice),
            renames: self.renames.get(namespace).map_or(&[], Vec::as_slice),
            namespace_imports: self
                .namespace_imports
                .get(namespace)
                .map_or(&[], Vec::as_slice),
            doc: self.namespace_docs.get(namespace),
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
        self.imports.insert(namespace.to_owned(), imports);
        self.renames.insert(namespace.to_owned(), renames);
        self.namespace_imports
            .insert(namespace.to_owned(), namespace_imports);
        self.namespace_symbols.remove(namespace);
        self.namespace_docs.remove(namespace);
        if let Some(symbol) = symbol {
            self.namespace_symbols.insert(namespace.to_owned(), symbol);
        }
        if let Some(doc) = doc {
            self.namespace_docs.insert(namespace.to_owned(), doc);
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
        self.macro_environment = Default::default();
    }
    /// Whether `namespace/name` is a Var of the compile-time session, and
    /// whether it is private.
    pub(crate) fn runtime_var(&self, key: &str) -> Option<bool> {
        self.runtime_vars.get(key).copied()
    }
    pub(crate) fn set_renames(&mut self, caller: &str, renames: Vec<(String, String, String)>) {
        self.renames.insert(caller.to_owned(), renames);
    }
    pub(crate) fn macro_analysis_frame(&self) -> Self {
        Self {
            compiler_data_readers: self.compiler_data_readers.clone(),
            imports: self.imports.clone(),
            renames: self.renames.clone(),
            namespace_symbols: self.namespace_symbols.clone(),
            namespace_docs: self.namespace_docs.clone(),
            runtime_vars: self.runtime_vars.clone(),
            namespace_imports: self.namespace_imports.clone(),
            limits: self.limits,
            evaluator_options: self.evaluator_options.clone(),
            assertions_disabled: self.assertions_disabled,
            interactive: self.interactive,
            interactive_control: self.interactive_control.clone(),
            name_policy: self.name_policy.clone(),
            bootstrap: self.bootstrap.clone(),
            bootstrap_seed: true,
            javascript_macros: self.javascript_macros.clone(),
            macro_names: self.macro_names.clone(),
            macro_environment: self.macro_environment.clone(),
            namespace_environment: self.namespace_environment.clone(),
            local_environment: self.local_environment.clone(),
            reader_environment: self.reader_environment.clone(),
            source_bindings: self.source_bindings.clone(),
            session_ref: self
                .session_handle()
                .map_or_else(std::rc::Weak::new, |s| std::rc::Rc::downgrade(&s)),
            expansions: self.expansions.clone(),
            generated_nodes: self.generated_nodes.clone(),
            work: self.work.clone(),
            opaque_packages: self.opaque_packages,
            script_host: self.script_host.clone(),
            reader_declared: self.reader_declared.clone(),
            reader_generation: self.reader_generation,
            dependency_log: self.dependency_log.clone(),
            build: self.build.clone(),
            configuration: self.configuration.clone(),
            ..Self::default()
        }
    }
    /// The build configuration `light.build/config` answers in the
    /// compilations this host analyzes, as `--config KEY=EDN` gives it.
    /// Every namespace they record in a dependency manifest depends on its
    /// fingerprint.
    pub fn set_build_configuration(
        &mut self,
        configuration: crate::build_services::BuildConfiguration,
    ) {
        self.configuration = std::rc::Rc::new(configuration);
    }
    pub fn build_configuration(&self) -> &crate::build_services::BuildConfiguration {
        &self.configuration
    }
    /// Log an observation for the dependency manifest, if one is recorded.
    pub(crate) fn observe(
        &self,
        record: impl FnOnce(&mut crate::dependency_manifest::ObservationLog),
    ) {
        if let Some(log) = self.dependency_log.borrow_mut().as_mut() {
            record(log);
        }
    }
    pub(crate) fn defers_source_units(&self) -> bool {
        !self.bootstrap_seed
    }
    /// Begin measuring the next namespace's logical work
    /// ([`WorkMarks`]): its counts start here, and its marks from nothing.
    pub(crate) fn begin_namespace_work(&self) -> WorkStart {
        let work = &self.work;
        work.measuring.set(true);
        work.node_peak.set(self.generated_nodes.get());
        work.macro_depth.set(0);
        work.analysis_depth.set(0);
        work.unit_depth.set(0);
        work.conversion_peak.set(0);
        work.unmeasured.set(false);
        WorkStart {
            expansions: self.expansions.get(),
            generated_nodes: self.generated_nodes.get(),
        }
    }
    /// The macro calls the compilation executed: its expansions less those
    /// installed artifacts charged.
    pub(crate) fn executed_expansions(&self) -> usize {
        self.expansions.get() - self.work.charged_expansions.get()
    }
    /// Why the compilation's compile-time work cannot go on: its session's
    /// execution control, or `control`, is cancelled or terminal. Checked
    /// without spending the poll budget ([`crate::evaluator::ExecutionControl::check`]).
    pub(crate) fn halted(
        &self,
        control: Option<&crate::evaluator::ExecutionControl>,
    ) -> Option<String> {
        let session = self
            .interactive_control
            .clone()
            .or_else(|| self.bootstrap.as_ref().map(|b| b.evaluator.control()));
        control
            .into_iter()
            .chain(session.as_ref())
            .find_map(|control| control.check().err())
            .map(|error| error.0)
    }
    pub(crate) fn session_handle(
        &self,
    ) -> Option<std::rc::Rc<crate::compilation_session::CompilationSession>> {
        self.session.clone().or_else(|| self.session_ref.upgrade())
    }
    pub(crate) fn has_source_value(&self, namespace: &str, name: &str) -> bool {
        self.source_bindings.get(&format!("{namespace}/{name}")) == Some(&SourceBindingKind::Value)
    }
    pub(crate) fn begin_compilation(&mut self) {
        if !self.bootstrap_seed {
            let mut readers = self.compiler_data_readers.borrow_mut();
            readers.evaluator = None;
            readers.functions.clear();
            readers.default_function = None;
            drop(readers);
            self.bootstrap = None;
            self.javascript_macros.clear();
            self.macro_names.clear();
            self.macro_environment = Default::default();
            self.namespace_environment = Default::default();
            self.local_environment = Default::default();
            self.reader_environment = Default::default();
            self.source_bindings.clear();
            self.session = Some(crate::compilation_session::CompilationSession::new());
            self.session_ref = std::rc::Weak::new();
        }
        self.expansions = Default::default();
        self.generated_nodes = Default::default();
        self.work = Default::default();
        self.reader_declared = Default::default();
        self.reader_generation =
            READER_GENERATION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.runtime_vars.clear();
        self.namespace_imports.clear();
    }
    pub(crate) fn register_source_binding(
        &mut self,
        namespace: &str,
        name: &str,
        kind: SourceBindingKind,
    ) -> Result<(), String> {
        let key = format!("{namespace}/{name}");
        if self
            .source_bindings
            .get(&key)
            .is_some_and(|previous| *previous != kind)
        {
            return Err(format!(
                "binding {key} cannot change between an ordinary Var and a compile-time macro; use distinct names for the two phases"
            ));
        }
        self.source_bindings.insert(key, kind);
        Ok(())
    }
    pub(crate) fn register_runtime_var(
        &mut self,
        namespace: &str,
        name: &str,
        private: bool,
    ) -> Result<(), String> {
        self.register_source_binding(namespace, name, SourceBindingKind::Value)?;
        self.runtime_vars
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
        self.imports
            .get(caller)
            .into_iter()
            .flatten()
            .find(|i| i.alias.as_deref() == Some(qualifier) || i.namespace == qualifier)
            .map(|i| i.namespace.as_str())
    }
    pub(crate) fn source_binding_private(&self, namespace: &str, name: &str) -> bool {
        if self
            .runtime_vars
            .get(&format!("{namespace}/{name}"))
            .copied()
            .unwrap_or(false)
        {
            return true;
        }
        self.javascript_macros
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
        self.namespace_imports.insert(namespace.to_owned(), imports);
    }
    pub(crate) fn definition_macro(&self, caller: &str, operator: &str) -> Option<String> {
        let imports = self.imports.get(caller).map(Vec::as_slice).unwrap_or(&[]);
        let target = if let Some((qualifier, name)) = operator.split_once('/') {
            imports
                .iter()
                .find(|i| i.namespace == qualifier || i.alias.as_deref() == Some(qualifier))
                .map(|i| (i.namespace.as_str(), name))
        } else if let Some((_, ns, name)) = self
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
        let imports = self.imports.get(caller).map(Vec::as_slice).unwrap_or(&[]);
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
    pub(crate) fn expand_compiler_seed(
        &mut self,
        form: &Form,
        name: &Name,
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
            if self.expansions.get() >= self.limits.expansions {
                return Err("compiler macro expansion limit exceeded".into());
            }
            self.expansions.set(self.expansions.get() + 1);
            self.generated_nodes.set(
                self.generated_nodes
                    .get()
                    .saturating_add(expanded.nodes().len()),
            );
            if self.generated_nodes.get() > self.limits.generated_nodes {
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
            && let Some(declaration) = self.javascript_macros.get(&key)
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
                self.namespace_imports
                    .get(&namespace)
                    .cloned()
                    .unwrap_or_default()
            } else {
                self.runtime_vars
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
                declarations.extend(self.javascript_macros.keys().filter_map(|key| {
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
        let mut names = self.javascript_macros.keys().collect::<Vec<_>>();
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
        names.extend(self.renames.get(namespace).into_iter().flatten().cloned());
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
        if let Some(imports) = self.imports.get(namespace) {
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
        self.macro_names.get(namespace).cloned().unwrap_or_default()
    }
    /// Declare the source macro `key` (`namespace/name`).
    pub(crate) fn insert_javascript_macro(
        &mut self,
        key: String,
        declaration: crate::compiler_bootstrap::CompiledMacro,
    ) {
        if !self.javascript_macros.contains_key(&key)
            && let Some((namespace, name)) = key.split_once('/')
        {
            let names = self.macro_names.entry_or_default(namespace.to_owned());
            if let Err(position) = names.binary_search_by(|old| old.as_str().cmp(name)) {
                names.insert(position, name.to_owned());
            }
        }
        self.javascript_macros.insert(key, declaration);
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
            generation: self.reader_generation,
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
            let mut cache = self.reader_environment.borrow_mut();
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
        let mut declared = self.reader_declared.borrow_mut();
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
        let imports = self.imports.get(namespace).cloned().unwrap_or_default();
        let namespace_symbol = self.namespace_symbols.get(namespace).cloned();
        let namespace_doc = self.namespace_docs.get(namespace).cloned();
        let namespace_imports = self
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
            let mut cache = self.macro_environment.borrow_mut();
            if let Some(data) = cache.as_ref() {
                data.clone()
            } else {
                let data = c.map(&{
                    let mut declarations = self.javascript_macros.iter().collect::<Vec<_>>();
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
                *cache = Some(data.clone());
                data
            }
        };
        let namespace_key = NamespaceEnvironmentKey {
            namespace: namespace.to_owned(),
            aliases: aliases.to_vec(),
            requires: requires.to_vec(),
            imports: imports.clone(),
            renames: self.renames.get(namespace).cloned().unwrap_or_default(),
            namespace_imports: namespace_imports.clone(),
            symbol: namespace_symbol.as_ref().map(|d| d.identity()),
            doc: namespace_doc.as_ref().map(|d| d.identity()),
        };
        let namespace_environment = {
            let mut cache = self.namespace_environment.borrow_mut();
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
                for (local, target, _) in self.renames.get(namespace).into_iter().flatten() {
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
                let mut cache = self.local_environment.borrow_mut();
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
        let Some(policy) = &self.name_policy else {
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
        assert!(
            host.compiler_data_readers
                .borrow()
                .default_function
                .is_none()
        );
        assert_eq!(read(&mut host, "#unknown 9").children().len(), 2);
        host.register_compiler_default_data_reader("(fn [tag value] 42)")
            .unwrap();
        assert_eq!(number(&read(&mut host, "#unknown 9")), 42.0);
    }
}
