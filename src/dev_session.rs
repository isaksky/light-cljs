//! The session compiler as an action producer (docs/dev-workflow.md,
//! "Actions"): a [`DevSession`] owns the incremental compiler of a session,
//! its macro host and the namespaces it has loaded, and no application heap.
//! Compiling a form or a source yields [`Action`]s, which a runtime
//! interprets: [`crate::EvalSession`] over its QuickJS heap, or a client that
//! sends the units to a browser tab or a Node process.
//!
//! The compiler works one top-level form at a time and hands each action to
//! an interpreter as soon as it has one, the way the session has always
//! evaluated: a `require` compiles a loaded namespace's form, runs it, then
//! reads and compiles the next. An interpreter in the same process, as the
//! session's heap is, therefore stops the compilation at the first unit that
//! fails, and a definition compiled after it never exists. The methods of
//! [`DevSession`] collect the actions instead, for a runtime elsewhere: the
//! compiler never waits on such a runtime.
use crate::analyzer::{LAZY_NAMESPACE, RuntimeReaderNamespace};
use crate::compiler_data::{DatumKind, Form, Name, SourceSpan};
use crate::eval_compiler::InteractiveCompiler;
use crate::eval_session::{EvalDiagnostic, EvalOptions};
use crate::evaluator::{EvalError, EvaluatorOptions, ExecutionControl};
use crate::source_map::{SourceFile, SourceMapping, json_string};
use crate::source_provider::{LoadRequest, Reload, SourceInput, SourceProvider};
use std::{
    cell::{Cell, Ref, RefCell, RefMut},
    collections::{HashMap, HashSet, VecDeque},
    rc::Rc,
};

/// One compiled top-level form: an interactive unit, the JavaScript function
/// expression `(__lightInteractiveRuntime) => ...` of the runtime whose
/// definitions live in the application registry,
/// `globalThis.__lightApplicationRoots["ns/name"]` (docs/evaluation.md).
#[derive(Clone, Debug)]
pub struct Unit {
    /// The namespace the form was compiled in.
    pub namespace: String,
    /// The unit's JavaScript.
    pub js: String,
    /// The unit's source map, when the compiler made one; the QuickJS
    /// session ignores it.
    pub source_map: Option<String>,
    /// The form, at which an error the unit throws is located.
    pub location: SourceSpan,
    /// The table of `source_map`, whose source 0 is the form's source, for
    /// a writer that puts units together ([`crate::dev_build::namespace_unit`]).
    pub(crate) mapping: Option<SourceMapping>,
}

/// What a runtime does with the result of compiling one input, in order.
#[derive(Clone, Debug)]
pub enum Action {
    /// Evaluate this unit for its value: a REPL form.
    Invoke(Unit),
    /// Run these units in order: the forms of a namespace a `require` loads,
    /// with those of the namespaces it loads first, or of a loaded file.
    Load(Vec<Unit>),
    /// The current namespace is now this one: an `ns` or `in-ns`, a
    /// `require` (which stays in its namespace), the namespace a load enters,
    /// or the one it returns to.
    Namespace(String),
    /// Analysis warnings for the input, located.
    Warnings(Vec<EvalDiagnostic>),
    /// The input did not compile; nothing was emitted for the failing form or
    /// after it. Always the last action.
    Error(EvalDiagnostic),
}

/// A runtime that applies actions as the compiler produces them.
pub(crate) trait Interpreter {
    /// A top-level form begins (each form of a top-level `do` that holds an
    /// `ns*` form is its own).
    fn begin_form(&mut self) -> Result<(), EvalDiagnostic> {
        Ok(())
    }
    /// Apply `action`; an error stops the compilation.
    fn apply(&mut self, action: Action) -> Result<(), EvalDiagnostic>;
}

/// The interpreter of the collecting methods: keeps every action, merging
/// consecutive `Load`s.
#[derive(Default)]
struct Collect(Vec<Action>);
impl Interpreter for Collect {
    fn apply(&mut self, action: Action) -> Result<(), EvalDiagnostic> {
        match (self.0.last_mut(), action) {
            (Some(Action::Load(units)), Action::Load(more)) => units.extend(more),
            (_, action) => self.0.push(action),
        }
        Ok(())
    }
}

/// How the units of the forms are delivered: a REPL form's value is wanted
/// (`Invoke`), a loaded form's is not (`Load`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Repl,
    Load,
}

