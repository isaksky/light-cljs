//! Compiler-backed interactive evaluation and explicit embedding capabilities.
use crate::analyzer::LAZY_NAMESPACE;
use crate::compiler_data::{Datum, DatumKind, Form, Origin, Source, SourceSpan};
use crate::eval_compiler::InteractiveCompiler;
pub use crate::eval_compiler::NamePolicy;
use crate::evaluator::{EvalError, Evaluator, EvaluatorOptions, ExecutionControl, ValueId};
pub use crate::evaluator::{HostArity, HostCall};
use crate::form_bridge::ConversionLimits;
use crate::source_provider::{LoadRequest, Reload, SourceInput, SourceProvider};
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    fmt,
    rc::Rc,
};

pub type HostCallback =
    Rc<dyn for<'a, 'js> Fn(&HostCall<'a, 'js>, &[ValueId]) -> Result<ValueId, EvalError>>;
#[derive(Clone)]
pub struct HostFunction {
    pub name: String,
    pub arity: HostArity,
    pub callback: HostCallback,
}
pub type Output = Rc<dyn Fn(&str) -> Result<(), EvalError>>;
#[derive(Clone, Debug)]
pub struct PrintOptions {
    pub readably: bool,
    pub newline: bool,
    pub length: Option<usize>,
    pub level: Option<usize>,
    pub namespace_maps: bool,
    pub meta: bool,
    pub dup: bool,
    pub flush_on_newline: bool,
}
impl Default for PrintOptions {
    fn default() -> Self {
        Self {
            readably: true,
            newline: true,
            length: None,
            level: None,
            namespace_maps: false,
            meta: false,
            dup: false,
            flush_on_newline: true,
        }
    }
}

pub type Input = Rc<dyn Fn(ExecutionControl) -> Result<Option<String>, EvalError>>;

/// Explicit clock/random capabilities. Neither callback receives engine values.
pub type NumberSource = Rc<dyn Fn() -> Result<f64, EvalError>>;
/// File loading is distinct from namespace resolution; paths are opaque to the session.
#[derive(Clone, Debug)]
pub struct FileLoadRequest {
    pub path: String,
    pub requesting_namespace: String,
}
pub type FileProvider = Rc<dyn Fn(&FileLoadRequest) -> Result<Option<SourceInput>, EvalError>>;

