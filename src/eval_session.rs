//! Compiler-backed interactive evaluation and explicit embedding capabilities.
use crate::compiler_data::{Datum, DatumKind, Form, Origin, Source, SourceSpan};
use crate::dev_session::{Action, DevSession, Interpreter, Mode, Unit};
pub use crate::eval_compiler::NamePolicy;
use crate::evaluator::{EvalError, Evaluator, EvaluatorOptions, ExecutionControl, ValueId};
pub use crate::evaluator::{HostArity, HostCall};
use crate::form_bridge::ConversionLimits;
use crate::source_provider::{SourceInput, SourceProvider};
use std::{
    cell::{Cell, RefCell},
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
    /// Analyze in warning mode, as `--compile --warnings` does: undeclared
    /// Vars and conflicting arities are warnings, and the code runs as
    /// upstream's would. Off by default, so the session's analysis is strict;
    /// a [`DevSession`] analyzes in warning mode whatever this says
    /// ([`DevSession::set_warning_mode`]).
    pub warning_mode: bool,
    /// Receives the analysis warnings of each form, located, as the compiler
    /// produces them: before the form runs, and before what it loads. The
    /// session drops them without it.
    pub warnings: Option<WarningOutput>,
}
/// The receiver of analysis warnings ([`EvalOptions::warnings`]); an error
/// it returns is the error of the form.
pub type WarningOutput = Rc<dyn Fn(&EvalDiagnostic) -> Result<(), EvalError>>;
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
            warning_mode: false,
            warnings: None,
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
    /// The `Invoke` of the form's unit.
    action: Action,
}
impl AnalyzedForm {
    /// The `Invoke` action of the form's unit, which [`EvalSession::evaluate`]
    /// interprets.
    pub fn action(&self) -> &Action {
        &self.action
    }
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

/// The session: a [`DevSession`], which compiles, and a QuickJS heap, which
/// interprets the compiler's actions as they come (docs/evaluation.md,
/// "Actions"). The runtime's `load-file`, `eval` and `macroexpand` call the
/// same compiler while a unit runs.
pub struct EvalSession {
    engine: Evaluator,
    namespace_update: ValueId,
    dev: Rc<DevSession>,
    options: EvalOptions,
    output_bytes: Rc<RefCell<usize>>,
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
        let dev = DevSession::with_control(&options, Some(engine.control()))?;
        let mut session = Self {
            engine,
            namespace_update,
            dev: Rc::new(dev),
            options,
            output_bytes: Rc::new(RefCell::new(0)),
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
        self.dev.namespace()
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
        self.dev.check_source(source)?;
        let mut cursor = 0;
        let mut last = self.value(&Datum::new(DatumKind::Nil))?;
        loop {
            let form = match self.dev.next_form(source, &mut cursor) {
                Ok(Some(form)) => form,
                Ok(None) => break,
                Err(error) => {
                    let _ = self.engine.release(last);
                    return Err(error);
                }
            };
            let result = self.form(&form);
            let result = self.after_jobs(result);
            let _ = self.engine.release(last);
            last = result?.value;
        }
        Ok(EvalResult {
            value: last,
            namespace: self.namespace(),
        })
    }
    /// Load only through the configured file capability, preserving session roots.
    /// The file runs as one script: promise jobs run after its last form.
    pub fn load_file(&mut self, path: &str) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin_jobs()?;
        let result = {
            let _file = FileLoadGuard::begin(&self.file_loading, path)?;
            file_source(&self.options, path, &self.namespace())
                .map_err(EvalDiagnostic::from)
                .and_then(|source| {
                    self.interpret(|dev, heap| dev.run_source(&source, Mode::Load, heap))
                })
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
    /// Evaluate one top-level form, then run the promise jobs it queued.
    pub fn eval_form(&mut self, form: &Form) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin_jobs()?;
        let result = self.form(form);
        self.after_jobs(result)
    }
    /// Compile one top-level form and interpret its actions.
    fn form(&self, form: &Form) -> Result<EvalResult, EvalDiagnostic> {
        self.interpret(|dev, heap| dev.run_form(form, Mode::Repl, heap))
    }
    /// Compile with `compile`, interpreting each action in this heap as the
    /// compiler produces it. The value is the value of the last action when
    /// that ran a unit, else nil.
    fn interpret(
        &self,
        compile: impl FnOnce(&DevSession, &mut dyn Interpreter) -> Result<(), EvalDiagnostic>,
    ) -> Result<EvalResult, EvalDiagnostic> {
        let mut heap = Heap {
            session: self,
            last: None,
        };
        let result = compile(&self.dev, &mut heap);
        let last = heap.last.take();
        if let Err(error) = result {
            if let Some(value) = last {
                let _ = self.engine.release(value);
            }
            return Err(error);
        }
        Ok(EvalResult {
            value: match last {
                Some(value) => value,
                None => self.value(&Datum::new(DatumKind::Nil))?,
            },
            namespace: self.namespace(),
        })
    }
    pub fn analyze(&mut self, form: &Form) -> Result<AnalyzedForm, EvalDiagnostic> {
        let _operation = self.begin()?;
        self.engine.control().poll()?;
        Ok(AnalyzedForm {
            owner: self.identity,
            action: Action::Invoke(self.dev.analyze_unit(form)?),
        })
    }
    pub fn evaluate(&self, analyzed: &AnalyzedForm) -> Result<EvalResult, EvalDiagnostic> {
        let _operation = self.begin()?;
        if analyzed.owner != self.identity {
            return Err(EvalError("analysis belongs to another session".into()).into());
        }
        let result = self.interpret(|_, heap| heap.apply(analyzed.action.clone()));
        // The unit registered its namespaces as they were when it was
        // analyzed, perhaps before units that ran since.
        self.dev.forget_registrations();
        result
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
        self.dev
            .compiler_mut()
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
                self.dev
                    .compiler()
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
        self.dev.read_forms(source)
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
        self.dev
            .compiler_mut()
            .declare_binding(ns, local)
            .map_err(|e| EvalError(e.to_string()))?;
        Ok(self.engine.bind(&format!("{ns}/{local}"), value, false)?)
    }
    pub fn bindings(&self) -> Vec<String> {
        self.dev
            .compiler()
            .globals()
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
        self.sync_namespace(&self.namespace())?;
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
        // The runtime's `load-file` compiles the file through this session's
        // compiler and interprets its actions in the active engine context,
        // as `load_file` does.
        let dev = Rc::downgrade(&self.dev);
        let namespace_update = self.namespace_update;
        let options = self.options.clone();
        let file_loading = self.file_loading.clone();
        let callback: HostCallback = Rc::new(move |call, args| {
            let dev = dev
                .upgrade()
                .ok_or_else(|| EvalError("session dropped".into()))?;
            let path = call.string(args[0])?;
            let _file = FileLoadGuard::begin(&file_loading, &path)?;
            let source = file_source(&options, &path, &dev.namespace())?;
            let mut runtime = Callback {
                call,
                namespace_update,
                last: None,
                warnings: options.warnings.clone(),
            };
            let result = dev.run_source(&source, Mode::Load, &mut runtime);
            runtime
                .finish(result)
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
            let dev = Rc::downgrade(&self.dev);
            let evaluate = name == "eval";
            let callback: HostCallback = Rc::new(move |call, args| {
                let dev = dev
                    .upgrade()
                    .ok_or_else(|| EvalError("session dropped".into()))?;
                let data = call.data(args[0])?;
                let source = Source::new("<runtime-eval>", "");
                let form = Form::from_data(data, source.span(0..0).unwrap(), Origin::Source);
                if evaluate {
                    let unit = dev
                        .analyze_unit(&form)
                        .map_err(|error| EvalError(error.message))?;
                    call.unit(&unit.js)
                } else {
                    let expanded = dev
                        .compiler_mut()
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
    /// Make `namespace` the runtime's current namespace (`*ns*`).
    fn sync_namespace(&self, namespace: &str) -> Result<(), EvalDiagnostic> {
        let name = self.value(&Datum::new(DatumKind::Symbol(
            crate::compiler_data::Name::new(None, namespace),
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

/// The session's heap as the interpreter of its compiler's actions: units
/// run in the heap, a namespace becomes the runtime's current one, and the
/// value of the last action that ran a unit is kept.
struct Heap<'a> {
    session: &'a EvalSession,
    last: Option<ValueId>,
}
impl Heap<'_> {
    fn keep(&mut self, value: Option<ValueId>) {
        if let Some(old) = std::mem::replace(&mut self.last, value) {
            let _ = self.session.engine.release(old);
        }
    }
    fn unit(&mut self, unit: &Unit) -> Result<(), EvalDiagnostic> {
        let engine = &self.session.engine;
        let value = engine
            .evaluate_unit(&unit.js)
            .map_err(|error| EvalDiagnostic {
                data: engine.error_data(),
                message: error.0,
                location: Some(unit.location.clone()),
            })?;
        self.keep(Some(value));
        Ok(())
    }
}
impl Interpreter for Heap<'_> {
    fn begin_form(&mut self) -> Result<(), EvalDiagnostic> {
        Ok(self.session.engine.input_action("clearError", 0)?)
    }
    fn apply(&mut self, action: Action) -> Result<(), EvalDiagnostic> {
        match action {
            Action::Invoke(unit) => self.unit(&unit),
            Action::Load(units) => units.iter().try_for_each(|unit| self.unit(unit)),
            Action::Namespace(name) => {
                self.session.sync_namespace(&name)?;
                self.keep(None);
                Ok(())
            }
            Action::Warnings(warnings) => warn(&self.session.options.warnings, &warnings),
            Action::Error(error) => Err(error),
        }
    }
}

/// Hand `warnings` to the session's receiver, if it has one.
fn warn(output: &Option<WarningOutput>, warnings: &[EvalDiagnostic]) -> Result<(), EvalDiagnostic> {
    if let Some(output) = output {
        for warning in warnings {
            output(warning)?;
        }
    }
    Ok(())
}

/// The heap of a runtime callback (`load-file` while a unit runs) as the
/// interpreter of the compiler's actions, in the already active engine
/// context: no second evaluator or filesystem authority is introduced.
struct Callback<'c, 'a, 'js> {
    call: &'c HostCall<'a, 'js>,
    namespace_update: ValueId,
    last: Option<ValueId>,
    warnings: Option<WarningOutput>,
}
impl Callback<'_, '_, '_> {
    fn keep(&mut self, value: Option<ValueId>) -> Result<(), EvalDiagnostic> {
        if let Some(old) = std::mem::replace(&mut self.last, value) {
            self.call.release_scoped(old)?;
        }
        Ok(())
    }
    fn unit(&mut self, unit: &Unit) -> Result<(), EvalDiagnostic> {
        let value = self.call.unit(&unit.js).map_err(|error| EvalDiagnostic {
            message: error.0,
            location: Some(unit.location.clone()),
            data: None,
        })?;
        self.keep(Some(value))
    }
    /// The value of the compilation that ended with `result`.
    fn finish(mut self, result: Result<(), EvalDiagnostic>) -> Result<ValueId, EvalDiagnostic> {
        result?;
        match self.last.take() {
            Some(value) => Ok(value),
            None => Ok(self.call.value(&Datum::new(DatumKind::Nil))?),
        }
    }
}
impl Interpreter for Callback<'_, '_, '_> {
    fn apply(&mut self, action: Action) -> Result<(), EvalDiagnostic> {
        match action {
            Action::Invoke(unit) => self.unit(&unit),
            Action::Load(units) => units.iter().try_for_each(|unit| self.unit(unit)),
            Action::Namespace(name) => {
                let name = self.call.value(&Datum::new(DatumKind::Symbol(
                    crate::compiler_data::Name::new(None, &name),
                )))?;
                let result = self.call.invoke(self.namespace_update, &[name]);
                self.call.release_scoped(name)?;
                self.call.release_scoped(result?)?;
                self.keep(None)
            }
            Action::Warnings(warnings) => warn(&self.warnings, &warnings),
            Action::Error(error) => Err(error),
        }
    }
}