/// The `light.lazy/loadable` references the session has analyzed but not
/// acted on: the namespaces to load, each with the site that named it first,
/// and the Var targets to check once they are loaded, with their sites.
#[derive(Default)]
struct LazyQueue {
    loads: VecDeque<(String, SourceSpan)>,
    checks: Vec<(String, String, SourceSpan)>,
}

/// The incremental compiler of a session without its heap: reads, compiles
/// and loads as `EvalSession` does, and returns the actions for a runtime to
/// apply. It keeps the namespaces it has loaded: a `require` of one loads
/// nothing unless it says `:reload` or `:reload-all`.
pub struct DevSession {
    compiler: RefCell<InteractiveCompiler>,
    source_provider: Option<Rc<dyn SourceProvider>>,
    max_source_bytes: usize,
    /// What the compiler and its macro heap poll: the control of the heap
    /// that interprets the actions in process, or the session's own.
    control: ExecutionControl,
    /// The limits of each operation when the session owns its control: each
    /// public method then begins a new deadline and poll budget, as
    /// `EvalSession` does for its heap's.
    operation: Option<EvaluatorOptions>,
    loaded: RefCell<HashSet<String>>,
    loading: RefCell<Vec<String>>,
    lazy: RefCell<LazyQueue>,
    /// While a `:reload-all` request loads, the namespaces it has loaded:
    /// it loads each namespace it reaches once, loaded before or not.
    reload_all: RefCell<Option<HashSet<String>>>,
    /// Each namespace as the last unit handed to an interpreter that
    /// registered it registered it with the runtime: what a runtime that ran
    /// the units in order knows of it ([`Registration`]).
    registered: RefCell<HashMap<String, Registration>>,
    /// Whether units register what changed ([`Self::changed_registrations`]),
    /// as they do except in [`Self::load_namespace_file`].
    registering: Cell<bool>,
}

/// What a unit registers of a namespace with the runtime: its reader
/// context (aliases, refers, excludes and macros) and its definitions, which
/// a session only adds to, by their number.
type Registration = (Option<RuntimeReaderNamespace>, usize);

/// The namespaces a unit registers, each with what it registers of it.
type Registrations = Vec<(String, Registration)>;