/// Immutable configuration reused by `fresh_session`; mutable values are never
/// cloned into another heap. Limits use JS bytes/stack/time/polls, not native fuel.
#[derive(Clone)]
pub struct EvalOptions {
    pub namespace: String,
    pub aliases: Vec<(String, String)>,
    pub engine: EvaluatorOptions,
    pub compiler: crate::MacroLimits,
    pub max_source_bytes: usize,
    pub max_output_bytes: usize,
    pub functions: Vec<HostFunction>,
    /// Tag handlers are compiler-emitted functions over finite readable data.
    pub data_readers: Vec<(crate::compiler_data::Name, String)>,
    pub default_data_reader: Option<String>,
    pub print: PrintOptions,
    pub bindings: Vec<(String, Datum)>,
    pub source_provider: Option<Rc<dyn SourceProvider>>,
    pub file_provider: Option<FileProvider>,
    pub clock: Option<NumberSource>,
    pub random: Option<NumberSource>,
    pub output: Option<Output>,
    pub error_output: Option<Output>,
    pub input: Option<Input>,
    pub arguments: Vec<String>,
    pub assertions: bool,
    pub policy: NamePolicy,
    pub unhandled_rejections: UnhandledRejections,
}
/// What the session does with a promise rejected with no handler attached
/// when the jobs after a top-level form run out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum UnhandledRejections {
    /// Keep a report of it for [`EvalSession::take_unhandled_rejections`],
    /// as a browser logs "Uncaught (in promise)" and goes on (`--repl`).
    #[default]
    Report,
    /// Fail the form with it, as Node fails a script (`--eval`).
    Fail,
}
impl Default for EvalOptions {
    fn default() -> Self {
        Self {
            namespace: "cljs.user".into(),
            aliases: vec![],
            engine: EvaluatorOptions::default(),
            compiler: crate::MacroLimits::default(),
            max_source_bytes: 4 * 1024 * 1024,
            max_output_bytes: 1024 * 1024,
            functions: vec![],
            data_readers: vec![],
            default_data_reader: None,
            print: PrintOptions::default(),
            bindings: vec![],
            source_provider: None,
            file_provider: None,
            clock: None,
            random: None,
            output: None,
            error_output: None,
            input: None,
            arguments: vec![],
            assertions: true,
            policy: NamePolicy::default(),
            unhandled_rejections: UnhandledRejections::default(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct EvalDiagnostic {
    pub message: String,
    pub location: Option<SourceSpan>,
    pub data: Option<Datum>,
}
impl fmt::Display for EvalDiagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if let Some(span) = &self.location {
            let (line, column) = span
                .source()
                .line_column(span.bytes().start)
                .unwrap_or((1, 1));
            let prefix = format!("{}:{line}:{column}: ", span.source().name());
            if !self.message.starts_with(&prefix) {
                f.write_str(&prefix)?;
            }
        }
        f.write_str(&self.message)
    }
}
impl std::error::Error for EvalDiagnostic {}
impl From<EvalError> for EvalDiagnostic {
    fn from(e: EvalError) -> Self {
        Self {
            message: e.0,
            data: None,
            location: None,
        }
    }
}
impl From<crate::compiler_data::ReadError> for EvalDiagnostic {
    fn from(error: crate::compiler_data::ReadError) -> Self {
        // Preserve the public reader message while adding an owned location.
        Self {
            message: error.to_string(),
            location: Some(error.span),
            data: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct EvalResult {
    pub value: ValueId,
    pub namespace: String,
}
/// Analysis retains no engine values; evaluation requires its originating session.
pub struct AnalyzedForm {
    owner: u64,
    source: Form,
    program: crate::Program,
}
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

struct OperationGuard(Rc<Cell<usize>>);
impl Drop for OperationGuard {
    fn drop(&mut self) {
        self.0.set(self.0.get() - 1);
    }
}
struct FileLoadGuard(Rc<RefCell<Vec<String>>>);
impl FileLoadGuard {
    fn begin(stack: &Rc<RefCell<Vec<String>>>, path: &str) -> Result<Self, EvalError> {
        if stack.borrow().iter().any(|entry| entry == path) {
            return Err(EvalError(format!(
                "file load cycle: {} -> {path}",
                stack.borrow().join(" -> ")
            )));
        }
        stack.borrow_mut().push(path.into());
        Ok(Self(stack.clone()))
    }
}
impl Drop for FileLoadGuard {
    fn drop(&mut self) {
        self.0.borrow_mut().pop();
    }
}

pub struct EvalSession {
    engine: Evaluator,
    namespace_update: ValueId,
    compiler: Rc<RefCell<InteractiveCompiler>>,
    options: EvalOptions,
    output_bytes: Rc<RefCell<usize>>,
    loaded: Rc<RefCell<HashSet<String>>>,
    loading: Rc<RefCell<Vec<String>>>,
    file_loading: Rc<RefCell<Vec<String>>>,
    identity: u64,
    operation_depth: Rc<Cell<usize>>,
    /// Reports of unhandled rejections, under `UnhandledRejections::Report`.
    rejections: RefCell<Vec<String>>,
}
impl EvalSession {
    pub fn new(options: EvalOptions) -> Result<Self, EvalDiagnostic> {
        let engine = Evaluator::interactive(options.engine.clone())?;
        engine.track_rejections();
        let namespace_update = engine
            .evaluate_js("name => { rt.core.currentNs = rt.core.createNs(name); return null; }")?;
        let mut host = crate::MacroHost::new();
        host.limits = options.compiler;
        host.evaluator_options = options.engine.clone();
        host.interactive_control = Some(engine.control());
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
        compiler.set_namespace(&options.namespace);
        for (alias, target) in &options.aliases {
            compiler.set_alias(alias, target);
        }
        let mut session = Self {
            engine,
            namespace_update,
            compiler: Rc::new(RefCell::new(compiler)),
            options,
            output_bytes: Rc::new(RefCell::new(0)),
            loaded: Rc::new(RefCell::new(HashSet::new())),
            loading: Rc::new(RefCell::new(vec![])),
            file_loading: Rc::new(RefCell::new(vec![])),
            operation_depth: Rc::new(Cell::new(0)),
            identity: NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            rejections: RefCell::new(vec![]),
        };
        for function in session.options.functions.clone() {
            let value = session.engine.install_host_function(
                &function.name,
                function.arity,
                function.callback,
            )?;
            session.bind(&function.name, value)?;
            session.engine.release(value)?;
        }
        for (name, data) in session.options.bindings.clone() {
            let value = session.value(&data)?;
            session.bind(&name, value)?;
            session.release(value)?;
        }
        session.install_services()?;
        Ok(session)
    }
    pub fn fresh_session(&self) -> Result<Self, EvalDiagnostic> {
        Self::new(self.options.clone())
    }
    /// Replace the terminal or live session with independent compiler/runtime roots.
    /// All old value and analysis IDs become invalid.
    pub fn reset(&mut self) -> Result<(), EvalDiagnostic> {
        *self = self.fresh_session()?;
        Ok(())
    }
    pub fn control(&self) -> ExecutionControl {
        self.engine.control()
    }
    pub fn namespace(&self) -> String {
        self.compiler.borrow().namespace().into()
    }
    fn begin(&self) -> Result<OperationGuard, EvalDiagnostic> {
        if self.operation_depth.get() == 0 {
            self.engine.begin_operation(&self.options.engine)?;
            *self.output_bytes.borrow_mut() = 0;
        }
        self.operation_depth.set(self.operation_depth.get() + 1);
        Ok(OperationGuard(self.operation_depth.clone()))
    }
    /// Begin an operation that runs promise jobs; the outermost one starts a
    /// new list of unhandled rejection reports.
    fn begin_jobs(&self) -> Result<OperationGuard, EvalDiagnostic> {
        if self.operation_depth.get() == 0 {
            self.rejections.borrow_mut().clear();
        }
        self.begin()
    }
    pub fn eval_string(&mut self, text: &str) -> Result<EvalResult, EvalDiagnostic> {
        self.eval_source(&SourceInput::new("<eval>.cljs", text))
    }
    /// Evaluate each top-level form of `source` in turn, running the promise
    /// jobs it queues after it (see [`Self::run_jobs`]).
    pub fn eval_source(&mut self, source: &SourceInput) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin_jobs()?;
        self.source(source, true)
    }
    /// Load only through the configured file capability, preserving session roots.
    /// The file runs as one script: promise jobs run after its last form.
    pub fn load_file(&mut self, path: &str) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin_jobs()?;
        let result = {
            let _file = FileLoadGuard::begin(&self.file_loading, path)?;
            file_source(&self.options, path, &self.namespace())
                .map_err(EvalDiagnostic::from)
                .and_then(|source| self.source(&source, false))
        };
        self.after_jobs(result)
    }
    /// Run the pending promise jobs: `.then` and `queueMicrotask` callbacks
    /// and the rest of an `^:async` function body after an `await`, with the
    /// jobs they queue in turn, until none is left. `eval_string`,
    /// `eval_source`, `eval_form` and `load_file` run them after each
    /// top-level form, as an event loop runs them after a script, and never
    /// between the forms of a namespace the session loads; an embedder that
    /// drives `evaluate` or `invoke` itself calls this. Jobs run under the
    /// operation's deadline, poll budget and memory limit. The first exception
    /// that leaves a job is the error, and `last_exception` holds it. Then
    /// each promise still rejected with no handler attached is reported (see
    /// [`UnhandledRejections`]).
    pub fn run_jobs(&self) -> Result<(), EvalDiagnostic> {
        let _operation = self.begin_jobs()?;
        self.jobs(false)
    }
    /// Take the reports of the promises that the latest `eval_string`,
    /// `eval_source`, `eval_form`, `load_file` or `run_jobs` left rejected
    /// with no handler attached once its jobs ran out, oldest first, under
    /// [`UnhandledRejections::Report`]: `unhandled promise rejection: ` and
    /// the printed reason. Each promise is reported once, and a handler
    /// attached later does not withdraw its report.
    pub fn take_unhandled_rejections(&self) -> Vec<String> {
        std::mem::take(&mut *self.rejections.borrow_mut())
    }
    /// Run the jobs, then take the unhandled rejections: under `Fail` the
    /// first is the error unless the form (`keep`) or a job failed first,
    /// and under `Report` each is kept for `take_unhandled_rejections`.
    fn jobs(&self, keep: bool) -> Result<(), EvalDiagnostic> {
        let jobs = self.engine.run_jobs(keep);
        let report = self.options.unhandled_rejections == UnhandledRejections::Report;
        // `Fail` prints only the first, and none once the form or a job failed.
        let raise = !report && !keep && jobs.is_ok();
        let limit = if report { u32::MAX } else { u32::from(raise) };
        let rejected = self
            .engine
            .unhandled_rejections(limit, raise)
            .map(|reasons| {
                reasons
                    .into_iter()
                    .map(|reason| format!("unhandled promise rejection: {reason}"))
            });
        let result = match (jobs, rejected) {
            (jobs, Ok(messages)) if report => {
                self.rejections.borrow_mut().extend(messages);
                jobs
            }
            (Ok(()), Ok(mut messages)) => messages
                .next()
                .map_or(Ok(()), |message| Err(EvalError(message))),
            (jobs, Ok(_)) => jobs,
            (jobs, Err(error)) => jobs.and(Err(error)),
        };
        result.map_err(|error| EvalDiagnostic {
            data: self.engine.error_data(),
            message: error.0,
            location: None,
        })
    }
    /// Run the promise jobs after a top-level form, whether or not the form
    /// succeeded. A failed form's error comes first, and its exception stays
    /// the last one while its jobs run.
    fn after_jobs(
        &self,
        result: Result<EvalResult, EvalDiagnostic>,
    ) -> Result<EvalResult, EvalDiagnostic> {
        match result {
            Ok(result) => match self.jobs(false) {
                Ok(()) => Ok(result),
                Err(error) => {
                    let _ = self.engine.release(result.value);
                    Err(error)
                }
            },
            Err(error) => {
                let _ = self.jobs(true);
                Err(error)
            }
        }
    }
    /// Evaluate each top-level form of `source`; with `jobs`, run the promise
    /// jobs after each one, as at the outermost boundary of a REPL or script.
    fn source(&mut self, source: &SourceInput, jobs: bool) -> Result<EvalResult, EvalDiagnostic> {
        if source.file.ends_with(".clj") {
            return Err(
                EvalError("native .clj evaluation is retired; use .cljs or .cljc".into()).into(),
            );
        }
        if source.source.len() > self.options.max_source_bytes {
            return Err(EvalDiagnostic {
                data: None,
                message: "compiler source byte limit exceeded".into(),
                location: None,
            });
        }
        let mut cursor = 0;
        let mut last = self.value(&Datum::new(DatumKind::Nil))?;
        loop {
            self.engine.control().poll()?;
            let next = self
                .compiler
                .borrow()
                .next_form(source, &mut cursor)
                .map_err(EvalDiagnostic::from);
            let form = match next {
                Ok(Some(form)) => form,
                Ok(None) => break,
                Err(error) => {
                    let _ = self.engine.release(last);
                    return Err(error);
                }
            };
            let mut result = self.form(&form);
            if jobs {
                result = self.after_jobs(result);
            }
            let _ = self.engine.release(last);
            last = result?.value;
        }
        Ok(EvalResult {
            value: last,
            namespace: self.namespace(),
        })
    }
    /// Evaluate one top-level form, then run the promise jobs it queued.
    pub fn eval_form(&mut self, form: &Form) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin_jobs()?;
        let result = self.form(form);
        self.after_jobs(result)
    }
    fn form(&mut self, form: &Form) -> Result<EvalResult, EvalDiagnostic> {
        if let Some(children) = top_level_do(form) {
            let mut last = self.value(&Datum::new(DatumKind::Nil))?;
            for child in children {
                let result = self.form(&child);
                let _ = self.engine.release(last);
                last = result?.value;
            }
            return Ok(EvalResult {
                value: last,
                namespace: self.namespace(),
            });
        }
        if self.loading.borrow().is_empty() {
            // What an earlier top-level form recorded and failed to act on.
            self.compiler.borrow_mut().lazy = Default::default();
        }
        self.engine.input_action("clearError", 0)?;
        let requests = self.compiler.borrow_mut().directive(form)?;
        if requests.is_some() {
            self.sync_namespace()?;
        }
        if let Some((requests, staged)) = requests {
            self.load_requests(requests, &form.span(), false)?;
            self.compiler
                .borrow_mut()
                .finish_directive(staged)
                .map_err(|message| EvalDiagnostic {
                    data: None,
                    message,
                    location: Some(form.span()),
                })?;
            self.drain_lazy()?;
            return Ok(EvalResult {
                value: self.value(&Datum::new(DatumKind::Nil))?,
                namespace: self.namespace(),
            });
        }
        let analyzed = self.analyze(form)?;
        if needs_lazy_library(&analyzed.program, &self.loaded.borrow()) {
            let request = lazy_request(LAZY_NAMESPACE, &self.namespace());
            self.load_requests(vec![request], &form.span(), false)?;
        }
        // A `light.lazy/loadable` of a namespace the session has not loaded
        // loads it, as a `require` would: before a top-level form runs, and
        // after the namespace that holds the form when one is loading.
        record_lazy(
            &mut self.compiler.borrow_mut(),
            &analyzed.program,
            &self.loaded.borrow(),
            &form.span(),
        );
        self.drain_lazy()?;
        self.evaluate(&analyzed)
    }
    /// Act on the recorded lazy references once no namespace is loading, as
    /// the compiler's loader visits a source's lazy targets after the source:
    /// load each recorded namespace not loaded yet, which may record more,
    /// then check each recorded Var target. A target may thus require the
    /// namespace that names it.
    fn drain_lazy(&mut self) -> Result<(), EvalDiagnostic> {
        if !self.loading.borrow().is_empty() {
            return Ok(());
        }
        loop {
            let next = self.compiler.borrow_mut().lazy.loads.pop_front();
            let Some((name, site)) = next else { break };
            let request = lazy_request(&name, &self.namespace());
            self.load_requests(vec![request], &site, true)?;
        }
        check_lazy(&mut self.compiler.borrow_mut())
    }
    /// Load and evaluate each requested namespace the session has not
    /// loaded, then return to the current namespace. `lazy` requests come
    /// from `light.lazy/loadable` sites instead of a `require`; `location`
    /// is the form that made the requests.
    fn load_requests(
        &mut self,
        requests: Vec<LoadRequest>,
        location: &SourceSpan,
        lazy: bool,
    ) -> Result<(), EvalDiagnostic> {
        let located = |message: String| EvalDiagnostic {
            data: None,
            message,
            location: Some(location.clone()),
        };
        for request in requests {
            let name = String::from_utf16_lossy(request.namespace.name());
            if self.loaded.borrow().contains(&name)
                && request.reload == crate::source_provider::Reload::None
            {
                continue;
            }
            if self.loading.borrow().contains(&name) {
                return Err(located(format!(
                    "namespace load cycle: {} -> {name}",
                    self.loading.borrow().join(" -> ")
                )));
            }
            if name == "cljs.core" || name == "clojure.core" {
                continue;
            }
            let source = if let Some(source) = crate::compiler_libraries::source(&name) {
                SourceInput::new(format!("<builtin>/{name}.cljs"), source)
            } else {
                self.options
                    .source_provider
                    .as_ref()
                    .ok_or_else(|| located(format!("no source provider for {name}")))?
                    .load(&request)
                    .map_err(|e| located(e.to_string()))?
                    .ok_or_else(|| located(missing_namespace(&name, lazy)))?
            };
            let namespace = self.namespace();
            self.loading.borrow_mut().push(name.clone());
            let result = self.source(&source, false);
            self.loading.borrow_mut().pop();
            self.compiler.borrow_mut().set_namespace(&namespace);
            self.sync_namespace()?;
            self.engine.release(result?.value)?;
            self.loaded.borrow_mut().insert(name);
        }
        Ok(())
    }
    pub fn analyze(&mut self, form: &Form) -> Result<AnalyzedForm, EvalDiagnostic> {
        let _operation = self.begin()?;
        self.engine.control().poll()?;
        let program =
            self.compiler
                .borrow_mut()
                .analyze_form(form)
                .map_err(|e| EvalDiagnostic {
                    data: None,
                    message: e.to_string(),
                    location: form
                        .span()
                        .source()
                        .span(e.span)
                        .or_else(|| Some(form.span())),
                })?;
        Ok(AnalyzedForm {
            owner: self.identity,
            source: form.clone(),
            program,
        })
    }
    pub fn evaluate(&self, analyzed: &AnalyzedForm) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin()?;
        if analyzed.owner != self.identity {
            return Err(EvalError("analysis belongs to another session".into()).into());
        }
        let source = crate::emitter::emit_interactive_unit(&analyzed.program);
        let value = self
            .engine
            .evaluate_unit(&source)
            .map_err(|e| EvalDiagnostic {
                data: self.engine.error_data(),
                message: e.0,
                location: Some(analyzed.source.span()),
            })?;
        Ok(EvalResult {
            value,
            namespace: self.namespace(),
        })
    }
    pub fn invoke(&self, function: ValueId, args: &[ValueId]) -> Result<ValueId, EvalDiagnostic> {
        let _operation = self.begin()?;
        self.engine
            .call(function, args)
            .map_err(|error| EvalDiagnostic {
                message: error.0,
                location: None,
                data: self.engine.error_data(),
            })
    }
    pub fn macroexpand(&self, form: &Form, once: bool) -> Result<Form, EvalDiagnostic> {
        let _operation = self.begin()?;
        self.compiler
            .borrow_mut()
            .macroexpand(form, once)
            .map_err(|message| EvalDiagnostic {
                data: None,
                message,
                location: Some(form.span()),
            })
    }
    /// Read one compiler form from the canonical host stream. Language reads use
    /// the same cursor, including the buffered suffix of the current line.
    pub fn read_input_form(&self) -> Result<Option<Form>, EvalDiagnostic> {
        let _operation = self.begin()?;
        let mut more = false;
        loop {
            let (text, units, eof, line) = self.engine.input_window(more)?;
            let offset = text
                .char_indices()
                .scan(0, |n, (i, ch)| {
                    let start = *n;
                    *n += ch.len_utf16();
                    Some((i, start))
                })
                .find(|(_, n)| *n == units)
                .map_or(text.len(), |(i, _)| i);
            if text.len() > self.options.max_source_bytes {
                return Err(EvalError("compiler source byte limit exceeded".into()).into());
            }
            let input = SourceInput::new("<stdin>.cljc", &text);
            let source = Source::with_line_offset(&input.file, &text, line - 1);
            let mut parser = crate::Parser::at_offset(&text, offset).retaining_discards();
            match parser.read_form() {
                Err(error)
                    if !eof
                        && matches!(
                            error.kind,
                            crate::ErrorKind::UnexpectedEof(_)
                                | crate::ErrorKind::Lexical(
                                    crate::LexError::UnterminatedString
                                        | crate::LexError::UnterminatedRegex
                                )
                        ) =>
                {
                    more = true;
                    continue;
                }
                Err(error) => {
                    self.engine.input_action("inputDiscard", 0)?;
                    let mut message = error.to_string();
                    if let Some(open) = &error.related {
                        let (line, column) = source.line_column(open.start).unwrap();
                        message.push_str(&format!("; opened at {}:{line}:{column}", source.name()));
                    }
                    return Err(EvalDiagnostic {
                        message,
                        location: source.span(error.span),
                        data: None,
                    });
                }
                Ok(None) if !eof => {
                    self.engine.input_action(
                        "inputConsume",
                        text[offset..parser.position()].encode_utf16().count(),
                    )?;
                    self.engine.input_action("inputCompact", 0)?;
                    more = false;
                    continue;
                }
                _ => {}
            }
            let mut cursor = offset;
            let result =
                self.compiler
                    .borrow()
                    .next_form_in_source(&input, &mut cursor, source.clone());
            match result {
                Ok(form) => {
                    self.engine.input_action(
                        "inputConsume",
                        text[offset..cursor].encode_utf16().count(),
                    )?;
                    if form.is_none() && !eof {
                        more = true;
                        continue;
                    }
                    return Ok(form);
                }
                Err(error) => {
                    self.engine.input_action("inputDiscard", 0)?;
                    return Err(error.into());
                }
            }
        }
    }
    pub fn recover_input(&self) -> Result<(), EvalDiagnostic> {
        let _operation = self.begin()?;
        self.engine.input_action("inputRecover", 0)?;
        self.engine.input_action("inputCompact", 0)?;
        Ok(())
    }
    /// Retain the last thrown application value for interactive error history.
    pub fn last_exception(&self) -> Result<ValueId, EvalDiagnostic> {
        let _operation = self.begin()?;
        Ok(self.engine.last_exception()?)
    }
    pub fn read_forms(&self, source: &SourceInput) -> Result<Vec<Form>, EvalDiagnostic> {
        let _operation = self.begin()?;
        self.compiler
            .borrow()
            .read_forms(source)
            .map_err(EvalDiagnostic::from)
    }
    pub fn value(&self, data: &Datum) -> Result<ValueId, EvalDiagnostic> {
        let _operation = self.begin()?;
        Ok(self.engine.import_data(data, ConversionLimits::default())?)
    }
    pub fn data(&self, value: ValueId) -> Result<Datum, EvalDiagnostic> {
        let _operation = self.begin()?;
        Ok(self
            .engine
            .export_data(value, ConversionLimits::default())?)
    }
    pub fn number(&self, value: ValueId) -> Result<f64, EvalDiagnostic> {
        let _operation = self.begin()?;
        Ok(self.engine.number(value)?)
    }
    pub fn string(&self, value: ValueId) -> Result<String, EvalDiagnostic> {
        let _operation = self.begin()?;
        Ok(self.engine.string(value)?)
    }
    pub fn print(&self, value: ValueId) -> Result<String, EvalDiagnostic> {
        let _operation = self.begin()?;
        let text = self.engine.print(value)?;
        if text.len() > self.options.max_output_bytes {
            return Err(EvalError("output byte limit exceeded".into()).into());
        }
        Ok(text)
    }
    pub fn release(&self, value: ValueId) -> Result<(), EvalDiagnostic> {
        let _operation = self.begin()?;
        Ok(self.engine.release(value)?)
    }
    pub fn bind(&mut self, name: &str, value: ValueId) -> Result<(), EvalDiagnostic> {
        let _operation = self.begin()?;
        self.engine.validate(value)?;
        let (target, core) = self.binding_target(name);
        if core {
            return Ok(self.engine.bind(&target, value, true)?);
        }
        let namespace = self.namespace();
        let (ns, local) = name.split_once('/').unwrap_or((&namespace, name));
        self.compiler
            .borrow_mut()
            .declare_binding(ns, local)
            .map_err(|e| EvalError(e.to_string()))?;
        Ok(self.engine.bind(&format!("{ns}/{local}"), value, false)?)
    }
    pub fn bindings(&self) -> Vec<String> {
        self.compiler
            .borrow()
            .globals()
            .iter()
            .map(|g| format!("{}/{}", g.namespace, g.name))
            .collect()
    }
    /// Temporarily redefine application roots. Every installed root is restored
    /// on errors and panics. Handles must belong to this session.
    pub fn with_bindings<T>(
        &mut self,
        bindings: &[(String, ValueId)],
        body: impl FnOnce(&mut Self) -> Result<T, EvalDiagnostic>,
    ) -> Result<T, EvalDiagnostic> {
        let _operation = self.begin()?;
        // Validate the full scope before allocating saved roots or mutating.
        for (_, value) in bindings {
            self.engine.validate(*value)?;
        }
        let mut saved = Vec::new();
        for (name, value) in bindings {
            let (name, core) = self.binding_target(name);
            let old = self.engine.lookup(&name, core)?;
            saved.push((name, old, *value, core));
        }
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            for (name, _, value, core) in &saved {
                self.engine.bind(name, *value, *core)?;
            }
            body(self)
        }));
        for (name, value, _, core) in saved.into_iter().rev() {
            let _ = self.engine.bind(&name, value, core);
            let _ = self.engine.release(value);
        }
        match result {
            Ok(result) => result,
            Err(panic) => std::panic::resume_unwind(panic),
        }
    }
    pub fn lookup(&self, name: &str) -> Result<ValueId, EvalDiagnostic> {
        let _operation = self.begin()?;
        let (name, core) = self.binding_target(name);
        Ok(self.engine.lookup(&name, core)?)
    }
    fn binding_target(&self, name: &str) -> (String, bool) {
        if let Some(("cljs.core" | "clojure.core", local)) = name.split_once('/')
            && let Some(core) = crate::CoreFnId::resolve(local)
        {
            return (core.runtime_name().into(), true);
        }
        (
            if name.contains('/') {
                name.into()
            } else {
                format!("{}/{name}", self.namespace())
            },
            false,
        )
    }
    fn install_services(&mut self) -> Result<(), EvalDiagnostic> {
        self.sync_namespace()?;
        let assertions = self.value(&Datum::new(DatumKind::Boolean(self.options.assertions)))?;
        self.engine.bind("assertions", assertions, true)?;
        self.release(assertions)?;
        for (name, source) in [
            ("hostClock", self.options.clock.clone()),
            ("hostRandom", self.options.random.clone()),
        ] {
            if let Some(source) = source {
                let callback: HostCallback =
                    Rc::new(move |call, _| call.value(&Datum::new(DatumKind::Double(source()?))));
                let value =
                    self.engine
                        .install_host_function(name, HostArity::Exact(0), callback)?;
                self.engine.bind(name, value, true)?;
                self.engine.release(value)?;
            }
        }
        let compiler = Rc::downgrade(&self.compiler);
        let namespace_update = self.namespace_update;
        let options = self.options.clone();
        let loaded = self.loaded.clone();
        let loading = self.loading.clone();
        let file_loading = self.file_loading.clone();
        let callback: HostCallback = Rc::new(move |call, args| {
            let compiler = compiler
                .upgrade()
                .ok_or_else(|| EvalError("session dropped".into()))?;
            let path = call.string(args[0])?;
            let _file = FileLoadGuard::begin(&file_loading, &path)?;
            let namespace = compiler.borrow().namespace().to_owned();
            let source = file_source(&options, &path, &namespace)?;
            callback_source(
                call,
                &compiler,
                namespace_update,
                &options,
                &loaded,
                &loading,
                &source,
            )
            .map_err(|error| EvalError(error.to_string()))
        });
        let value = self
            .engine
            .install_host_function("loadFile", HostArity::Exact(1), callback)?;
        self.engine.bind("loadFile", value, true)?;
        self.engine.release(value)?;
        let print = self.options.print.clone();
        for (name, kind) in [
            ("printReadably", DatumKind::Boolean(print.readably)),
            ("printNewline", DatumKind::Boolean(print.newline)),
            ("printMeta", DatumKind::Boolean(print.meta)),
            ("printDup", DatumKind::Boolean(print.dup)),
            ("flushOnNewline", DatumKind::Boolean(print.flush_on_newline)),
            (
                "printNamespaceMaps",
                DatumKind::Boolean(print.namespace_maps),
            ),
            (
                "printLength",
                print
                    .length
                    .map_or(DatumKind::Nil, |n| DatumKind::Double(n as f64)),
            ),
            (
                "printLevel",
                print
                    .level
                    .map_or(DatumKind::Nil, |n| DatumKind::Double(n as f64)),
            ),
        ] {
            let value = self.value(&Datum::new(kind))?;
            self.engine.bind(name, value, true)?;
            self.release(value)?;
        }

        for (name, output) in [
            ("printFn", self.options.output.clone()),
            ("printErrFn", self.options.error_output.clone()),
        ] {
            if let Some(output) = output {
                let used = self.output_bytes.clone();
                let limit = self.options.max_output_bytes;
                let callback: HostCallback = Rc::new(move |call, args| {
                    let text = call.string(args[0])?;
                    {
                        let mut n = used.borrow_mut();
                        *n = n.saturating_add(text.len());
                        if *n > limit {
                            return Err(EvalError("output byte limit exceeded".into()));
                        }
                    }
                    output(&text)?;
                    call.value(&Datum::new(DatumKind::Nil))
                });
                let value =
                    self.engine
                        .install_host_function(name, HostArity::Exact(1), callback)?;
                self.engine.bind(name, value, true)?;
                self.engine.release(value)?;
            }
        }
        if let Some(input) = self.options.input.clone() {
            let value = self
                .engine
                .input_provider("<host-input>", move |control| input(control))?;
            self.engine.bind("readerCurrentInputValue", value, true)?;
            self.engine.release(value)?;
        }
        let args = Datum::new(DatumKind::Collection {
            kind: crate::CollectionKind::List,
            children: self
                .options
                .arguments
                .iter()
                .map(|s| Datum::new(DatumKind::String(s.encode_utf16().collect())))
                .collect(),
        });
        let value = self.value(&args)?;
        self.engine.bind("commandLineArgs", value, true)?;
        self.release(value)?;
        for (name, once) in [
            ("macroexpand", false),
            ("macroexpand1", true),
            ("eval", false),
        ] {
            let compiler = Rc::downgrade(&self.compiler);
            let evaluate = name == "eval";
            let callback: HostCallback = Rc::new(move |call, args| {
                let compiler = compiler
                    .upgrade()
                    .ok_or_else(|| EvalError("session dropped".into()))?;
                let data = call.data(args[0])?;
                let source = Source::new("<runtime-eval>", "");
                let form = Form::from_data(data, source.span(0..0).unwrap(), Origin::Source);
                if evaluate {
                    let program = compiler
                        .borrow_mut()
                        .analyze_form(&form)
                        .map_err(|e| EvalError(e.to_string()))?;
                    call.unit(&crate::emitter::emit_interactive_unit(&program))
                } else {
                    let expanded = compiler
                        .borrow_mut()
                        .macroexpand(&form, once)
                        .map_err(EvalError)?;
                    call.value(&expanded.data())
                }
            });
            let value = self
                .engine
                .install_host_function(name, HostArity::Exact(1), callback)?;
            self.engine.bind(name, value, true)?;
            self.engine.release(value)?;
        }
        for (tag, source) in self.options.data_readers.clone() {
            let function = self.eval_string(&source)?.value;
            let tag = self.value(&Datum::new(DatumKind::Symbol(tag)))?;
            let register = self.engine.evaluate_js("rt.core.readerRegisterTagParser")?;
            let old = self.engine.call(register, &[tag, function])?;
            for id in [old, register, tag, function] {
                self.release(id)?;
            }
        }
        if let Some(source) = self.options.default_data_reader.clone() {
            let function = self.eval_string(&source)?.value;
            let register = self
                .engine
                .evaluate_js("rt.core.readerRegisterDefaultTagParser")?;
            let old = self.engine.call(register, &[function])?;
            for id in [old, register, function] {
                self.release(id)?;
            }
        }
        Ok(())
    }
    fn sync_namespace(&self) -> Result<(), EvalDiagnostic> {
        let name = self.value(&Datum::new(DatumKind::Symbol(
            crate::compiler_data::Name::new(None, &self.namespace()),
        )))?;
        let result = self.engine.call(self.namespace_update, &[name]);
        self.engine.release(name)?;
        self.engine.release(result?)?;
        Ok(())
    }
}