impl DevSession {
    /// A session compiler configured by the compiler fields of `options`:
    /// `namespace`, `aliases`, `compiler`, `engine` (for the macro heap),
    /// `data_readers`, `default_data_reader`, `source_provider`,
    /// `max_source_bytes`, `assertions` and `policy`. The other fields
    /// configure a heap. Each call of [`Self::form`], [`Self::source`],
    /// [`Self::load_file`] or [`Self::read_forms`] runs under a new deadline
    /// and poll budget of `engine`; one that runs out, or a cancelled
    /// [`Self::control`], leaves the session terminal, as an `EvalSession`.
    ///
    /// Analysis is in warning mode, as dev compilation's is
    /// (docs/dev-workflow.md, "Dev-mode contract"), whatever
    /// `options.warning_mode` says, which is an `EvalSession`'s setting;
    /// [`Self::set_warning_mode`] changes it.
    pub fn new(options: &EvalOptions) -> Result<Self, EvalDiagnostic> {
        let session = Self::with_control(options, None)?;
        session.set_warning_mode(true);
        Ok(session)
    }
    /// Whether analysis is in warning mode: undeclared Vars and conflicting
    /// arities are warnings (a `Warnings` action) and the code runs as
    /// upstream's would, instead of errors.
    pub fn warning_mode(&self) -> bool {
        self.compiler.borrow().options.warning_mode
    }
    /// Turn warning mode on or off for the forms compiled from now on.
    pub fn set_warning_mode(&self, on: bool) {
        self.compiler.borrow_mut().options.warning_mode = on;
    }
    /// With `control`, the macro host and the compiler poll the control of
    /// the heap that interprets the actions, whose owner begins operations.
    pub(crate) fn with_control(
        options: &EvalOptions,
        control: Option<ExecutionControl>,
    ) -> Result<Self, EvalDiagnostic> {
        let (control, operation) = match control {
            Some(control) => (control, None),
            None => (
                ExecutionControl::new(&options.engine)?,
                Some(options.engine.clone()),
            ),
        };
        let mut host = crate::MacroHost::new();
        host.limits = options.compiler;
        host.evaluator_options = options.engine.clone();
        host.interactive_control = Some(control.clone());
        for (tag, source) in &options.data_readers {
            host.register_compiler_data_reader(tag.clone(), source)
                .map_err(EvalError)?;
        }
        if let Some(source) = &options.default_data_reader {
            host.register_compiler_default_data_reader(source)
                .map_err(EvalError)?;
        }
        let mut compiler = InteractiveCompiler::with_host(host);
        compiler.host.set_assertions(options.assertions);
        compiler.policy = options.policy.clone();
        compiler.options.warning_mode = options.warning_mode;
        compiler.set_namespace(&options.namespace);
        // `light.build` reads the resources of the namespaces' provider.
        compiler.serve_resources(
            options
                .source_provider
                .as_ref()
                .and_then(|provider| provider.resources()),
        );
        for (alias, target) in &options.aliases {
            compiler.set_alias(alias, target);
        }
        Ok(Self {
            compiler: RefCell::new(compiler),
            source_provider: options.source_provider.clone(),
            max_source_bytes: options.max_source_bytes,
            control,
            operation,
            loaded: RefCell::default(),
            loading: RefCell::default(),
            lazy: RefCell::default(),
            reload_all: RefCell::default(),
            registered: RefCell::default(),
            registering: Cell::new(true),
        })
    }
    /// The current namespace.
    pub fn namespace(&self) -> String {
        self.compiler.borrow().namespace().into()
    }
    /// Whether the session has loaded `namespace`: a `require` of it loads
    /// nothing unless it reloads.
    pub fn loaded(&self, namespace: &str) -> bool {
        self.loaded.borrow().contains(namespace)
    }
    /// The namespaces `namespace` requires, in the order its directives
    /// named them, without repeats: what its `ns` form requires, with what
    /// later `require` and `ns*` forms added.
    pub fn requires(&self, namespace: &str) -> Vec<String> {
        let mut requires = self.compiler.borrow().namespace_requires(namespace);
        let mut seen = HashSet::new();
        requires.retain(|required| required != namespace && seen.insert(required.clone()));
        requires
    }
    /// Whether `namespace` declares macros, so that a change to it must
    /// reach the namespaces that require it, whose units expanded them.
    pub fn declares_macros(&self, namespace: &str) -> bool {
        let prefix = format!("{namespace}/");
        self.compiler
            .borrow()
            .host
            .javascript_macros
            .keys()
            .any(|name| name.starts_with(&prefix))
    }
    /// The hook functions of the retained program, the functions whose
    /// definitions' metadata marks them `^:dev/before-load` and
    /// `^:dev/after-load` (or their `-async` variants), each list in
    /// definition order, with the namespaces ordered as `namespaces`, the
    /// load order ([`crate::dev_build::program_hooks`]).
    pub fn hooks(&self, namespaces: &[String]) -> [Vec<crate::dev_build::DevHook>; 2] {
        let compiler = self.compiler.borrow();
        compiler.program().map_or_else(Default::default, |program| {
            crate::dev_build::program_hooks(program, namespaces)
        })
    }
    /// Whether the last `ns` form of `namespace` marks it `^:dev/NAME`
    /// (`once` or `always`) with a value other than nil or false.
    pub fn namespace_flag(&self, namespace: &str, name: &str) -> bool {
        self.compiler
            .borrow()
            .host
            .namespace_symbol(namespace)
            .is_some_and(|symbol| crate::dev_build::namespace_flag(symbol, name))
    }
    /// The registration of `namespaces` with the runtime, as the session
    /// knows them now: the statements that register each one's reader
    /// context and definitions after a unit's prelude, which binds the
    /// runtime to `rt` ([`crate::emitter::emit_namespace_registration`]).
    /// The units of [`Self::load_namespace_file`] register no namespace, so
    /// a file that holds a namespace's units registers the namespace with
    /// this once ([`crate::dev_build::namespace_unit`]).
    pub(crate) fn registration(&self, namespaces: &[&str]) -> String {
        let compiler = self.compiler.borrow();
        compiler.program().map_or_else(String::new, |program| {
            crate::emitter::emit_namespace_registration(program, Some(namespaces))
        })
    }
    /// The actions of one top-level form, as a REPL evaluates it: an `Invoke`
    /// of its unit, or for a directive the `Namespace` and `Load` actions of
    /// what it loads. A failing form ends with an `Error`.
    pub fn form(&self, form: &Form) -> Vec<Action> {
        collect(|to| {
            self.begin()?;
            self.run_form(form, Mode::Repl, to)
        })
    }
    /// The actions of each top-level form of `source` in turn, as
    /// [`Self::form`]; the first form that fails ends them with an `Error`.
    pub fn source(&self, source: &SourceInput) -> Vec<Action> {
        collect(|to| {
            self.begin()?;
            self.run_source(source, Mode::Repl, to)
        })
    }
    /// The actions of loading the file `source`: its forms are units of a
    /// `Load` with those of the namespaces they require, and its directives
    /// `Namespace` actions. The current namespace stays the one the file
    /// leaves, and the file's value is the value of its last unit when that
    /// is the last action, else nil.
    pub fn load_file(&self, source: &SourceInput) -> Vec<Action> {
        collect(|to| {
            self.begin()?;
            self.run_source(source, Mode::Load, to)
        })
    }
    /// [`Self::load_file`] of `source`, the file of `namespace`, read from
    /// its first form in `namespace`, as `--compile` reads a namespace's
    /// source: a `::` keyword in its `ns` form names `namespace`, whatever
    /// namespace is current before. A watch step compiles a changed file so.
    /// Its units, and those of the namespaces it loads, register no
    /// namespace: the file of each namespace's units registers it once,
    /// with what all its forms defined ([`crate::dev_build::namespace_unit`]).
    pub fn load_namespace_file(&self, namespace: &str, source: &SourceInput) -> Vec<Action> {
        collect(|to| {
            self.begin()?;
            self.compiler.borrow_mut().set_namespace(namespace);
            let registering = self.registering.replace(false);
            let result = self.run_source(source, Mode::Load, to);
            self.registering.set(registering);
            result
        })
    }
    /// Make `namespace` the current namespace, in which the next form is read
    /// and compiled, as a connected REPL keeps one per connection.
    pub fn set_namespace(&self, namespace: &str) {
        self.compiler.borrow_mut().set_namespace(namespace);
    }
    /// Read the top-level form of `source` at `cursor` in the current
    /// namespace and move `cursor` past it: None at the end of the source.
    pub fn read_form(
        &self,
        source: &SourceInput,
        cursor: &mut usize,
    ) -> Result<Option<Form>, EvalDiagnostic> {
        self.begin()?;
        self.check_source(source)?;
        self.next_form(source, cursor)
    }
    /// Declare `namespace/name`, a definition the runtime supplies, so that
    /// forms naming it compile to a read of its registry entry without a
    /// warning: the functions of `light.dev` that the dev client defines.
    pub fn declare(&self, namespace: &str, name: &str) -> Result<(), EvalDiagnostic> {
        self.begin()?;
        // Declared from its own namespace, so that no other namespace
        // imports it.
        let current = self.namespace();
        self.compiler.borrow_mut().set_namespace(namespace);
        let result = self
            .compiler
            .borrow_mut()
            .declare_binding(namespace, name)
            .map_err(|error| EvalError(error.to_string()).into());
        self.compiler.borrow_mut().set_namespace(&current);
        result
    }
    /// Read every top-level form of `source` in the current namespace.
    pub fn read_forms(&self, source: &SourceInput) -> Result<Vec<Form>, EvalDiagnostic> {
        self.begin()?;
        self.compiler
            .borrow()
            .read_forms(source)
            .map_err(EvalDiagnostic::from)
    }
    /// The control the compiler and its macro heap poll; cancelling it
    /// leaves the session terminal.
    pub fn control(&self) -> ExecutionControl {
        self.control.clone()
    }
    /// Begin an operation of a session that owns its control.
    fn begin(&self) -> Result<(), EvalDiagnostic> {
        if let Some(options) = &self.operation {
            self.control.begin(options)?;
        }
        Ok(())
    }
    pub(crate) fn compiler(&self) -> Ref<'_, InteractiveCompiler> {
        self.compiler.borrow()
    }
    pub(crate) fn compiler_mut(&self) -> RefMut<'_, InteractiveCompiler> {
        self.compiler.borrow_mut()
    }
    fn poll(&self) -> Result<(), EvalDiagnostic> {
        Ok(self.control.poll()?)
    }
    /// Refuse a source the session cannot compile: native `.clj` code, or
    /// one over the source byte limit.
    pub(crate) fn check_source(&self, source: &SourceInput) -> Result<(), EvalDiagnostic> {
        if source.file.ends_with(".clj") {
            return Err(
                EvalError("native .clj evaluation is retired; use .cljs or .cljc".into()).into(),
            );
        }
        if source.source.len() > self.max_source_bytes {
            return Err(EvalError("compiler source byte limit exceeded".into()).into());
        }
        Ok(())
    }
    /// Read the next top-level form of `source` at `cursor`.
    pub(crate) fn next_form(
        &self,
        source: &SourceInput,
        cursor: &mut usize,
    ) -> Result<Option<Form>, EvalDiagnostic> {
        self.poll()?;
        self.compiler
            .borrow()
            .next_form(source, cursor)
            .map_err(EvalDiagnostic::from)
    }
    /// Compile each top-level form of `source` in turn, each read after the
    /// previous one's actions were applied.
    pub(crate) fn run_source(
        &self,
        source: &SourceInput,
        mode: Mode,
        to: &mut dyn Interpreter,
    ) -> Result<(), EvalDiagnostic> {
        self.check_source(source)?;
        let mut cursor = 0;
        while let Some(form) = self.next_form(source, &mut cursor)? {
            self.run_form(&form, mode, to)?;
        }
        Ok(())
    }
    /// Compile one top-level form and apply its actions. A top-level `do`
    /// that holds an `ns*` form is a sequence of top-level forms.
    pub(crate) fn run_form(
        &self,
        form: &Form,
        mode: Mode,
        to: &mut dyn Interpreter,
    ) -> Result<(), EvalDiagnostic> {
        if let Some(children) = top_level_do(form) {
            for child in &children {
                self.run_form(child, mode, to)?;
            }
            return Ok(());
        }
        if self.loading.borrow().is_empty() {
            // What an earlier top-level form recorded and failed to act on.
            *self.lazy.borrow_mut() = LazyQueue::default();
        }
        to.begin_form()?;
        let result = self.compile_form(form, mode, to);
        if result.is_err() {
            // The warnings of a directive or an analysis that failed.
            self.flush_warnings(form, to)?;
        }
        result
    }
    /// The body of [`Self::run_form`]: a directive's loads, or the form's
    /// unit with the namespaces it loads lazily first. The form's warnings
    /// come before what it loads, whose forms report their own.
    fn compile_form(
        &self,
        form: &Form,
        mode: Mode,
        to: &mut dyn Interpreter,
    ) -> Result<(), EvalDiagnostic> {
        let directive = self.compiler.borrow_mut().directive(form)?;
        self.flush_warnings(form, to)?;
        if let Some((requests, staged)) = directive {
            to.apply(Action::Namespace(self.namespace()))?;
            self.load_requests(requests, &form.span(), false, to)?;
            self.compiler
                .borrow_mut()
                .finish_directive(staged)
                .map_err(|message| located(message, form.span()))?;
            return self.drain_lazy(to);
        }
        self.poll()?;
        let (unit, program, registration) = self.analyze(form)?;
        self.flush_warnings(form, to)?;
        if !program.lazy_targets.is_empty() && !self.loaded.borrow().contains(LAZY_NAMESPACE) {
            // A loadable needs no require of `light.lazy`, but its type must
            // exist when the form runs, so the session loads `light.lazy`
            // before the form runs, even while a namespace is loading: it
            // requires only the core.
            let request = lazy_request(LAZY_NAMESPACE, &self.namespace());
            self.load_requests(vec![request], &form.span(), false, to)?;
        }
        // A `light.lazy/loadable` of a namespace the session has not loaded
        // loads it, as a `require` would: before a top-level form runs, and
        // after the namespace that holds the form when one is loading.
        self.record_lazy(&program, &form.span());
        self.drain_lazy(to)?;
        // A runtime that runs the unit has its registrations, which come
        // before its form: from now on, a unit registers these namespaces
        // only when they change again. A unit that fails may have failed
        // before them, as one the runtime cannot parse does, so after it the
        // next unit registers every namespace again.
        self.registered.borrow_mut().extend(registration);
        let applied = to.apply(match mode {
            Mode::Repl => Action::Invoke(unit),
            Mode::Load => Action::Load(vec![unit]),
        });
        if applied.is_err() {
            self.forget_registrations();
        }
        applied
    }
    /// Analyze one form as an expression, with no directive or load, into
    /// its unit: `EvalSession::analyze` and the runtime's `eval`. Its
    /// warnings are dropped. The unit may run later, or never, so the
    /// namespaces it registers stay to be registered by the next unit too.
    pub(crate) fn analyze_unit(&self, form: &Form) -> Result<Unit, EvalDiagnostic> {
        let result = self.analyze(form);
        self.compiler.borrow_mut().warnings.clear();
        Ok(result?.0)
    }
    /// Make the next unit register every namespace again: after a unit that
    /// failed, perhaps before its registrations ran, or a unit of
    /// [`Self::analyze_unit`] that ran after units compiled later, which
    /// registered its namespaces as they were before those units.
    pub(crate) fn forget_registrations(&self) {
        self.registered.borrow_mut().clear();
    }
    /// Analyze one form that is not a directive into its unit, with the
    /// fragment it was emitted from and what the unit registers
    /// ([`Self::changed_registrations`]).
    fn analyze(
        &self,
        form: &Form,
    ) -> Result<(Unit, crate::Program, Registrations), EvalDiagnostic> {
        let program = self
            .compiler
            .borrow_mut()
            .analyze_form(form)
            .map_err(|error| EvalDiagnostic {
                data: None,
                message: error.to_string(),
                location: form
                    .span()
                    .source()
                    .span(error.span)
                    .or_else(|| Some(form.span())),
            })?;
        let namespace = self.namespace();
        let registration = if self.registering.get() {
            self.changed_registrations(&program)
        } else {
            Vec::new()
        };
        let registered = registration
            .iter()
            .map(|(namespace, _)| namespace.as_str())
            .collect::<Vec<_>>();
        let (mut js, mapping) = crate::emitter::emit_mapped_interactive_unit(&program, &registered);
        // The unit is named as the dev build's file of its namespace, which
        // a reload of the namespace replaces.
        let url = crate::dev_build::unit_file(&namespace);
        js.push_str("\n//# sourceURL=");
        js.push_str(&url);
        js.push('\n');
        let source = form.span().source().clone();
        let source_map = mapping.to_json(
            Some(&url),
            &[SourceFile {
                url: source.name(),
                text: source.text(),
            }],
            false,
        );
        let unit = Unit {
            namespace,
            js,
            source_map: Some(source_map),
            location: form.span(),
            mapping: Some(mapping),
        };
        Ok((unit, program, registration))
    }
    /// The namespaces of `program`, a form's fragment, that a unit of it
    /// registers, each with what it registers: those whose reader context or
    /// definitions a runtime that ran the units handed to interpreters so
    /// far does not have, which before the first unit is every one. A
    /// namespace's reader context changes when a form in it is analyzed
    /// after a directive, and its definitions grow with a definition or a
    /// host binding.
    fn changed_registrations(&self, program: &crate::Program) -> Registrations {
        let mut definitions = HashMap::<&str, usize>::new();
        for global in &program.globals {
            *definitions.entry(global.namespace.as_str()).or_default() += 1;
        }
        let registered = self.registered.borrow();
        program
            .namespaces
            .iter()
            .filter_map(|namespace| {
                let context = program
                    .reader_contexts
                    .get(namespace)
                    .map(|context| &**context);
                let count = definitions.get(namespace.as_str()).copied().unwrap_or(0);
                let known = registered
                    .get(namespace)
                    .is_some_and(|(known, known_count)| {
                        known.as_ref() == context && *known_count == count
                    });
                (!known).then(|| (namespace.clone(), (context.cloned(), count)))
            })
            .collect()
    }
    /// Apply the warnings the compiler gathered since the last call, located
    /// in the source of `form`, as one `Warnings` action.
    fn flush_warnings(&self, form: &Form, to: &mut dyn Interpreter) -> Result<(), EvalDiagnostic> {
        let warnings = std::mem::take(&mut self.compiler.borrow_mut().warnings);
        if warnings.is_empty() {
            return Ok(());
        }
        to.apply(Action::Warnings(
            warnings
                .into_iter()
                .map(|warning| EvalDiagnostic {
                    message: format!("{} [{}]", warning.message, warning.kind),
                    location: form.span().source().span(warning.span),
                    data: None,
                })
                .collect(),
        ))
    }
    /// Load each requested namespace the session has not loaded, compiling
    /// its forms as a `Load` after a `Namespace` of the namespace it enters,
    /// then return to the current namespace (a `Namespace` action, also when
    /// the load fails). A request that reloads loads its namespace again,
    /// and one that reloads all loads again, once, every namespace its load
    /// requires, directly or not. `lazy` requests come from
    /// `light.lazy/loadable` sites instead of a `require`; `location` is the
    /// form that made the requests.
    fn load_requests(
        &self,
        requests: Vec<LoadRequest>,
        location: &SourceSpan,
        lazy: bool,
        to: &mut dyn Interpreter,
    ) -> Result<(), EvalDiagnostic> {
        for request in requests {
            let name = String::from_utf16_lossy(request.namespace.name());
            // Whether a `:reload-all` that is loading has loaded it already.
            let reloaded = self
                .reload_all
                .borrow()
                .as_ref()
                .map(|loaded| loaded.contains(&name));
            if self.loaded.borrow().contains(&name)
                && request.reload == Reload::None
                && reloaded != Some(false)
            {
                continue;
            }
            if request.reload == Reload::All && reloaded.is_none() {
                *self.reload_all.borrow_mut() = Some(HashSet::new());
                let result = self.load_requests(vec![request], location, lazy, to);
                *self.reload_all.borrow_mut() = None;
                result?;
                continue;
            }
            if let Some(loaded) = self.reload_all.borrow_mut().as_mut() {
                loaded.insert(name.clone());
            }
            if self.loading.borrow().contains(&name) {
                return Err(located(
                    format!(
                        "namespace load cycle: {} -> {name}",
                        self.loading.borrow().join(" -> ")
                    ),
                    location.clone(),
                ));
            }
            if name == "cljs.core" || name == "clojure.core" {
                continue;
            }
            // The source provider first, then the bundled libraries, as
            // `--compile` looks in its source roots before them: a source
            // root may supply its own `clojure.string`.
            let provided = match &self.source_provider {
                Some(provider) => provider
                    .load(&request)
                    .map_err(|error| located(error.to_string(), location.clone()))?,
                None => None,
            };
            let source = match (provided, crate::compiler_libraries::source(&name)) {
                (Some(source), _) => source,
                (None, Some(source)) => SourceInput::new(format!("<builtin>/{name}.cljs"), source),
                (None, None) if self.source_provider.is_none() => {
                    return Err(located(
                        format!("no source provider for {name}"),
                        location.clone(),
                    ));
                }
                (None, None) => {
                    return Err(located(missing_namespace(&name, lazy), location.clone()));
                }
            };
            let namespace = self.namespace();
            self.loading.borrow_mut().push(name.clone());
            let result = self.run_source(&source, Mode::Load, to);
            self.loading.borrow_mut().pop();
            self.compiler.borrow_mut().set_namespace(&namespace);
            to.apply(Action::Namespace(namespace))?;
            result?;
            self.loaded.borrow_mut().insert(name);
        }
        Ok(())
    }
    /// Act on the recorded lazy references once no namespace is loading, as
    /// the compiler's loader visits a source's lazy targets after the source:
    /// load each recorded namespace not loaded yet, which may record more,
    /// then check each recorded Var target. A target may thus require the
    /// namespace that names it.
    fn drain_lazy(&self, to: &mut dyn Interpreter) -> Result<(), EvalDiagnostic> {
        if !self.loading.borrow().is_empty() {
            return Ok(());
        }
        loop {
            let next = self.lazy.borrow_mut().loads.pop_front();
            let Some((name, site)) = next else { break };
            let request = lazy_request(&name, &self.namespace());
            self.load_requests(vec![request], &site, true, to)?;
        }
        // Check the recorded Var targets, now that their namespaces are
        // loaded: the first one the session does not define is an error at
        // its site.
        let checks = std::mem::take(&mut self.lazy.borrow_mut().checks);
        for (namespace, name, site) in checks {
            if !self.compiler.borrow().defines(&namespace, &name) {
                *self.lazy.borrow_mut() = LazyQueue::default();
                return Err(located(
                    format!("lazy target {namespace}/{name} is not defined"),
                    site,
                ));
            }
        }
        Ok(())
    }
    /// Record the `light.lazy/loadable` references of `program`, a form at
    /// `site`: each namespace the session has neither loaded nor entered, to
    /// load, and each Var target, to check once its namespace is loaded.
    fn record_lazy(&self, program: &crate::Program, site: &SourceSpan) {
        let loaded = self.loaded.borrow();
        let mut lazy = self.lazy.borrow_mut();
        for target in &program.lazy_targets {
            let name = &program.lazy_namespaces[target.namespace];
            if !(loaded.contains(name)
                || program.namespaces.contains(name)
                || lazy.loads.iter().any(|(queued, _)| queued == name))
            {
                lazy.loads.push_back((name.clone(), site.clone()));
            }
            if let Some(global) = target.global {
                let global = &program.globals[global.0];
                lazy.checks
                    .push((global.namespace.clone(), global.name.clone(), site.clone()));
            }
        }
    }
}

/// The actions `run` applies to a collecting interpreter, ending with the
/// error it returns.
fn collect(run: impl FnOnce(&mut dyn Interpreter) -> Result<(), EvalDiagnostic>) -> Vec<Action> {
    let mut actions = Collect::default();
    if let Err(error) = run(&mut actions) {
        actions.0.push(Action::Error(error));
    }
    actions.0
}

fn located(message: String, location: SourceSpan) -> EvalDiagnostic {
    EvalDiagnostic {
        message,
        location: Some(location),
        data: None,
    }
}

/// The forms of a top-level `do` that holds an `ns*` form, at any depth of
/// `do`, which each run as a top-level form.
fn top_level_do(form: &Form) -> Option<Vec<Form>> {
    if !is_list(form) {
        return None;
    }
    let children = form.children();
    match children.first()?.data().kind() {
        DatumKind::Symbol(name) if name == &Name::new(None, "do") => {
            let children: Vec<_> = children.into_iter().skip(1).collect();
            children
                .iter()
                .any(has_nsstar_operation)
                .then_some(children)
        }
        _ => None,
    }
}
fn has_nsstar_operation(form: &Form) -> bool {
    if !is_list(form) {
        return false;
    }
    let children = form.children();
    match children.first().map(|head| head.data().kind().clone()) {
        Some(DatumKind::Symbol(name))
            if name == Name::new(None, "ns*")
                || name == Name::new(Some("cljs.core"), "ns*")
                || name == Name::new(Some("clojure.core"), "ns*") =>
        {
            true
        }
        Some(DatumKind::Symbol(name)) if name == Name::new(None, "do") => {
            children.iter().skip(1).any(has_nsstar_operation)
        }
        _ => false,
    }
}
fn is_list(form: &Form) -> bool {
    matches!(
        form.data().kind(),
        DatumKind::Collection {
            kind: crate::CollectionKind::List,
            ..
        }
    )
}