fn file_source(
    options: &EvalOptions,
    path: &str,
    namespace: &str,
) -> Result<SourceInput, EvalError> {
    if path.ends_with(".clj") {
        return Err(EvalError(
            "native .clj evaluation is retired; use .cljs or .cljc".into(),
        ));
    }
    let provider = options
        .file_provider
        .as_ref()
        .ok_or_else(|| EvalError("load-file requires an explicit file provider".into()))?;
    provider(&FileLoadRequest {
        path: path.into(),
        requesting_namespace: namespace.into(),
    })?
    .ok_or_else(|| EvalError(format!("missing file {path}")))
}

fn top_level_do(form: &Form) -> Option<Vec<Form>> {
    if !matches!(
        form.data().kind(),
        DatumKind::Collection {
            kind: crate::CollectionKind::List,
            ..
        }
    ) {
        return None;
    }
    let children = form.children();
    match children.first()?.data().kind() {
        DatumKind::Symbol(name) if name == &crate::compiler_data::Name::new(None, "do") => {
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
    if !matches!(
        form.data().kind(),
        DatumKind::Collection {
            kind: crate::CollectionKind::List,
            ..
        }
    ) {
        return false;
    }
    let children = form.children();
    match children.first().map(|head| head.data().kind().clone()) {
        Some(DatumKind::Symbol(name))
            if name == crate::compiler_data::Name::new(None, "ns*")
                || name == crate::compiler_data::Name::new(Some("cljs.core"), "ns*")
                || name == crate::compiler_data::Name::new(Some("clojure.core"), "ns*") =>
        {
            true
        }
        Some(DatumKind::Symbol(name)) if name == crate::compiler_data::Name::new(None, "do") => {
            children.iter().skip(1).any(has_nsstar_operation)
        }
        _ => false,
    }
}
fn source_operations(form: Form) -> Vec<Form> {
    match top_level_do(&form) {
        Some(children) => children.into_iter().flat_map(source_operations).collect(),
        None => vec![form],
    }
}
/// Record the `light.lazy/loadable` references of `program`, a form at
/// `site`: each namespace the session has neither loaded nor entered, to
/// load, and each Var target, to check once its namespace is loaded.
fn record_lazy(
    compiler: &mut InteractiveCompiler,
    program: &crate::Program,
    loaded: &HashSet<String>,
    site: &SourceSpan,
) {
    for target in &program.lazy_targets {
        let name = &program.lazy_namespaces[target.namespace];
        if !(loaded.contains(name)
            || program.namespaces.contains(name)
            || compiler.lazy.loads.iter().any(|(queued, _)| queued == name))
        {
            compiler.lazy.loads.push_back((name.clone(), site.clone()));
        }
        if let Some(global) = target.global {
            let global = &program.globals[global.0];
            compiler.lazy.checks.push((
                global.namespace.clone(),
                global.name.clone(),
                site.clone(),
            ));
        }
    }
}

/// Whether `program`, a form, constructs a `light.lazy/Loadable` the session
/// has not loaded. A loadable needs no require of `light.lazy`, but its type
/// must exist when the form runs, so the session loads `light.lazy` before
/// the form runs, even while a namespace is loading: it requires only the core.
fn needs_lazy_library(program: &crate::Program, loaded: &HashSet<String>) -> bool {
    !program.lazy_targets.is_empty() && !loaded.contains(LAZY_NAMESPACE)
}

/// A request for the target namespace of a lazy reference.
fn lazy_request(name: &str, requesting: &str) -> LoadRequest {
    LoadRequest {
        namespace: crate::compiler_data::Name::new(None, name),
        requesting_namespace: crate::compiler_data::Name::new(None, requesting),
        requesting_file: None,
        reload: Reload::None,
    }
}

/// Check the recorded Var targets, once their namespaces are loaded: the
/// first one the session does not define is an error at its site.
fn check_lazy(compiler: &mut InteractiveCompiler) -> Result<(), EvalDiagnostic> {
    let checks = std::mem::take(&mut compiler.lazy.checks);
    for (namespace, name, site) in checks {
        if !compiler.defines(&namespace, &name) {
            compiler.lazy = Default::default();
            return Err(EvalDiagnostic {
                data: None,
                message: format!("lazy target {namespace}/{name} is not defined"),
                location: Some(site),
            });
        }
    }
    Ok(())
}

fn missing_namespace(name: &str, lazy: bool) -> String {
    if lazy {
        format!("lazy target namespace {name} not found")
    } else {
        format!("missing namespace {name}")
    }
}

/// Runtime callbacks use the already active engine context and this session's
/// compiler. No second evaluator or filesystem authority is introduced.
fn callback_source(
    call: &HostCall<'_, '_>,
    compiler: &Rc<RefCell<InteractiveCompiler>>,
    namespace_update: ValueId,
    options: &EvalOptions,
    loaded: &Rc<RefCell<HashSet<String>>>,
    loading: &Rc<RefCell<Vec<String>>>,
    source: &SourceInput,
) -> Result<ValueId, EvalDiagnostic> {
    if source.file.ends_with(".clj") {
        return Err(
            EvalError("native .clj evaluation is retired; use .cljs or .cljc".into()).into(),
        );
    }
    if source.source.len() > options.max_source_bytes {
        return Err(EvalError("compiler source byte limit exceeded".into()).into());
    }
    // Load and evaluate each requested namespace not loaded yet, then return
    // to the current namespace; `lazy` requests come from loadable sites, and
    // `location` is the form that made the requests.
    let load = |requests: Vec<LoadRequest>,
                lazy: bool,
                location: &SourceSpan|
     -> Result<(), EvalDiagnostic> {
        let located = |message: String| EvalDiagnostic {
            message,
            location: Some(location.clone()),
            data: None,
        };
        for request in requests {
            let name = String::from_utf16_lossy(request.namespace.name());
            if name == "cljs.core"
                || name == "clojure.core"
                || (loaded.borrow().contains(&name)
                    && request.reload == crate::source_provider::Reload::None)
            {
                continue;
            }
            if loading.borrow().contains(&name) {
                return Err(located(format!(
                    "namespace load cycle: {} -> {name}",
                    loading.borrow().join(" -> ")
                )));
            }
            let dependency = if let Some(text) = crate::compiler_libraries::source(&name) {
                SourceInput::new(format!("<builtin>/{name}.cljs"), text)
            } else {
                options
                    .source_provider
                    .as_ref()
                    .ok_or_else(|| located(format!("no source provider for {name}")))?
                    .load(&request)
                    .map_err(|error| located(error.to_string()))?
                    .ok_or_else(|| located(missing_namespace(&name, lazy)))?
            };
            let namespace = compiler.borrow().namespace().to_owned();
            loading.borrow_mut().push(name.clone());
            let result = callback_source(
                call,
                compiler,
                namespace_update,
                options,
                loaded,
                loading,
                &dependency,
            );
            loading.borrow_mut().pop();
            compiler.borrow_mut().set_namespace(&namespace);
            callback_sync_namespace(call, compiler, namespace_update)?;
            call.release_scoped(result?)?;
            loaded.borrow_mut().insert(name);
        }
        Ok(())
    };
    // `EvalSession::drain_lazy` through this loader.
    let drain_lazy = || -> Result<(), EvalDiagnostic> {
        if !loading.borrow().is_empty() {
            return Ok(());
        }
        loop {
            let next = compiler.borrow_mut().lazy.loads.pop_front();
            let Some((name, site)) = next else { break };
            let request = lazy_request(&name, compiler.borrow().namespace());
            load(vec![request], true, &site)?;
        }
        check_lazy(&mut compiler.borrow_mut())
    };
    let mut cursor = 0;
    let mut last = call.value(&Datum::new(DatumKind::Nil))?;
    loop {
        call.control().poll()?;
        let next = compiler
            .borrow()
            .next_form(source, &mut cursor)
            .map_err(EvalDiagnostic::from)?;
        let Some(form) = next else { return Ok(last) };
        for form in source_operations(form) {
            let located = |message: String| EvalDiagnostic {
                message,
                location: Some(form.span()),
                data: None,
            };
            if loading.borrow().is_empty() {
                // What an earlier top-level form recorded and failed to act on.
                compiler.borrow_mut().lazy = Default::default();
            }
            let requests = compiler.borrow_mut().directive(&form)?;
            if requests.is_some() {
                callback_sync_namespace(call, compiler, namespace_update)?;
            }
            if let Some((requests, staged)) = requests {
                load(requests, false, &form.span())?;
                compiler
                    .borrow_mut()
                    .finish_directive(staged)
                    .map_err(located)?;
                drain_lazy()?;
                let value = call.value(&Datum::new(DatumKind::Nil))?;
                call.release_scoped(last)?;
                last = value;
            } else {
                let program =
                    compiler
                        .borrow_mut()
                        .analyze_form(&form)
                        .map_err(|error| EvalDiagnostic {
                            message: error.to_string(),
                            location: form
                                .span()
                                .source()
                                .span(error.span)
                                .or_else(|| Some(form.span())),
                            data: None,
                        })?;
                if needs_lazy_library(&program, &loaded.borrow()) {
                    let request = lazy_request(LAZY_NAMESPACE, compiler.borrow().namespace());
                    load(vec![request], false, &form.span())?;
                }
                record_lazy(
                    &mut compiler.borrow_mut(),
                    &program,
                    &loaded.borrow(),
                    &form.span(),
                );
                drain_lazy()?;
                let value = call
                    .unit(&crate::emitter::emit_interactive_unit(&program))
                    .map_err(|error| located(error.0))?;
                call.release_scoped(last)?;
                last = value;
            }
        }
    }
}
fn callback_sync_namespace(
    call: &HostCall<'_, '_>,
    compiler: &Rc<RefCell<InteractiveCompiler>>,
    namespace_update: ValueId,
) -> Result<(), EvalError> {
    let name = call.value(&Datum::new(DatumKind::Symbol(
        crate::compiler_data::Name::new(None, compiler.borrow().namespace()),
    )))?;
    let result = call.invoke(namespace_update, &[name]);
    call.release_scoped(name)?;
    call.release_scoped(result?)?;
    Ok(())
}