/// A request for the target namespace of a lazy reference.
fn lazy_request(name: &str, requesting: &str) -> LoadRequest {
    LoadRequest {
        namespace: Name::new(None, name),
        requesting_namespace: Name::new(None, requesting),
        requesting_file: None,
        reload: Reload::None,
    }
}

fn missing_namespace(name: &str, lazy: bool) -> String {
    if lazy {
        format!("lazy target namespace {name} not found")
    } else {
        format!("missing namespace {name}")
    }
}

/// `actions` as a script for a running dev build ([`crate::dev_build`]),
/// which applies them in order to its runtime, `globalThis.__lightRuntime`,
/// as the session's heap does: a `Namespace` makes the namespace the
/// runtime's current one (`*ns*`); each run of consecutive units of one
/// namespace in a `Load` is one call of the client's
/// `globalThis.__lightDev.define`, whose unit runs them in order and returns
/// the last one's value (the session does not track a namespace's requires,
/// so each call lists none); an `Invoke` runs its unit; an `Error` throws
/// its message, since nothing after it was compiled. `Warnings` are left out.
pub fn script(actions: &[Action]) -> String {
    let mut script = String::new();
    for action in actions {
        match action {
            Action::Namespace(name) => {
                script.push_str("(rt => { rt.core.currentNs = rt.core.createNs(rt.symbol(null, ");
                json_string(&mut script, name);
                script.push_str(")); })(globalThis.__lightRuntime);\n");
            }
            Action::Load(units) => {
                for run in units.chunk_by(|a, b| a.namespace == b.namespace) {
                    script.push_str("globalThis.__lightDev.define(");
                    json_string(&mut script, &run[0].namespace);
                    script.push_str(", [], (__lightInteractiveRuntime) => {\n");
                    for (index, unit) in run.iter().enumerate() {
                        script.push_str(if index + 1 == run.len() {
                            "  return ("
                        } else {
                            "  ("
                        });
                        script.push_str(&unit.js);
                        script.push_str(")(__lightInteractiveRuntime);\n");
                    }
                    script.push_str("});\n");
                }
            }
            Action::Invoke(unit) => {
                script.push('(');
                script.push_str(&unit.js);
                script.push_str(")(globalThis.__lightRuntime);\n");
            }
            Action::Warnings(_) => {}
            Action::Error(error) => {
                script.push_str("throw new Error(");
                json_string(&mut script, &error.to_string());
                script.push_str(");\n");
            }
        }
    }
    script
}
