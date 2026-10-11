//! Resolved, owned instructions and sequential execution of the primitive core.
use super::*;
use crate::parser::CollectionKind;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum EvalPhase {
    Read,
    Analysis,
    Macroexpansion,
    Execution,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum EvalErrorKind {
    Read(ReadErrorKind),
    UnresolvedSymbol(Name),
    Namespace(NamespaceError),
    Load(LoadError),
    Var(VarError),
    InvalidForm(&'static str),
    InvalidProtocolImplementation {
        protocol: Box<Name>,
        method: Box<Name>,
        message: &'static str,
    },
    UnsupportedForm(&'static str),
    WrongOwner,
    PermissionDenied(Name),
    DuplicateMapKey,
    DuplicateSetMember,
    Collection(CollectionError),
    Native {
        name: Box<Name>,
        error: NativeError,
    },
    MissingProtocolImplementation {
        protocol: Box<Name>,
        method: Box<Name>,
        receiver: &'static str,
    },
    NotCallable,
    NoMatchingClause,
    Thrown(ExceptionCategory),
    InvalidThrow,
    InvalidReferenceState,
    Arity {
        fixed: Box<[usize]>,
        variadic: Option<usize>,
        actual: usize,
    },
    CallDepthExceeded,
    EvaluationDepthExceeded,
    CleanupLimitExceeded,
    ExecutionInterrupted(String),
    Resource(ResourceError),
}

impl fmt::Display for EvalErrorKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Native { name, error } => {
                if let Some(namespace) = name.namespace() {
                    write!(f, "{}/", String::from_utf16_lossy(namespace))?;
                }
                write!(f, "{}: {error}", String::from_utf16_lossy(name.name()))
            }
            Self::MissingProtocolImplementation {
                protocol,
                method,
                receiver,
            } => write!(
                f,
                "missing implementation of {}/{} method {}/{} for {receiver}",
                String::from_utf16_lossy(protocol.namespace().unwrap_or_default()),
                String::from_utf16_lossy(protocol.name()),
                String::from_utf16_lossy(method.namespace().unwrap_or_default()),
                String::from_utf16_lossy(method.name())
            ),
            Self::InvalidProtocolImplementation {
                protocol,
                method,
                message,
            } => write!(
                f,
                "{message}: {}/{} method {}/{}",
                String::from_utf16_lossy(protocol.namespace().unwrap_or_default()),
                String::from_utf16_lossy(protocol.name()),
                String::from_utf16_lossy(method.namespace().unwrap_or_default()),
                String::from_utf16_lossy(method.name())
            ),
            Self::InvalidReferenceState => f.write_str("Invalid reference state"),
            other => write!(f, "{other:?}"),
        }
    }
}

/// Public failure boundary. Only Language errors participate in catch dispatch.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ErrorClass {
    Language,
    Host,
    Control,
    Diagnostic,
}

/// Call-site provenance, allocated only while a failure unwinds. Formatting and
/// line/column indexing happen on demand, never on the successful call path.
#[derive(Clone, Debug)]
pub struct SourceFrame {
    pub span: SourceSpan,
    pub origin: Origin,
}

/// Errors retain their input and exact byte spans after reset or context drop.
#[derive(Clone, Debug)]
pub struct EvalError {
    pub phase: EvalPhase,
    pub kind: EvalErrorKind,
    pub span: Box<SourceSpan>,
    pub related: Option<Box<SourceSpan>>,
    pub origin: Origin,
    pub frames: Vec<SourceFrame>,
    pub exception: Option<Box<RootedValue>>,
}
impl EvalErrorKind {
    /// Exhaustive classification makes new resource/control variants require an
    /// explicit decision before they can participate in language catch dispatch.
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Var(VarError::Resource(_))
            | Self::Namespace(NamespaceError::Resource(_))
            | Self::Resource(_)
            | Self::Collection(CollectionError::Resource(_))
            | Self::CallDepthExceeded
            | Self::EvaluationDepthExceeded
            | Self::CleanupLimitExceeded
            | Self::ExecutionInterrupted(_) => ErrorClass::Control,
            Self::Var(VarError::Handle(_))
            | Self::Load(LoadError::Provider(_))
            | Self::WrongOwner
            | Self::Collection(CollectionError::WrongOwner) => ErrorClass::Host,
            Self::Native { error, .. } => match error {
                NativeError::Collection(CollectionError::Resource(_)) => ErrorClass::Control,
                NativeError::Host(_)
                | NativeError::WrongOwner
                | NativeError::Collection(CollectionError::WrongOwner) => ErrorClass::Host,
                NativeError::Invocation(error) => match error.error() {
                    InvokeError::Dispatch(kind) => kind.class(),
                    InvokeError::Execution(error) => error.class(),
                },
                NativeError::EndOfInput
                | NativeError::Arity { .. }
                | NativeError::StringBounds { .. }
                | NativeError::Type { .. }
                | NativeError::Arithmetic(_)
                | NativeError::UnsupportedNumber(_)
                | NativeError::Collection(_) => ErrorClass::Language,
            },
            Self::PermissionDenied(_)
            | Self::Read(_)
            | Self::UnresolvedSymbol(_)
            | Self::Namespace(_)
            | Self::InvalidForm(_)
            | Self::InvalidProtocolImplementation { .. }
            | Self::UnsupportedForm(_) => ErrorClass::Diagnostic,
            Self::Var(_)
            | Self::Load(_)
            | Self::DuplicateMapKey
            | Self::DuplicateSetMember
            | Self::Collection(_)
            | Self::MissingProtocolImplementation { .. }
            | Self::NotCallable
            | Self::NoMatchingClause
            | Self::Thrown(_)
            | Self::InvalidThrow
            | Self::InvalidReferenceState
            | Self::Arity { .. } => ErrorClass::Language,
        }
    }
}
impl EvalError {
    pub fn class(&self) -> ErrorClass {
        if self.kind.class() == ErrorClass::Control {
            ErrorClass::Control
        } else if self.phase != EvalPhase::Execution {
            ErrorClass::Diagnostic
        } else if matches!(self.kind, EvalErrorKind::Read(_)) {
            // Reading EDN text during execution is an ordinary language failure.
            // Source reading before execution still reports a diagnostic above.
            ErrorClass::Language
        } else {
            self.kind.class()
        }
    }
}
impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (line, column) = self
            .span
            .source()
            .line_column(self.span.bytes().start)
            .unwrap();
        write!(
            f,
            "{}:{line}:{column}: {:?}: {}",
            self.span.source().name(),
            self.phase,
            self.kind
        )?;
        if self.origin != Origin::Source {
            write!(f, " [{:?}]", self.origin)?;
        }
        if let Some(message) = self.exception.as_ref().and_then(|e| e.exception_message())
            && let ValueView::String(message) = message.inspect()
        {
            write!(f, ": {}", String::from_utf16_lossy(&message))?;
        }
        for frame in &self.frames {
            let (line, column) = frame
                .span
                .source()
                .line_column(frame.span.bytes().start)
                .unwrap();
            write!(
                f,
                "\n  at {}:{line}:{column} ({:?})",
                frame.span.source().name(),
                frame.origin
            )?;
        }
        Ok(())
    }
}
impl std::error::Error for EvalError {}
impl From<ReadError> for EvalError {
    fn from(error: ReadError) -> Self {
        if let ReadErrorKind::Lower(LowerErrorKind::DataReader { tag, error: native }) = error.kind
        {
            let kind = match *native {
                NativeError::Invocation(forwarded) => match forwarded.into_error() {
                    InvokeError::Execution(mut body) => {
                        body.frames.insert(
                            0,
                            SourceFrame {
                                span: (*body.span).clone(),
                                origin: body.origin.clone(),
                            },
                        );
                        *body.span = error.span;
                        body.phase = EvalPhase::Read;
                        body.origin = Origin::DataReader(Rc::new(*tag));
                        return *body;
                    }
                    InvokeError::Dispatch(kind) => kind,
                },
                error => EvalErrorKind::Native {
                    name: tag.clone(),
                    error,
                },
            };
            return Self {
                phase: EvalPhase::Read,
                kind,
                span: Box::new(error.span),
                related: error.related.map(Box::new),
                origin: Origin::DataReader(Rc::new(*tag)),
                frames: Vec::new(),
                exception: None,
            };
        }
        Self {
            phase: EvalPhase::Read,
            kind: match error.kind {
                ReadErrorKind::Lower(LowerErrorKind::Resource(e)) => EvalErrorKind::Resource(e),
                kind => EvalErrorKind::Read(kind),
            },
            span: Box::new(error.span),
            related: error.related.map(Box::new),
            origin: Origin::Source,
            frames: Vec::new(),
            exception: None,
        }
    }
}

/// Direct invocation has no caller source span. Body errors preserve their
/// original location; dispatch failures acquire the call span during evaluation.
#[derive(Clone, Debug)]
pub enum InvokeError {
    Dispatch(EvalErrorKind),
    // Boxing keeps failure-only diagnostic payloads off recursive success frames.
    Execution(Box<EvalError>),
}
impl fmt::Display for InvokeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Dispatch(kind) => write!(f, "{kind}"),
            Self::Execution(error) => error.fmt(f),
        }
    }
}
impl std::error::Error for InvokeError {}
impl InvokeError {
    pub fn class(&self) -> ErrorClass {
        match self {
            Self::Dispatch(kind) => kind.class(),
            Self::Execution(error) => error.class(),
        }
    }
}
impl From<CollectionError> for InvokeError {
    fn from(error: CollectionError) -> Self {
        Self::Dispatch(EvalErrorKind::Collection(error))
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct ExprId(usize);
#[derive(Clone, Copy, Debug)]
pub(super) struct ConstantId(usize);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LocalSlot(usize);
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Access {
    Local(LocalSlot),
    Capture(usize),
    SelfValue,
}
/// Cooperative execution boundaries; polling does not interrupt host callbacks.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum PollPoint {
    Function,
    Recur,
    /// One iteration of an eager sequence consumer or function combinator.
    Iteration,
    Read,
    Analysis,
    Expression,
    Macroexpansion,
    Host,
}
/// Return an error to stop evaluation at a function, backedge or iteration boundary.
pub type ExecutionPoll = fn(PollPoint) -> Result<(), String>;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct TargetId(usize);
pub(super) struct Target {
    id: TargetId,
    slots: Box<[LocalSlot]>,
}
#[derive(Clone, Copy, Debug)]
pub(super) struct FunctionId(usize);
pub(super) struct FunctionCode {
    captures: Box<[Access]>,
    methods: Box<[Method]>,
}
struct Method {
    fixed: usize,
    variadic: bool,
    body: ExprId,
    local_count: usize,
    target: Target,
}
// Raw edges only: cycles through letfn captures are traced by the owner.
pub(super) struct Closure {
    pub(super) unit: UnitId,
    function: FunctionId,
    pub(super) captures: Box<[Value]>,
}

// Construction primitives validate even callable literals recovered from a
// macro expansion. Only fixed method closures matching the descriptor enter
// implementation tables; an omitted declared overload still fails ordinary fn arity.
pub(super) fn protocol_implementation_arities(
    store: &Store,
    value: Value,
    arities: &[usize],
) -> Result<bool, ResourceError> {
    let Some(HeapValue::Closure(closure)) = store.heap_value(value) else {
        return Ok(false);
    };
    let code = store
        .units
        .get(closure.unit.0)
        .expect("rooted closure code");
    let methods = &code.functions[closure.function.0].methods;
    store
        .accounting
        .work((methods.len() as u64).saturating_mul(arities.len() as u64 + 1))?;
    Ok(methods
        .iter()
        .all(|m| !m.variadic && arities.contains(&m.fixed)))
}

// No names or locations are consulted by successful execution. Each expression's
// provenance is in the unit's locations table at the matching ExprId.
pub(super) enum Expr {
    Constant(ConstantId),
    Local(Access),
    Var(VarId),
    SetVar {
        var: VarId,
        value: ExprId,
    },
    Def {
        var: VarId,
        init: Option<ExprId>,
        metadata: Option<ExprId>,
        base: ConstantId,
        once: bool,
    },
    Function(FunctionId),
    LetFn {
        bindings: Box<[(LocalSlot, FunctionId)]>,
        body: ExprId,
    },
    If {
        test: ExprId,
        yes: ExprId,
        no: ExprId,
    },
    Case {
        value: ExprId,
        table: ConstantId,
        branches: Box<[ExprId]>,
        default: Option<ExprId>,
    },
    Do(Box<[ExprId]>),
    Throw(ExprId),
    Try {
        body: ExprId,
        catches: Box<[Catch]>,
        finally: Option<ExprId>,
    },
    Call {
        callee: ExprId,
        arguments: Box<[ExprId]>,
    },
    Let {
        bindings: Box<[(LocalSlot, ExprId)]>,
        body: ExprId,
    },
    Loop {
        bindings: Box<[(LocalSlot, ExprId)]>,
        body: ExprId,
        target: Target,
    },
    Recur {
        target: TargetId,
        arguments: Box<[ExprId]>,
    },
    Collection {
        kind: CollectionKind,
        children: Box<[ExprId]>,
        metadata: Option<ExprId>,
    },
}

pub(super) struct Catch {
    category: ExceptionCategory,
    slot: LocalSlot,
    body: ExprId,
}

/// One analyzed expression with its rooted code, constants and provenance.
/// Analysis checks the entire expression, including unreachable branches. Use
/// `eval_form`/`eval_source` for incremental top-level `do` semantics instead.
#[derive(Clone, Debug)]
pub struct AnalyzedForm {
    unit: ExecutableUnit,
    root: ExprId,
}
impl AnalyzedForm {
    pub fn unit(&self) -> &ExecutableUnit {
        &self.unit
    }
    pub fn span(&self) -> SourceSpan {
        self.unit.span(self.root.0).expect("expression span")
    }
    pub fn expression_count(&self) -> usize {
        self.unit.code.expressions.len()
    }
    pub fn local_count(&self) -> usize {
        self.unit.code.local_count
    }
}

/// Evaluate in a fresh, isolated context. The result owns its runtime values.
pub fn eval_string(text: &str) -> Result<RootedValue, EvalError> {
    assert_execution_enabled();
    Context::new().eval_string(text)
}

#[derive(Default)]
pub(super) struct Scratch {
    locals: Vec<Option<RootedValue>>,
    values: Vec<RootedValue>,
}
impl Scratch {
    pub(super) fn clear(&mut self) {
        self.locals.clear();
        self.values.clear();
    }
}

impl Context {
    /// Read, analyze and execute each top-level form before reading the next.
    /// Empty input returns nil. No host capabilities are installed by default.
    pub fn eval_string(&mut self, text: &str) -> Result<RootedValue, EvalError> {
        assert_execution_enabled();
        let source = self.source("<eval>", text);
        self.eval_source(&source)
    }

    /// Evaluate an owned, named source; diagnostics keep that source alive.
    pub fn eval_source(&mut self, source: &Source) -> Result<RootedValue, EvalError> {
        assert_execution_enabled();
        let _scope = self.accounting.enter();
        let owner = self.owner.clone();
        let previous = self
            .enter_source_context(source)
            .map_err(|error| EvalError {
                phase: EvalPhase::Read,
                kind: error.kind(),
                span: Box::new(source.span(0..source.text().len()).unwrap()),
                related: None,
                origin: Origin::Source,
                frames: Vec::new(),
                exception: None,
            })?;
        let result = self.eval_source_inner(source);
        if Rc::ptr_eq(&owner, &self.owner) {
            self.restore_source_context(previous);
            if let Err(error) = self.accounting.check()
                && !result
                    .as_ref()
                    .is_err_and(|e| e.class() == ErrorClass::Control)
            {
                return Err(EvalError {
                    phase: EvalPhase::Execution,
                    kind: self.accounting.kind(error),
                    span: Box::new(source.span(0..source.text().len()).unwrap()),
                    related: None,
                    origin: Origin::Source,
                    frames: Vec::new(),
                    exception: None,
                });
            }
        }
        result
    }

    fn eval_source_inner(&mut self, source: &Source) -> Result<RootedValue, EvalError> {
        self.poll_execution(PollPoint::Read)
            .map_err(|kind| EvalError {
                phase: EvalPhase::Read,
                kind,
                span: Box::new(source.span(0..source.text().len()).unwrap()),
                related: None,
                origin: Origin::Source,
                frames: Vec::new(),
                exception: None,
            })?;
        let mut reader = self.reader(source).map_err(|_| EvalError {
            phase: EvalPhase::Read,
            kind: EvalErrorKind::WrongOwner,
            span: Box::new(source.span(0..0).expect("empty source span")),
            related: None,
            origin: Origin::Source,
            frames: Vec::new(),
            exception: None,
        })?;
        let mut result = self.nil();
        while let Some(form) = reader.read_form()? {
            result = self.eval_form(&form)?;
        }
        Ok(result)
    }

    /// Execute a form as top-level input. A top-level `do` recursively analyzes
    /// and executes its children in order, allowing future definitions/macros
    /// to affect analysis of later children. A nested expression is analyzed
    /// completely before any of its instructions execute.
    pub fn eval_form(&mut self, form: &Form) -> Result<RootedValue, EvalError> {
        assert_execution_enabled();
        let _scope = self.accounting.enter();
        if self.macros.form_depth >= self.accounting.depths().1 {
            return Err(analysis_error(
                form,
                self.accounting
                    .kind(self.accounting.fail(ResourceError::EvaluationDepthExceeded)),
            ));
        }
        self.macros.form_depth += 1;
        let result = self.eval_form_inner(form);
        self.macros.form_depth -= 1;
        if !result
            .as_ref()
            .is_err_and(|e| e.class() == ErrorClass::Control)
        {
            self.accounting
                .check()
                .map_err(|e| analysis_error(form, self.accounting.kind(e)))?;
        }
        result
    }
    fn eval_form_inner(&mut self, form: &Form) -> Result<RootedValue, EvalError> {
        self.check_form_owner(form)?;
        self.poll_execution(PollPoint::Analysis)
            .map_err(|kind| analysis_error(form, kind))?;
        let expanded = self.expand_form(form, &self.nil())?;
        let form = &expanded;
        if let FormKind::Collection {
            kind: CollectionKind::List,
            children,
        } = &form.node().kind
            && let Some(first) = children.first()
            && is_named(&form.at(*first).unwrap(), "do")
        {
            self.check_primitive(&Name::new(None, "do"))
                .map_err(|kind| analysis_error(form, kind))?;
            let mut result = self.nil();
            for child in &children[1..] {
                result = self.eval_form(&form.at(*child).unwrap())?;
            }
            return Ok(result);
        }
        let analyzed = self.analyze_form(form)?;
        self.eval_analyzed(&analyzed)
    }

    /// Resolve lexical symbols once into dense slots. The immutable result can
    /// be executed repeatedly in this owner without reading/analyzing again.
    pub fn analyze_form(&mut self, form: &Form) -> Result<AnalyzedForm, EvalError> {
        let _scope = self.accounting.enter();
        self.check_form_owner(form)?;
        self.poll_execution(PollPoint::Analysis)
            .map_err(|kind| analysis_error(form, kind))?;
        if let Some(analyzed) = self.reused_analysis(form) {
            return Ok(analyzed);
        }
        let started = self.macros.code_diagnostics.then(std::time::Instant::now);
        let invocations = self.macros.invocations;
        let mut scratch = std::mem::take(&mut self.analysis_scratch);
        scratch.scopes.push(Scope::default());
        let mut analyzer = Analyzer {
            context: self,
            expressions: Vec::new(),
            scratch,
            functions: Vec::new(),
            next_target: 0,
            permissions: Vec::new(),
            depth: 0,
            cacheable: true,
        };
        let result: Result<AnalyzedForm, EvalError> = (|| {
            let root = analyzer.analyze(form, RecurContext::default())?;
            analyzer
                .context
                .accounting
                .check()
                .map_err(|e| analysis_error(form, analyzer.context.accounting.kind(e)))?;
            let code = Rc::new(CodeUnit {
                constants: analyzer.constants.iter().map(RootedValue::value).collect(),
                locations: analyzer
                    .spans
                    .iter()
                    .zip(&analyzer.origins)
                    .map(|(span, origin)| Location {
                        source: span.source.id(),
                        bytes: span.bytes.clone(),
                        origin: origin.clone(),
                    })
                    .collect(),
                expressions: std::mem::take(&mut analyzer.expressions).into_boxed_slice(),
                local_count: analyzer.scopes[0].local_count,
                functions: std::mem::take(&mut analyzer.functions).into_boxed_slice(),
                vars: analyzer
                    .vars
                    .iter()
                    .map(|var| var.var_id().expect("analyzed Var"))
                    .collect(),
                permissions: std::mem::take(&mut analyzer.permissions).into_boxed_slice(),
            });
            let id = UnitId(analyzer.context.owner.store.borrow_mut().units.insert(code));
            let analyzed = AnalyzedForm {
                unit: analyzer.context.owner.unit(id).expect("new analyzed unit"),
                root,
            };
            analyzer
                .context
                .accounting
                .check()
                .map_err(|e| analysis_error(form, analyzer.context.accounting.kind(e)))?;
            Ok(analyzed)
        })();
        let cacheable = analyzer.cacheable;
        let mut scratch = analyzer.scratch;
        // Clear runtime/source roots on success AND failure. Keep only bounded
        // empty backing buffers, including across reentrant macro callbacks.
        scratch.clear();
        self.analysis_scratch = scratch;
        let analyzed = result?;
        self.record_code_generation(form, &analyzed, started);
        if cacheable && self.macros.invocations == invocations {
            self.retain_analysis(form, &analyzed);
        }
        Ok(analyzed)
    }

    pub(super) fn callable_definition(&self, value: &RootedValue) -> Option<DiagnosticLocation> {
        let store = self.owner.store.borrow();
        let HeapValue::Closure(closure) = store.heap_value(value.value())? else {
            return None;
        };
        let code = store.units.get(closure.unit.0).ok()?;
        let index = code
            .expressions
            .iter()
            .position(|expression| match expression {
                Expr::Function(id) => id.0 == closure.function.0,
                Expr::LetFn { bindings, .. } => {
                    bindings.iter().any(|(_, id)| id.0 == closure.function.0)
                }
                _ => false,
            })?;
        Some(DiagnosticLocation::new(&store, &code.locations[index]))
    }

    pub fn eval_analyzed(&mut self, analyzed: &AnalyzedForm) -> Result<RootedValue, EvalError> {
        assert_execution_enabled();
        let _scope = self.accounting.enter();
        if !Rc::ptr_eq(&self.owner, &analyzed.unit.owner) {
            return Err(EvalError {
                phase: EvalPhase::Execution,
                kind: EvalErrorKind::WrongOwner,
                span: Box::new(analyzed.span()),
                related: None,
                origin: Origin::Source,
                frames: Vec::new(),
                exception: None,
            });
        }
        for permission in &analyzed.unit.code.permissions {
            self.check_primitive(permission).map_err(|kind| EvalError {
                phase: EvalPhase::Execution,
                kind,
                span: Box::new(analyzed.span()),
                related: None,
                origin: Origin::Source,
                frames: Vec::new(),
                exception: None,
            })?;
        }
        for var in &analyzed.unit.code.vars {
            self.check_var_access(*var).map_err(|kind| EvalError {
                phase: EvalPhase::Execution,
                kind,
                span: Box::new(analyzed.span()),
                related: None,
                origin: Origin::Source,
                frames: Vec::new(),
                exception: None,
            })?;
        }
        let mut scratch = std::mem::take(&mut self.evaluation);
        scratch
            .locals
            .resize_with(analyzed.unit.code.local_count, || None);
        let result = Evaluator {
            context: self,
            unit: &analyzed.unit,
            scratch: &mut scratch,
            base: 0,
            callee: None,
        }
        .eval_value(analyzed.root);
        // Release every temporary root even on errors; capacity survives reuse.
        scratch.clear();
        self.evaluation = scratch;
        result.map_err(|error| *error)
    }

    /// Call a native or interpreted function without reparsing or reanalysis.
    pub fn invoke(
        &mut self,
        callee: &RootedValue,
        arguments: &[RootedValue],
    ) -> Result<RootedValue, InvokeError> {
        assert_execution_enabled();
        let _scope = self.accounting.enter();
        self.accounting
            .check()
            .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        if !Rc::ptr_eq(&callee.owner, &self.owner)
            || arguments.iter().any(|v| !Rc::ptr_eq(&v.owner, &self.owner))
        {
            return Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner));
        }
        self.charge_allocation(arguments.len() as u64)
            .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        let mut scratch = std::mem::take(&mut self.evaluation);
        scratch.values.extend_from_slice(arguments);
        let result = invoke(self, callee, 0, &mut scratch, None);
        scratch.clear();
        self.evaluation = scratch;
        if !result
            .as_ref()
            .is_err_and(|error| error.class() == ErrorClass::Control)
        {
            self.accounting
                .check()
                .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        }
        result
    }

    /// Apply an already seq-coerced argument list. Variadic functions retain the
    /// unused tail, including deferred projections, instead of realizing it.
    pub(super) fn invoke_sequence(
        &mut self,
        callee: &RootedValue,
        arguments: &RootedValue,
    ) -> Result<RootedValue, InvokeError> {
        let _scope = self.accounting.enter();
        self.accounting
            .check()
            .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        if !Rc::ptr_eq(&callee.owner, &self.owner) || !Rc::ptr_eq(&arguments.owner, &self.owner) {
            return Err(InvokeError::Dispatch(EvalErrorKind::WrongOwner));
        }
        let mut scratch = std::mem::take(&mut self.evaluation);
        let result = invoke(self, callee, 0, &mut scratch, Some(arguments));
        scratch.clear();
        self.evaluation = scratch;
        if !result
            .as_ref()
            .is_err_and(|error| error.class() == ErrorClass::Control)
        {
            self.accounting
                .check()
                .map_err(|e| InvokeError::Dispatch(self.accounting.kind(e)))?;
        }
        result
    }

    fn check_form_owner(&self, form: &Form) -> Result<(), EvalError> {
        if Rc::ptr_eq(&self.owner, &form.data().owner) {
            Ok(())
        } else {
            Err(analysis_error(form, EvalErrorKind::WrongOwner))
        }
    }
}

fn is_named(form: &Form, name: &str) -> bool {
    matches!(form.data().inspect(), ValueView::Symbol(n) if n == Name::new(None, name))
}
fn is_core_named(form: &Form, name: &str) -> bool {
    matches!(form.data().inspect(), ValueView::Symbol(n) if n == Name::new(None, name) || n == Name::new(Some("clojure.core"), name))
}
pub(super) fn analysis_error(form: &Form, kind: EvalErrorKind) -> EvalError {
    EvalError {
        phase: EvalPhase::Analysis,
        kind,
        span: Box::new(form.span()),
        related: None,
        origin: form.node().origin.clone(),
        frames: Vec::new(),
        exception: None,
    }
}

struct Analyzer<'a> {
    context: &'a mut Context,
    expressions: Vec<Expr>,
    scratch: AnalysisScratch,
    functions: Vec<FunctionCode>,
    next_target: usize,
    permissions: Vec<Name>,
    depth: usize,
    cacheable: bool,
}
#[derive(Default)]
pub(super) struct AnalysisScratch {
    constants: Vec<RootedValue>,
    spans: Vec<SourceSpan>,
    origins: Vec<Origin>,
    scopes: Vec<Scope>,
    // Macros may unmap Vars and collect while analysis is in progress.
    vars: Vec<RootedValue>,
}
impl AnalysisScratch {
    fn clear(&mut self) {
        fn reset<T>(buffer: &mut Vec<T>) {
            buffer.clear();
            if buffer.capacity() > 8_192 {
                *buffer = Vec::new();
            }
        }
        reset(&mut self.constants);
        reset(&mut self.spans);
        reset(&mut self.origins);
        reset(&mut self.scopes);
        reset(&mut self.vars);
    }
}
impl std::ops::Deref for Analyzer<'_> {
    type Target = AnalysisScratch;
    fn deref(&self) -> &Self::Target {
        &self.scratch
    }
}
impl std::ops::DerefMut for Analyzer<'_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.scratch
    }
}
#[derive(Default)]
struct Scope {
    bindings: Vec<(SymbolId, Access, macros::BindingIdentity)>,
    captures: Vec<Access>,
    local_count: usize,
}
// Target identity is lexical, never inherited across a function boundary.
// A try analyzer must increment try_depth for its protected body/catch/finally;
// a fresh loop target records that depth and a fresh function starts at zero.
#[derive(Clone, Copy, Default)]
struct RecurContext {
    target: Option<(TargetId, usize, usize)>,
    tail: bool,
    try_depth: usize,
}
impl RecurContext {
    fn non_tail(self) -> Self {
        Self {
            tail: false,
            ..self
        }
    }
    fn with_target(self, id: TargetId, arity: usize) -> Self {
        Self {
            target: Some((id, arity, self.try_depth)),
            tail: true,
            ..self
        }
    }
}
impl Analyzer<'_> {
    fn target(&mut self, slots: Vec<LocalSlot>) -> Target {
        let id = TargetId(self.next_target);
        self.next_target += 1;
        Target {
            id,
            slots: slots.into_boxed_slice(),
        }
    }
    fn scope(&mut self) -> &mut Scope {
        self.scopes.last_mut().unwrap()
    }
    fn bind(&mut self, symbol: SymbolId) -> LocalSlot {
        let identity = self.context.macros.binding_identity();
        let scope = self.scope();
        let slot = LocalSlot(scope.local_count);
        scope.local_count += 1;
        scope.bindings.push((symbol, Access::Local(slot), identity));
        slot
    }
    fn resolve(&mut self, depth: usize, symbol: SymbolId) -> Option<Access> {
        if let Some((_, access, _)) = self.scopes[depth]
            .bindings
            .iter()
            .rev()
            .find(|(id, _, _)| *id == symbol)
        {
            return Some(*access);
        }
        if depth == 0 {
            return None;
        }
        let source = self.resolve(depth - 1, symbol)?;
        let captures = &mut self.scopes[depth].captures;
        let index = captures
            .iter()
            .position(|a| *a == source)
            .unwrap_or_else(|| {
                captures.push(source);
                captures.len() - 1
            });
        Some(Access::Capture(index))
    }

    fn push(&mut self, form: &Form, expr: Expr) -> ExprId {
        let id = ExprId(self.expressions.len());
        self.expressions.push(expr);
        self.spans.push(form.span());
        self.origins.push(form.node().origin.clone());
        id
    }
    fn constant(&mut self, form: &Form, value: RootedValue) -> ExprId {
        let constant = ConstantId(self.constants.len());
        self.constants.push(value);
        self.push(form, Expr::Constant(constant))
    }
    fn analyze(&mut self, form: &Form, recur: RecurContext) -> Result<ExprId, EvalError> {
        let width = match &form.node().kind {
            FormKind::Scalar => 0,
            FormKind::Collection { children, .. } => children.len(),
        };
        self.context
            .poll_execution(PollPoint::Analysis)
            .and_then(|()| {
                self.context
                    .charge_allocation(width.saturating_add(1) as u64)
                    .map_err(|e| self.context.accounting.kind(e))
            })
            .map_err(|kind| analysis_error(form, kind))?;
        if self.depth >= self.context.accounting.depths().1 {
            return Err(analysis_error(
                form,
                self.context.accounting.kind(
                    self.context
                        .accounting
                        .fail(ResourceError::EvaluationDepthExceeded),
                ),
            ));
        }
        self.depth += 1;
        let mut result = self.analyze_inner(form, recur);
        self.depth -= 1;
        if let Err(e) = self.context.accounting.check() {
            match &mut result {
                Err(error) => error.kind = self.context.accounting.kind(e),
                Ok(_) => return Err(analysis_error(form, self.context.accounting.kind(e))),
            }
        }
        result
    }
    fn analyze_inner(&mut self, form: &Form, recur: RecurContext) -> Result<ExprId, EvalError> {
        match &form.node().kind {
            FormKind::Scalar => {
                let value = form.data();
                if let ValueView::Symbol(name) = value.inspect() {
                    let symbol = value.symbol_id().expect("symbol ID");
                    if let Some(access) = self.resolve(self.scopes.len() - 1, symbol) {
                        Ok(self.push(form, Expr::Local(access)))
                    } else if let Some(var) = self
                        .context
                        .resolve_var(&name)
                        .map_err(|e| analysis_error(form, EvalErrorKind::Namespace(e)))?
                    {
                        let id = var.var_id().unwrap();
                        if self.context.is_macro_var(id) {
                            return Err(analysis_error(
                                form,
                                EvalErrorKind::InvalidForm("cannot take the value of a macro"),
                            ));
                        }
                        self.vars.push(var);
                        Ok(self.push(form, Expr::Var(id)))
                    } else {
                        Err(analysis_error(form, EvalErrorKind::UnresolvedSymbol(name)))
                    }
                } else {
                    Ok(self.constant(form, value))
                }
            }
            FormKind::Collection {
                kind: CollectionKind::List,
                children,
            } if !children.is_empty() => self.list(form, &form.children(), recur),
            // SCI treats the empty list as a constant, including its metadata.
            FormKind::Collection {
                kind: CollectionKind::List,
                ..
            } => Ok(self.constant(form, form.data())),
            FormKind::Collection { kind, .. } => {
                let kind = *kind;
                // SCI analyzes vector/set metadata first, map metadata last;
                // execution always constructs the members before metadata.
                let mut metadata = None;
                if kind != CollectionKind::Map {
                    metadata = form
                        .metadata()
                        .map(|m| self.analyze(&m, recur.non_tail()))
                        .transpose()?;
                }
                let children = form
                    .children()
                    .iter()
                    .map(|f| self.analyze(f, recur.non_tail()))
                    .collect::<Result<_, _>>()?;
                if kind == CollectionKind::Map {
                    metadata = form
                        .metadata()
                        .map(|m| self.analyze(&m, recur.non_tail()))
                        .transpose()?;
                }
                Ok(self.push(
                    form,
                    Expr::Collection {
                        kind,
                        children,
                        metadata,
                    },
                ))
            }
        }
    }
    fn body(
        &mut self,
        form: &Form,
        children: &[Form],
        recur: RecurContext,
    ) -> Result<ExprId, EvalError> {
        let children = children
            .iter()
            .enumerate()
            .map(|(i, f)| {
                self.analyze(
                    f,
                    if i + 1 == children.len() {
                        recur
                    } else {
                        recur.non_tail()
                    },
                )
            })
            .collect::<Result<_, _>>()?;
        Ok(self.push(form, Expr::Do(children)))
    }
    fn list(
        &mut self,
        form: &Form,
        children: &[Form],
        recur: RecurContext,
    ) -> Result<ExprId, EvalError> {
        let operator = &children[0];
        if let ValueView::Symbol(name) = operator.data().inspect() {
            self.context
                .check_primitive(&name)
                .map_err(|kind| analysis_error(operator, kind))?;
            if macros::primitive(&name) || name == Name::new(None, "fn*") {
                self.permissions.push(name);
            }
        }
        let local_operator = operator
            .data()
            .symbol_id()
            .and_then(|symbol| self.resolve(self.scopes.len() - 1, symbol))
            .is_some();
        if !local_operator
            && self
                .context
                .macro_candidate(&form.data(), &self.context.nil())
                .map_err(|kind| analysis_error(form, kind))?
                .is_some()
        {
            let env = self.lexical_env(form)?;
            let expanded = self.context.expand_form(form, &env)?;
            return self.analyze(&expanded, recur);
        }
        let defonce = self.builtin_macro(operator, "defonce");
        if defonce {
            self.check_synthetic(operator, "defonce")?;
        }
        if is_named(operator, "def") || defonce {
            return self.definition(form, children, recur, false, defonce);
        }
        if self.builtin_macro(operator, "declare") {
            self.check_synthetic(operator, "declare")?;
            let mut defs = Vec::new();
            for name in &children[1..] {
                defs.push(self.definition(
                    form,
                    &[operator.clone(), name.clone()],
                    recur,
                    true,
                    false,
                )?);
            }
            if defs.is_empty() {
                defs.push(self.constant(form, self.context.nil()));
            }
            return Ok(self.push(form, Expr::Do(defs.into_boxed_slice())));
        }
        if is_named(operator, "set!") {
            let Some(target) = children.get(1) else {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("set! requires a Var symbol"),
                ));
            };
            let ValueView::Symbol(name) = target.data().inspect() else {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("set! requires a Var symbol"),
                ));
            };
            let local = target
                .data()
                .symbol_id()
                .is_some_and(|symbol| self.resolve(self.scopes.len() - 1, symbol).is_some());
            let var = if local {
                None
            } else {
                Some(
                    self.context
                        .resolve_var(&name)
                        .map_err(|e| analysis_error(form, EvalErrorKind::Namespace(e)))?
                        .ok_or_else(|| {
                            analysis_error(target, EvalErrorKind::UnresolvedSymbol(name))
                        })?,
                )
            };
            let value = if let Some(value) = children.get(2) {
                self.analyze(value, recur.non_tail())?
            } else {
                self.constant(form, self.context.nil())
            };
            let Some(var) = var else {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("set! cannot assign a lexical local"),
                ));
            };
            let id = var.var_id().unwrap();
            self.vars.push(var);
            return Ok(self.push(form, Expr::SetVar { var: id, value }));
        }
        if is_named(operator, "var") {
            if children.len() != 2 {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("var requires one symbol"),
                ));
            }
            let ValueView::Symbol(name) = children[1].data().inspect() else {
                return Err(analysis_error(
                    &children[1],
                    EvalErrorKind::InvalidForm("var requires a symbol"),
                ));
            };
            let var = self
                .context
                .resolve_var(&name)
                .map_err(|e| analysis_error(form, EvalErrorKind::Namespace(e)))?
                .ok_or_else(|| {
                    analysis_error(&children[1], EvalErrorKind::UnresolvedSymbol(name))
                })?;
            self.vars.push(var.clone());
            return Ok(self.constant(form, var));
        }
        if is_named(operator, "quote") {
            if children.len() != 2 {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("quote requires one argument"),
                ));
            }
            return Ok(self.constant(form, children[1].data()));
        }
        if is_named(operator, "if") {
            // Validate every child first, even with a constant test or bad arity.
            let args = children[1..]
                .iter()
                .enumerate()
                .map(|(i, f)| self.analyze(f, if i == 0 { recur.non_tail() } else { recur }))
                .collect::<Result<Vec<_>, _>>()?;
            if !(2..=3).contains(&args.len()) {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("if requires two or three arguments"),
                ));
            }
            let no = args
                .get(2)
                .copied()
                .unwrap_or_else(|| self.constant(form, self.context.nil()));
            return Ok(self.push(
                form,
                Expr::If {
                    test: args[0],
                    yes: args[1],
                    no,
                },
            ));
        }
        if is_named(operator, "do") {
            return self.body(form, &children[1..], recur);
        }
        if is_named(operator, "case*") {
            if !(3..=4).contains(&children.len())
                || !matches!(children[2].data().inspect(), ValueView::Vector(_))
            {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm(
                        "case* requires input, constant/branch vector and optional default",
                    ),
                ));
            }
            let pairs = children[2].children();
            if !pairs.len().is_multiple_of(2) {
                return Err(analysis_error(
                    &children[2],
                    EvalErrorKind::InvalidForm("case* requires constant/branch pairs"),
                ));
            }
            let value = self.analyze(&children[1], recur.non_tail())?;
            // SCI analyzes the default before result branches. Group results
            // remain independent analyses, including their macro effects.
            let default = children
                .get(3)
                .map(|f| self.analyze(f, recur))
                .transpose()?;
            let mut table = self
                .context
                .map(&[])
                .map_err(|e| analysis_error(form, EvalErrorKind::Collection(e)))?;
            let mut branches = Vec::new();
            for pair in pairs.as_chunks::<2>().0 {
                let constant = pair[0].data();
                if table
                    .get(&constant)
                    .map_err(|e| analysis_error(&pair[0], EvalErrorKind::Collection(e)))?
                    .is_some()
                {
                    return Err(analysis_error(
                        &pair[0],
                        EvalErrorKind::InvalidForm("duplicate case* constant"),
                    ));
                }
                table = self
                    .context
                    .assoc(
                        &table,
                        &constant,
                        &self.context.integer(branches.len() as i64),
                    )
                    .map_err(|e| analysis_error(&pair[0], EvalErrorKind::Collection(e)))?;
                branches.push(self.analyze(&pair[1], recur)?);
            }
            let index = ConstantId(self.constants.len());
            self.constants.push(table);
            return Ok(self.push(
                form,
                Expr::Case {
                    value,
                    table: index,
                    branches: branches.into_boxed_slice(),
                    default,
                },
            ));
        }
        if is_named(operator, "let*") || is_named(operator, "loop*") {
            return self.let_form(form, children, recur, is_named(operator, "loop*"));
        }
        if is_named(operator, "recur") {
            // Like SCI, analyze operands before reporting target/arity errors.
            let arguments: Box<[_]> = children[1..]
                .iter()
                .map(|f| self.analyze(f, recur.non_tail()))
                .collect::<Result<_, _>>()?;
            let Some((target, arity, try_depth)) = recur.target else {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("recur has no lexical target"),
                ));
            };
            if recur.try_depth != try_depth {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("cannot recur across try"),
                ));
            }
            if !recur.tail {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("recur requires tail position"),
                ));
            }
            if arguments.len() != arity {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("recur argument count does not match target"),
                ));
            }
            return Ok(self.push(form, Expr::Recur { target, arguments }));
        }
        if is_named(operator, "fn*") && !local_operator {
            let function = self.function(form, children)?;
            return Ok(self.push(form, Expr::Function(function)));
        }
        if is_named(operator, "letfn*") {
            return self.letfn_form(form, children, recur);
        }
        if is_named(operator, "throw") {
            if children.len() != 2 {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("throw requires one argument"),
                ));
            }
            let value = self.analyze(&children[1], recur.non_tail())?;
            return Ok(self.push(form, Expr::Throw(value)));
        }
        if is_named(operator, "try") {
            return self.try_form(form, children, recur);
        }
        if is_named(operator, "set!") {
            return Err(analysis_error(
                operator,
                EvalErrorKind::UnsupportedForm("special form or macro is not implemented"),
            ));
        }
        let callee = self.analyze(operator, recur.non_tail())?;
        let arguments = children[1..]
            .iter()
            .map(|child| self.analyze(child, recur.non_tail()))
            .collect::<Result<_, _>>()?;
        if matches!(operator.data().inspect(), ValueView::Keyword(_))
            && !(2..=3).contains(&children.len())
        {
            return Err(analysis_error(
                form,
                EvalErrorKind::InvalidForm("keyword invocation requires one or two arguments"),
            ));
        }
        Ok(self.push(form, Expr::Call { callee, arguments }))
    }
    fn try_form(
        &mut self,
        form: &Form,
        children: &[Form],
        recur: RecurContext,
    ) -> Result<ExprId, EvalError> {
        let recur = RecurContext {
            try_depth: recur.try_depth + 1,
            ..recur
        };
        let mut body = Vec::new();
        let mut catches = Vec::new();
        let mut final_form = None;
        for (i, child) in children[1..].iter().enumerate() {
            let parts = child.children();
            let list = matches!(
                child.node().kind,
                FormKind::Collection {
                    kind: CollectionKind::List,
                    ..
                }
            );
            if list && parts.first().is_some_and(|f| is_named(f, "catch")) {
                catches.push(parts);
            } else if list
                && parts.first().is_some_and(|f| is_named(f, "finally"))
                && i + 2 == children.len()
            {
                final_form = Some((child, parts));
            } else {
                body.push(child.clone());
            }
        }
        let body = self.body(form, &body, recur)?;
        let mut analyzed = Vec::new();
        for parts in catches {
            let invalid = || {
                analysis_error(
                    form,
                    EvalErrorKind::InvalidForm(
                        "catch requires a portable exception category and symbol",
                    ),
                )
            };
            let category = match parts.get(1).map(|f| f.data().inspect()) {
                Some(ValueView::Symbol(name)) => {
                    ExceptionCategory::named(&name).ok_or_else(invalid)?
                }
                _ => return Err(invalid()),
            };
            let binding = parts.get(2).ok_or_else(invalid)?.data();
            if !matches!(binding.inspect(), ValueView::Symbol(ref n) if n.namespace().is_none()) {
                return Err(invalid());
            }
            let outer = self.scope().bindings.len();
            let slot = self.bind(binding.symbol_id().unwrap());
            let body = self.body(form, &parts[3..], recur)?;
            self.scope().bindings.truncate(outer);
            analyzed.push(Catch {
                category,
                slot,
                body,
            });
        }
        let finally = final_form
            .map(|(f, parts)| self.body(f, &parts[1..], recur))
            .transpose()?;
        Ok(self.push(
            form,
            Expr::Try {
                body,
                catches: analyzed.into_boxed_slice(),
                finally,
            },
        ))
    }
    fn lexical_env(&self, form: &Form) -> Result<RootedValue, EvalError> {
        let mut env = self
            .context
            .map(&[])
            .map_err(|e| analysis_error(form, EvalErrorKind::Collection(e)))?;
        for scope in &self.scopes {
            for (symbol, _, identity) in &scope.bindings {
                let key = self
                    .context
                    .symbol_value(*symbol)
                    .expect("rooted local symbol");
                let value = self.context.symbol(Name::new(
                    None,
                    &format!("local__{}_{}", symbol.0.owner, identity.0),
                ));
                env = self
                    .context
                    .assoc(&env, &key, &value)
                    .map_err(|e| analysis_error(form, EvalErrorKind::Collection(e)))?;
            }
        }
        Ok(env)
    }
    fn check_synthetic(&mut self, form: &Form, name: &str) -> Result<(), EvalError> {
        for name in [name, "def"] {
            let name = Name::new(None, name);
            self.context
                .check_primitive(&name)
                .map_err(|kind| analysis_error(form, kind))?;
            self.permissions.push(name);
        }
        Ok(())
    }
    fn builtin_macro(&mut self, form: &Form, name: &str) -> bool {
        if !is_core_named(form, name) {
            return false;
        }
        let data = form.data();
        if self
            .resolve(self.scopes.len() - 1, data.symbol_id().unwrap())
            .is_some()
        {
            return false;
        }
        let ValueView::Symbol(name) = data.inspect() else {
            unreachable!()
        };
        self.context
            .resolve_var(&name)
            .expect("current namespace")
            .is_none()
    }
    fn definition(
        &mut self,
        form: &Form,
        children: &[Form],
        recur: RecurContext,
        declared: bool,
        once: bool,
    ) -> Result<ExprId, EvalError> {
        // Interning/initial metadata and dynamic *file* are analysis effects.
        self.cacheable = false;
        let invalid = || {
            analysis_error(
                form,
                EvalErrorKind::InvalidForm(
                    "def requires a symbol and optional initializer/docstring",
                ),
            )
        };
        let name_form = children.get(1).ok_or_else(invalid)?;
        let ValueView::Symbol(name) = name_form.data().inspect() else {
            return Err(invalid());
        };
        let ns = self.context.current_namespace();
        let ValueView::Namespace(ns_name) = ns.inspect() else {
            unreachable!()
        };
        if name.namespace().is_some_and(|n| n != ns_name.name()) {
            return Err(analysis_error(
                name_form,
                EvalErrorKind::InvalidForm("def cannot create a Var outside the current namespace"),
            ));
        }
        if let Some(existing) = self.context.lookup_mapping(
            ns.namespace_id().unwrap(),
            &Name::from_utf16(None, name.name()),
        ) && !self
            .context
            .owner
            .store
            .borrow()
            .var_belongs_to(existing, ns.namespace_id().unwrap())
        {
            return Err(analysis_error(
                name_form,
                EvalErrorKind::Namespace(NamespaceError::ReferredVar(name.clone())),
            ));
        }
        if ns.namespace_id() == Some(self.context.registry().borrow().core) {
            // Definitions must observe read-only builtin identity even when its
            // root has not yet been requested through ordinary symbol lookup.
            self.context.core_var(&Name::from_utf16(None, name.name()));
        }
        let rooted_var = self
            .context
            .intern_language_var(
                ns.namespace_id().unwrap(),
                Name::from_utf16(None, name.name()),
            )
            .map_err(|e| analysis_error(form, EvalErrorKind::Namespace(e)))?;
        let var = rooted_var.var_id().unwrap();
        self.context
            .check_var_access(var)
            .map_err(|kind| analysis_error(name_form, kind))?;
        self.vars.push(rooted_var);
        let allocation_error = |e| analysis_error(form, EvalErrorKind::Collection(e));
        let mut base = self
            .context
            .var_base_metadata(var)
            .map_err(allocation_error)?;

        let file = if form.span().source().name() == "<eval>" {
            self.context
                .lookup_mapping(
                    self.context.registry().borrow().core,
                    &Name::new(None, "*file*"),
                )
                .map_or_else(
                    || self.context.nil(),
                    |var| self.context.var_get(var).expect("file value"),
                )
        } else {
            self.context.string(
                form.span()
                    .source()
                    .name()
                    .encode_utf16()
                    .collect::<Vec<_>>(),
            )
        };
        base = self
            .context
            .assoc(&base, &self.context.keyword(Name::new(None, "file")), &file)
            .map_err(allocation_error)?;
        if self.context.var_value(var).unwrap().metadata().is_none() {
            self.context.set_var_metadata(var, &base);
        }
        let has_doc =
            children.len() == 4 && matches!(children[2].data().inspect(), ValueView::String(_));
        let init = match children.len() {
            2 if !once => None,
            3 => Some(&children[2]),
            4 if !once && matches!(children[2].data().inspect(), ValueView::String(_)) => {
                base = self
                    .context
                    .assoc(
                        &base,
                        &self.context.keyword(Name::new(None, "doc")),
                        &children[2].data(),
                    )
                    .map_err(allocation_error)?;
                Some(&children[3])
            }
            _ => return Err(invalid()),
        };
        let init = init
            .map(|f| self.analyze(f, recur.non_tail()))
            .transpose()?;
        // SCI strips the metadata from a qualified same-namespace def symbol.
        let metadata = if name.namespace().is_none() {
            if let Some(meta) = name_form.metadata() {
                let mut children = Vec::new();
                for pair in meta.children().as_chunks::<2>().0 {
                    // These entries are replaced by SCI before analysis, so
                    // overwritten metadata expressions must not run.
                    if matches!(pair[0].data().inspect(), ValueView::Keyword(n) if ["ns", "line", "column"].iter().any(|k| n == Name::new(None, k)) || (has_doc && n == Name::new(None, "doc")) || (declared && n == Name::new(None, "declared")))
                    {
                        continue;
                    }
                    children.push(self.analyze(&pair[0], recur.non_tail())?);
                    children.push(self.analyze(&pair[1], recur.non_tail())?);
                }
                Some(self.push(
                    &meta,
                    Expr::Collection {
                        kind: CollectionKind::Map,
                        children: children.into_boxed_slice(),
                        metadata: None,
                    },
                ))
            } else {
                None
            }
        } else {
            None
        };
        let (line, column) = form
            .span()
            .source()
            .line_column(form.span().bytes().start)
            .unwrap();
        base = self
            .context
            .assoc(
                &base,
                &self.context.keyword(Name::new(None, "line")),
                &self.context.integer(line as i64),
            )
            .map_err(allocation_error)?;
        base = self
            .context
            .assoc(
                &base,
                &self.context.keyword(Name::new(None, "column")),
                &self.context.integer(column as i64),
            )
            .map_err(allocation_error)?;
        if declared {
            base = self
                .context
                .assoc(
                    &base,
                    &self.context.keyword(Name::new(None, "declared")),
                    &self.context.boolean(true),
                )
                .map_err(allocation_error)?;
        }
        let base_id = ConstantId(self.constants.len());
        self.constants.push(base);
        Ok(self.push(
            form,
            Expr::Def {
                var,
                init,
                metadata,
                base: base_id,
                once,
            },
        ))
    }
    fn function(&mut self, form: &Form, children: &[Form]) -> Result<FunctionId, EvalError> {
        if form
            .metadata()
            .is_some_and(|metadata| metadata.data().count() != Some(0))
        {
            return Err(analysis_error(
                form,
                EvalErrorKind::UnsupportedForm("function metadata is not implemented"),
            ));
        }
        let mut start = 1;
        let name = children.get(start).and_then(|f| f.data().symbol_id());
        if name.is_some() {
            start += 1;
        }
        let body = &children[start..];
        let Some(first) = body.first() else {
            return Err(analysis_error(
                form,
                EvalErrorKind::InvalidForm("fn* requires a parameter declaration"),
            ));
        };
        let bodies = if matches!(
            first.node().kind,
            FormKind::Collection {
                kind: CollectionKind::List,
                ..
            }
        ) {
            let mut bodies = Vec::new();
            for method in body {
                if !matches!(
                    method.node().kind,
                    FormKind::Collection {
                        kind: CollectionKind::List,
                        ..
                    }
                ) {
                    return Err(analysis_error(
                        method,
                        EvalErrorKind::InvalidForm("fn* requires method lists"),
                    ));
                }
                bodies.push(method.children());
            }
            bodies
        } else {
            vec![body.to_vec()]
        };
        self.scopes.push(Scope::default());
        let mut methods = Vec::new();
        let mut preceding_variadic = None;
        let self_binding = name.map(|name| (name, self.context.macros.binding_identity()));
        for body in bodies {
            self.scope().bindings.clear();
            self.scope().local_count = 0;
            if let Some((name, identity)) = self_binding {
                self.scope()
                    .bindings
                    .push((name, Access::SelfValue, identity));
            }
            let Some(params) = body.first() else {
                return Err(analysis_error(
                    form,
                    EvalErrorKind::InvalidForm("fn* requires a parameter declaration"),
                ));
            };
            if !matches!(
                params.node().kind,
                FormKind::Collection {
                    kind: CollectionKind::Vector,
                    ..
                }
            ) {
                return Err(analysis_error(
                    params,
                    EvalErrorKind::InvalidForm("fn* parameters must be a vector"),
                ));
            }
            let params = params.children();
            let fixed = params
                .iter()
                .position(|f| is_named(f, "&"))
                .unwrap_or(params.len());
            let rest = params.get(fixed + 1);
            let variadic = rest.is_some();
            for param in params[..fixed].iter().chain(rest) {
                let Some(symbol) = param.data().symbol_id() else {
                    return Err(analysis_error(
                        param,
                        EvalErrorKind::InvalidForm(
                            "fn* parameter must be a symbol; destructuring requires fn",
                        ),
                    ));
                };
                self.bind(symbol);
            }
            // Analyze before signature checks, matching the pinned SCI order.
            let target = self.target((0..fixed + usize::from(variadic)).map(LocalSlot).collect());
            let expr = self.body(
                form,
                &body[1..],
                RecurContext::default().with_target(target.id, target.slots.len()),
            )?;
            if let Some(min) = preceding_variadic {
                if variadic {
                    return Err(analysis_error(
                        form,
                        EvalErrorKind::InvalidForm("more than one consecutive variadic overload"),
                    ));
                }
                if fixed > min {
                    return Err(analysis_error(
                        form,
                        EvalErrorKind::InvalidForm(
                            "fixed arity exceeds preceding variadic minimum",
                        ),
                    ));
                }
            }
            preceding_variadic = variadic.then_some(fixed);
            methods.push(Method {
                fixed,
                variadic,
                body: expr,
                local_count: self.scope().local_count,
                target,
            });
        }
        let scope = self.scopes.pop().unwrap();
        let id = FunctionId(self.functions.len());
        self.functions.push(FunctionCode {
            captures: scope.captures.into_boxed_slice(),
            methods: methods.into_boxed_slice(),
        });
        Ok(id)
    }

    fn letfn_form(
        &mut self,
        form: &Form,
        children: &[Form],
        recur: RecurContext,
    ) -> Result<ExprId, EvalError> {
        let Some(binding_form) = children.get(1) else {
            return Err(analysis_error(
                form,
                EvalErrorKind::InvalidForm("letfn* requires a binding vector"),
            ));
        };
        if !matches!(
            binding_form.node().kind,
            FormKind::Collection {
                kind: CollectionKind::Vector,
                ..
            }
        ) {
            return Err(analysis_error(
                binding_form,
                EvalErrorKind::InvalidForm("letfn* requires a binding vector"),
            ));
        }
        let pairs = binding_form.children();
        if !pairs.len().is_multiple_of(2) {
            return Err(analysis_error(
                binding_form,
                EvalErrorKind::InvalidForm("letfn* requires binding/function pairs"),
            ));
        }
        let outer = self.scope().bindings.len();
        let mut slots = Vec::new();
        for pair in pairs.as_chunks::<2>().0 {
            let value = pair[0].data();
            if !matches!(value.inspect(), ValueView::Symbol(ref n) if n.namespace().is_none() && !n.name().is_empty())
            {
                return Err(analysis_error(
                    &pair[0],
                    EvalErrorKind::InvalidForm("letfn* binding must be an unqualified symbol"),
                ));
            }
            slots.push(self.bind(value.symbol_id().unwrap()));
        }
        let mut bindings = Vec::new();
        for (pair, slot) in pairs.as_chunks::<2>().0.iter().zip(slots) {
            let parts = pair[1].children();
            if !matches!(
                pair[1].node().kind,
                FormKind::Collection {
                    kind: CollectionKind::List,
                    ..
                }
            ) || !parts.first().is_some_and(|f| is_named(f, "fn*"))
            {
                return Err(analysis_error(
                    &pair[1],
                    EvalErrorKind::InvalidForm("letfn* initializer must be fn*"),
                ));
            }
            bindings.push((slot, self.function(&pair[1], &parts)?));
        }
        let body = self.body(form, &children[2..], recur)?;
        self.scope().bindings.truncate(outer);
        Ok(self.push(
            form,
            Expr::LetFn {
                bindings: bindings.into_boxed_slice(),
                body,
            },
        ))
    }

    fn let_form(
        &mut self,
        form: &Form,
        children: &[Form],
        recur: RecurContext,
        is_loop: bool,
    ) -> Result<ExprId, EvalError> {
        let Some(binding_form) = children.get(1) else {
            return Err(analysis_error(
                form,
                EvalErrorKind::InvalidForm(if is_loop {
                    "loop* requires a binding vector"
                } else {
                    "let* requires a binding vector"
                }),
            ));
        };
        if !matches!(
            binding_form.node().kind,
            FormKind::Collection {
                kind: CollectionKind::Vector,
                ..
            }
        ) {
            return Err(analysis_error(
                binding_form,
                EvalErrorKind::InvalidForm(if is_loop {
                    "loop* requires a binding vector"
                } else {
                    "let* requires a binding vector"
                }),
            ));
        }
        let pairs = binding_form.children();
        if !pairs.len().is_multiple_of(2) {
            return Err(analysis_error(
                binding_form,
                EvalErrorKind::InvalidForm(if is_loop {
                    "loop* requires binding/initializer pairs"
                } else {
                    "let* requires binding/initializer pairs"
                }),
            ));
        }
        let outer = self.scope().bindings.len();
        let mut bindings = Vec::with_capacity(pairs.len() / 2);
        for pair in pairs.as_chunks::<2>().0 {
            let value = pair[0].data();
            if !matches!(value.inspect(), ValueView::Symbol(ref n) if n.namespace().is_none() && !n.name().is_empty())
            {
                return Err(analysis_error(
                    &pair[0],
                    EvalErrorKind::InvalidForm(if is_loop {
                        "loop* binding must be an unqualified symbol"
                    } else {
                        "let* binding must be an unqualified symbol"
                    }),
                ));
            }
            let init = self.analyze(&pair[1], recur.non_tail())?;
            let slot = self.bind(value.symbol_id().expect("binding symbol"));
            bindings.push((slot, init));
        }
        let target = is_loop.then(|| self.target(bindings.iter().map(|(slot, _)| *slot).collect()));
        let body_context = target.as_ref().map_or(recur, |target| {
            recur.with_target(target.id, target.slots.len())
        });
        let body = self.body(form, &children[2..], body_context)?;
        self.scope().bindings.truncate(outer);
        let bindings = bindings.into_boxed_slice();
        Ok(self.push(
            form,
            match target {
                Some(target) => Expr::Loop {
                    bindings,
                    body,
                    target,
                },
                None => Expr::Let { bindings, body },
            },
        ))
    }
}

fn invoke(
    context: &mut Context,
    callee: &RootedValue,
    args_base: usize,
    scratch: &mut Scratch,
    applied: Option<&RootedValue>,
) -> Result<RootedValue, InvokeError> {
    let dispatch = |kind| InvokeError::Dispatch(kind);
    let mut target = callee.clone();
    let mut seen = std::collections::HashSet::new();
    let value = loop {
        let value = target.owner.store.borrow().unannotated(target.value());
        if let Value::Var(id) = value {
            context.check_var_access(id).map_err(dispatch)?;
            if !seen.insert(id) {
                return Err(dispatch(EvalErrorKind::NotCallable));
            }
            target = context
                .var_get(id)
                .map_err(|_| dispatch(EvalErrorKind::WrongOwner))?;
        } else {
            break value;
        }
    };
    let callee = &target;
    if let Value::Native(definition) = value {
        context.check_native(definition).map_err(dispatch)?;
        if let Some(arguments) = applied {
            if let Some(result) = sequence_core::apply_constructor(context, definition, arguments) {
                return result.map_err(|error| match error {
                    NativeError::Invocation(error) => error.into_error(),
                    error => dispatch(EvalErrorKind::Native {
                        name: Box::new(definition.metadata.qualified_name()),
                        error,
                    }),
                });
            }
            sequence_core::force_arguments(context, arguments, &mut scratch.values)?;
        }
        return context
            .invoke_native(definition, &scratch.values[args_base..])
            .map_err(|error| match error {
                NativeError::Invocation(error) => error.into_error(),
                error => dispatch(EvalErrorKind::Native {
                    name: Box::new(definition.metadata.qualified_name()),
                    error,
                }),
            });
    }
    if let Some(result) =
        protocols::invoke_method(context, callee, &scratch.values[args_base..], applied)
    {
        return result;
    }
    if let Some(result) =
        sequence_core::invoke_higher_order(context, callee, &scratch.values[args_base..], applied)
    {
        return result;
    }
    if let Some(source) = applied
        && !matches!(callee.inspect(), ValueView::Function)
    {
        sequence_core::force_arguments(context, source, &mut scratch.values)?;
    }
    if let Some(result) =
        collection_core::invoke_data(context, callee, &scratch.values[args_base..])
    {
        return result;
    }
    let (unit, function) = match callee.owner.store.borrow().heap_value(value) {
        Some(HeapValue::Closure(closure)) => (closure.unit, closure.function),
        _ => return Err(dispatch(EvalErrorKind::NotCallable)),
    };
    let unit = callee.owner.unit(unit).expect("rooted closure code");
    for name in &unit.code.permissions {
        context.check_primitive(name).map_err(dispatch)?;
    }
    let methods = &unit.code.functions[function.0].methods;
    let actual = if let Some(arguments) = applied {
        if let Some(count) = arguments.count() {
            count
        } else {
            let bound = methods
                .iter()
                .map(|m| m.fixed)
                .max()
                .unwrap_or(0)
                .saturating_add(1);
            let mut cursor = collection_core::Cursor::new(context, arguments, 0)
                .map_err(deferred::invoke_error)?;
            let mut count = 0;
            while count < bound
                && cursor
                    .next(context)
                    .map_err(deferred::invoke_error)?
                    .is_some()
            {
                count += 1;
            }
            count
        }
    } else {
        scratch.values.len() - args_base
    };
    // Pinned SCI uses the last fixed overload and the first variadic overload.
    let method = methods
        .iter()
        .rev()
        .find(|m| !m.variadic && m.fixed == actual)
        .or_else(|| {
            methods
                .iter()
                .find(|m| m.variadic)
                .filter(|m| actual >= m.fixed)
        });
    let mut applied_tail = None;
    if let Some(arguments) = applied {
        if let Some(method) = method.filter(|method| method.variadic) {
            sequence_core::applied_prefix(context, arguments, method.fixed)?;
        }
        // SCI's multi-method dispatcher demands a four-argument entry prefix
        // before it delegates to the selected body's own variadic signature.
        if methods.len() > 1 {
            sequence_core::applied_prefix(context, arguments, 4)?;
            let mut tail = arguments.clone();
            for _ in 0..actual.min(4) {
                context
                    .poll_execution(PollPoint::Iteration)
                    .map_err(dispatch)?;
                tail =
                    collection_core::sequence(context, &tail, 0).map_err(deferred::invoke_error)?;
                tail.nth(0)
                    .map_err(|e| dispatch(EvalErrorKind::Collection(e)))?;
                tail = context.sequence_tail(&tail)?;
            }
        }
        let fixed = method.filter(|m| m.variadic).map_or(actual, |m| m.fixed);
        let mut tail = arguments.clone();
        for _ in 0..fixed {
            context
                .poll_execution(PollPoint::Iteration)
                .map_err(dispatch)?;
            tail = collection_core::sequence(context, &tail, 0).map_err(deferred::invoke_error)?;
            let value = tail
                .nth(0)
                .map_err(|e| dispatch(EvalErrorKind::Collection(e)))?
                .expect("argument length");
            scratch.values.push(value);
            tail = context.sequence_tail(&tail)?;
        }
        applied_tail = Some(if tail.count() == Some(0) {
            context.nil()
        } else {
            tail
        });
    }
    let method = method.ok_or_else(|| {
        dispatch(EvalErrorKind::Arity {
            fixed: methods
                .iter()
                .filter(|m| !m.variadic)
                .map(|m| m.fixed)
                .collect(),
            variadic: methods.iter().find(|m| m.variadic).map(|m| m.fixed),
            actual,
        })
    })?;
    // Ordinary calls remain bounded; recur reuses this frame instead of calling.
    if context.active_calls >= context.accounting.depths().0 {
        return Err(dispatch(
            context
                .accounting
                .kind(context.accounting.fail(ResourceError::CallDepthExceeded)),
        ));
    }
    context
        .poll_execution(PollPoint::Function)
        .map_err(dispatch)?;
    let base = scratch.locals.len();
    scratch
        .locals
        .resize_with(base + method.local_count, || None);
    for i in 0..method.fixed {
        scratch.locals[base + i] = Some(scratch.values[args_base + i].clone());
    }
    if method.variadic {
        let extra = &scratch.values[args_base + method.fixed..];
        let rest = if let Some(rest) = applied_tail {
            rest
        } else if extra.is_empty() {
            context.nil()
        } else {
            context
                .charge_allocation(extra.len().saturating_add(1) as u64)
                .map_err(|e| dispatch(context.accounting.kind(e)))?;
            let values = extra.iter().map(RootedValue::value).collect();
            let id = HeapId(
                context
                    .owner
                    .store
                    .borrow_mut()
                    .heap
                    .insert(HeapValue::ArgumentSeq(values)),
            );
            context.owner.value(Value::Heap(id))
        };
        scratch.locals[base + method.fixed] = Some(rest);
    }
    context.active_calls += 1;
    let result = Evaluator {
        context,
        unit: &unit,
        scratch,
        base,
        callee: Some(callee),
    }
    .eval_target(method.body, &method.target);
    context.active_calls -= 1;
    scratch.locals.truncate(base);
    result.map_err(InvokeError::Execution)
}

enum Control {
    Value(RootedValue),
    // Replacement operands are rooted in reusable scratch until the target
    // consumes them. This signal cannot be confused with a language value.
    Recur {
        target: TargetId,
        values_base: usize,
    },
}

struct Evaluator<'a> {
    context: &'a mut Context,
    unit: &'a ExecutableUnit,
    scratch: &'a mut Scratch,
    base: usize,
    callee: Option<&'a RootedValue>,
}
impl Evaluator<'_> {
    fn access(&self, access: Access) -> RootedValue {
        match access {
            Access::Local(slot) => self.scratch.locals[self.base + slot.0]
                .as_ref()
                .expect("initialized local")
                .clone(),
            Access::SelfValue => self.callee.expect("function frame").clone(),
            Access::Capture(index) => {
                let callee = self.callee.expect("function frame");
                let value = {
                    let store = callee.owner.store.borrow();
                    let Some(HeapValue::Closure(closure)) = store.heap_value(callee.value()) else {
                        unreachable!()
                    };
                    closure.captures[index]
                };
                callee.owner.value(value)
            }
        }
    }
    fn closure(
        &self,
        function: FunctionId,
        captures: Box<[Value]>,
    ) -> Result<RootedValue, ResourceError> {
        self.context
            .charge_allocation(captures.len().saturating_add(1) as u64)?;
        let closure = Closure {
            unit: self.unit.id(),
            function,
            captures,
        };
        let id = HeapId(
            self.context
                .owner
                .store
                .borrow_mut()
                .heap
                .insert(HeapValue::Closure(closure)),
        );
        let value = self.context.owner.value(Value::Heap(id));
        self.context.accounting.check()?;
        Ok(value)
    }

    fn definition_metadata(
        &mut self,
        id: ExprId,
        base: ConstantId,
        expression: Option<ExprId>,
    ) -> Result<RootedValue, Box<EvalError>> {
        let mut metadata = self.unit.constant(base.0).expect("metadata constant");
        if let Some(expression) = expression {
            let supplied = self.eval_value(expression)?;
            for (key, value) in supplied
                .entries()
                .map_err(|e| self.error(id, EvalErrorKind::Collection(e)))?
            {
                metadata = self
                    .context
                    .assoc(&metadata, &key, &value)
                    .map_err(|e| self.error(id, EvalErrorKind::Collection(e)))?;
            }
        }
        let defaults = self.unit.constant(base.0).expect("metadata constant");
        for name in ["name", "file"] {
            let key = self.context.keyword(Name::new(None, name));
            metadata = self
                .context
                .assoc(
                    &metadata,
                    &key,
                    &defaults
                        .get(&key)
                        .map_err(|e| self.error(id, EvalErrorKind::Collection(e)))?
                        .unwrap(),
                )
                .map_err(|e| self.error(id, EvalErrorKind::Collection(e)))?;
        }
        Ok(metadata)
    }
    fn error(&self, id: ExprId, kind: EvalErrorKind) -> Box<EvalError> {
        Box::new(EvalError {
            phase: EvalPhase::Execution,
            kind,
            span: Box::new(self.unit.span(id.0).expect("expression span")),
            related: None,
            origin: self.unit.code.locations[id.0].origin.clone(),
            frames: Vec::new(),
            exception: None,
        })
    }
    fn eval_value(&mut self, id: ExprId) -> Result<RootedValue, Box<EvalError>> {
        match self.eval(id)? {
            Control::Value(value) => Ok(value),
            Control::Recur { .. } => unreachable!("analyzer excludes recur in value positions"),
        }
    }

    fn eval_target(
        &mut self,
        body: ExprId,
        target: &Target,
    ) -> Result<RootedValue, Box<EvalError>> {
        loop {
            match self.eval(body)? {
                Control::Value(value) => return Ok(value),
                Control::Recur {
                    target: reached,
                    values_base,
                } => {
                    assert_eq!(target.id, reached, "lexically resolved recur target");
                    // Every operand has completed against the old frame. Draining
                    // moves roots into target slots without allocating per hop.
                    for (slot, value) in target
                        .slots
                        .iter()
                        .zip(self.scratch.values.drain(values_base..))
                    {
                        self.scratch.locals[self.base + slot.0] = Some(value);
                    }
                }
            }
        }
    }

    fn eval(&mut self, id: ExprId) -> Result<Control, Box<EvalError>> {
        // Deep bodies amplify call recursion; bound expression frames as well.
        self.context
            .poll_execution(PollPoint::Expression)
            .map_err(|kind| self.error(id, kind))?;
        if self.context.active_expressions >= self.context.accounting.depths().1 {
            return Err(self.error(
                id,
                self.context.accounting.kind(
                    self.context
                        .accounting
                        .fail(ResourceError::EvaluationDepthExceeded),
                ),
            ));
        }
        self.context.active_expressions += 1;
        let values_base = self.scratch.values.len();
        let mut result = self.eval_inner(id);
        self.context.active_expressions -= 1;
        if let Err(error) = self.context.accounting.check()
            && !result
                .as_ref()
                .is_err_and(|e| e.class() != ErrorClass::Language)
        {
            result = Err(self.error(id, self.context.accounting.kind(error)));
        }
        if !matches!(result, Ok(Control::Recur { .. })) {
            self.scratch.values.truncate(values_base);
        }
        if let Err(error) = &mut result
            && matches!(self.unit.code.expressions[id.0], Expr::Call { .. })
        {
            error.frames.push(SourceFrame {
                span: self.unit.span(id.0).unwrap(),
                origin: self.unit.code.locations[id.0].origin.clone(),
            });
        }
        result
    }
    // Keep exception-only temporaries off every ordinary expression's Rust frame.
    fn eval_try(
        &mut self,
        body: ExprId,
        catches: &[Catch],
        finally: Option<ExprId>,
    ) -> Result<Control, Box<EvalError>> {
        let mut result = self.eval(body);
        if let Err(error) = &mut result
            && error.class() == ErrorClass::Language
        {
            let category = error
                .exception
                .as_ref()
                .map(|v| match v.inspect() {
                    ValueView::Exception(c) => c,
                    _ => unreachable!(),
                })
                .unwrap_or(ExceptionCategory::Exception);
            if let Some(catch) = catches.iter().find(|c| c.category.catches(category)) {
                let exception = if let Some(value) = &error.exception {
                    Ok((**value).clone())
                } else {
                    let message = self
                        .context
                        .string(error.kind.to_string().encode_utf16().collect::<Vec<_>>());
                    self.context.runtime_exception(
                        category,
                        &message,
                        &self.context.nil(),
                        &self.context.nil(),
                    )
                };
                match exception {
                    Ok(exception) => {
                        self.scratch.locals[self.base + catch.slot.0] = Some(exception);
                        result = self.eval(catch.body);
                        self.scratch.locals[self.base + catch.slot.0] = None;
                    }
                    Err(failure) => {
                        let kind = match failure {
                            NativeError::Invocation(e) => match e.into_error() {
                                InvokeError::Dispatch(kind) => kind,
                                InvokeError::Execution(e) => e.kind,
                            },
                            NativeError::Collection(error) => EvalErrorKind::Collection(error),
                            _ => unreachable!("validated exception fields"),
                        };
                        result = Err(self.error(body, kind));
                    }
                }
            }
        }
        if let Some(finally) = finally {
            let terminal = result
                .as_ref()
                .is_err_and(|e| e.class() != ErrorClass::Language);
            if terminal {
                self.context.accounting.begin_cleanup();
            }
            let cleanup = self.eval_value(finally);
            if terminal {
                self.context.accounting.end_cleanup();
            }
            if !terminal && let Err(error) = cleanup {
                result = Err(error);
            }
        }
        result
    }
    fn eval_collection(
        &mut self,
        id: ExprId,
        kind: CollectionKind,
        children: &[ExprId],
        metadata: Option<ExprId>,
    ) -> Result<RootedValue, Box<EvalError>> {
        let base = self.scratch.values.len();
        for child in children {
            let value = self.eval_value(*child)?;
            self.scratch.values.push(value);
        }
        if matches!(kind, CollectionKind::Map | CollectionKind::Set) {
            let step = if kind == CollectionKind::Map { 2 } else { 1 };
            for index in (base..self.scratch.values.len()).step_by(step) {
                let key = self.scratch.values[index].clone();
                self.context.realize_key(&key).map_err(|error| {
                    match deferred::invoke_error(error) {
                        InvokeError::Dispatch(kind) => self.error(children[index - base], kind),
                        InvokeError::Execution(error) => error,
                    }
                })?;
            }
        }
        let values = &self.scratch.values[base..];
        // Check after evaluating all members, before metadata, matching
        // the checked SCI collection constructors' argument evaluation.
        if matches!(kind, CollectionKind::Map | CollectionKind::Set) {
            let step = if kind == CollectionKind::Map { 2 } else { 1 };
            let mut seen: HashMap<u64, Vec<usize>> = HashMap::new();
            for index in (0..values.len()).step_by(step) {
                let hash = values[index].try_value_hash().map_err(|error| {
                    self.error(children[index], EvalErrorKind::Collection(error))
                })?;
                let bucket = seen.entry(hash).or_default();
                if let Some(&first) = bucket.iter().find(|&&i| values[index].value_eq(&values[i])) {
                    let kind = if kind == CollectionKind::Map {
                        EvalErrorKind::DuplicateMapKey
                    } else {
                        EvalErrorKind::DuplicateSetMember
                    };
                    let mut error = self.error(children[index], kind);
                    error.related = self.unit.span(children[first].0).map(Box::new);
                    return Err(error);
                }
                bucket.push(index);
            }
        }
        let value = match &kind {
            CollectionKind::Vector => self.context.vector(values),
            CollectionKind::Set => self.context.set(values),
            CollectionKind::Map => self.context.map(
                &values
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|p| (p[0].clone(), p[1].clone()))
                    .collect::<Vec<_>>(),
            ),
            CollectionKind::List | CollectionKind::AnonymousFn => {
                unreachable!("reader lists are constants or special forms")
            }
        }
        .map_err(|error| self.error(id, EvalErrorKind::Collection(error)))?;
        self.scratch.values.truncate(base);
        Ok(if let Some(metadata) = metadata {
            let metadata = self.eval_value(metadata)?;
            self.context
                .with_meta(&value, &metadata)
                .map_err(|error| self.error(id, EvalErrorKind::Collection(error)))?
        } else {
            value
        })
    }
    fn eval_inner(&mut self, id: ExprId) -> Result<Control, Box<EvalError>> {
        let result = match &self.unit.code.expressions[id.0] {
            Expr::Throw(value) => {
                let value = self.eval_value(*value)?;
                let ValueView::Exception(category) = value.inspect() else {
                    return Err(self.error(id, EvalErrorKind::InvalidThrow));
                };
                let mut error = self.error(id, EvalErrorKind::Thrown(category));
                error.exception = Some(Box::new(value));
                return Err(error);
            }
            Expr::Try {
                body,
                catches,
                finally,
            } => return self.eval_try(*body, catches, *finally),
            Expr::Constant(index) => {
                let value = self.unit.constant(index.0).expect("constant index");
                if let Some(var) = value.var_id() {
                    self.context
                        .check_var_access(var)
                        .map_err(|kind| self.error(id, kind))?;
                    self.context.var_value(var).expect("rooted Var")
                } else {
                    value
                }
            }
            Expr::Local(access) => self.access(*access),
            Expr::Var(var) => {
                self.context
                    .check_var_access(*var)
                    .map_err(|kind| self.error(id, kind))?;
                self.context.var_get(*var).expect("rooted Var")
            }
            Expr::SetVar { var, value } => {
                let value = self.eval_value(*value)?;
                self.context
                    .set_var_binding(*var, &value)
                    .map_err(|e| self.error(id, e.kind()))?;
                value
            }
            Expr::Def {
                var,
                init,
                metadata,
                base,
                once,
            } => {
                if *once {
                    let meta = self.definition_metadata(id, *base, *metadata)?;
                    let target = self
                        .context
                        .definition_target(*var)
                        .map_err(|e| self.error(id, EvalErrorKind::Namespace(e)))?;
                    self.context
                        .check_var_write(target)
                        .map_err(|e| self.error(id, e.kind()))?;
                    self.context.set_var_metadata(target, &meta);
                    if !matches!(
                        self.context.var_root(target).expect("rooted Var").inspect(),
                        ValueView::Unbound(_)
                    ) {
                        return Ok(Control::Value(self.context.nil()));
                    }
                }
                let value = init.map(|init| self.eval_value(init)).transpose()?;
                let meta = self.definition_metadata(id, *base, *metadata)?;
                let target = self
                    .context
                    .definition_target(*var)
                    .map_err(|e| self.error(id, EvalErrorKind::Namespace(e)))?;
                self.context
                    .check_var_write(target)
                    .map_err(|e| self.error(id, e.kind()))?;
                if let Some(value) = value
                    && !matches!(value.value(), Value::Unbound(_))
                {
                    self.context
                        .set_var_root_internal(target, &value)
                        .expect("same owner");
                }
                self.context.set_var_metadata(target, &meta);
                self.context.var_value(target).expect("rooted Var")
            }
            Expr::Function(function) => {
                let captures = self.unit.code.functions[function.0]
                    .captures
                    .iter()
                    .map(|access| self.access(*access).value())
                    .collect();
                self.closure(*function, captures)
                    .map_err(|e| self.error(id, self.context.accounting.kind(e)))?
            }
            Expr::LetFn { bindings, body } => {
                // Allocate all identities before filling capture edges. There
                // is no callback/evaluation between these two phases.
                for (slot, function) in bindings {
                    self.scratch.locals[self.base + slot.0] = Some(
                        self.closure(*function, Box::default())
                            .map_err(|e| self.error(id, self.context.accounting.kind(e)))?,
                    );
                }
                for (slot, function) in bindings {
                    let captures = self.unit.code.functions[function.0]
                        .captures
                        .iter()
                        .map(|access| self.access(*access).value())
                        .collect();
                    let id = self.scratch.locals[self.base + slot.0]
                        .as_ref()
                        .unwrap()
                        .heap_id()
                        .unwrap();
                    let mut store = self.context.owner.store.borrow_mut();
                    let HeapValue::Closure(closure) = store.heap.get_mut(id.0) else {
                        unreachable!()
                    };
                    closure.captures = captures;
                }
                let result = self.eval(*body);
                for (slot, _) in bindings {
                    self.scratch.locals[self.base + slot.0] = None;
                }
                return result;
            }
            Expr::If { test, yes, no } => {
                let test = self.eval_value(*test)?;
                let truthy = !matches!(
                    self.context.owner.store.borrow().unannotated(test.value()),
                    Value::Nil | Value::Boolean(false)
                );
                return self.eval(if truthy { *yes } else { *no });
            }
            Expr::Case {
                value,
                table,
                branches,
                default,
            } => {
                let value = self.eval_value(*value)?;
                let table = self.unit.constant(table.0).expect("case constants");
                let index = table
                    .get(&value)
                    .map_err(|e| self.error(id, EvalErrorKind::Collection(e)))?;
                return if let Some(index) = index {
                    let ValueView::Integer(index) = index.inspect() else {
                        unreachable!("case branch index")
                    };
                    self.eval(branches[index as usize])
                } else if let Some(default) = default {
                    self.eval(*default)
                } else {
                    Err(self.error(id, EvalErrorKind::NoMatchingClause))
                };
            }
            Expr::Do(children) => {
                let mut result = Control::Value(self.context.nil());
                for child in children {
                    result = self.eval(*child)?;
                    if matches!(result, Control::Recur { .. }) {
                        break;
                    }
                }
                return Ok(result);
            }
            Expr::Call { callee, arguments } => {
                let callee = self.eval_value(*callee)?;
                let base = self.scratch.values.len();
                for argument in arguments {
                    let value = self.eval_value(*argument)?;
                    self.scratch.values.push(value);
                }
                let result = invoke(self.context, &callee, base, self.scratch, None);
                self.scratch.values.truncate(base);
                result.map_err(|error| match error {
                    InvokeError::Dispatch(kind) => self.error(id, kind),
                    InvokeError::Execution(error) => error,
                })?
            }
            Expr::Let { bindings, body } => {
                let result = (|| {
                    for (slot, init) in bindings {
                        self.scratch.locals[self.base + slot.0] = Some(self.eval_value(*init)?);
                    }
                    self.eval(*body)
                })();
                for (slot, _) in bindings {
                    self.scratch.locals[self.base + slot.0] = None;
                }
                return result;
            }
            Expr::Loop {
                bindings,
                body,
                target,
            } => {
                let result = (|| {
                    for (slot, init) in bindings {
                        self.scratch.locals[self.base + slot.0] = Some(self.eval_value(*init)?);
                    }
                    self.eval_target(*body, target)
                })();
                for (slot, _) in bindings {
                    self.scratch.locals[self.base + slot.0] = None;
                }
                result?
            }
            Expr::Recur { target, arguments } => {
                let values_base = self.scratch.values.len();
                for argument in arguments {
                    let value = self.eval_value(*argument)?;
                    self.scratch.values.push(value);
                }
                self.context
                    .poll_execution(PollPoint::Recur)
                    .map_err(|kind| self.error(id, kind))?;
                return Ok(Control::Recur {
                    target: *target,
                    values_base,
                });
            }
            Expr::Collection {
                kind,
                children,
                metadata,
            } => self.eval_collection(id, *kind, children, *metadata)?,
        };
        // This is an execution-order test probe, not a language callback. The
        // production evaluator never reads provenance on the successful path.
        #[cfg(test)]
        self.context
            .evaluation_trace
            .push(self.unit.span(id.0).unwrap().text().into());
        Ok(Control::Value(result))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recur_validation_tracks_try_barriers_and_resets_for_new_targets() {
        let mut c = Context::new();
        for (text, valid) in [
            ("(recur 1)", false),
            ("(loop* [x 1] (recur x))", true),
            ("(fn* [x] (recur x))", true),
        ] {
            let source = c.source("barrier-test", text);
            let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
            let mut analyzer = Analyzer {
                context: &mut c,
                expressions: Vec::new(),
                scratch: AnalysisScratch {
                    scopes: vec![Scope::default()],
                    ..Default::default()
                },
                cacheable: true,
                functions: Vec::new(),
                next_target: 1,
                permissions: Vec::new(),
                depth: 0,
            };
            // Exercise target reset independently of the source-level try tests.
            let recur = RecurContext {
                target: Some((TargetId(0), 1, 0)),
                tail: true,
                try_depth: 1,
            };
            let result = analyzer.analyze(&form, recur);
            if valid {
                assert!(result.is_ok(), "{text}");
            } else {
                let error = result.unwrap_err();
                assert_eq!(
                    error.kind,
                    EvalErrorKind::InvalidForm("cannot recur across try")
                );
                assert_eq!(error.span.text(), "(recur 1)");
            }
        }
    }

    #[test]
    fn recur_reuses_scratch_and_releases_all_frame_roots_on_exit() {
        let mut c = Context::new();
        for iterations in [100, 1000] {
            let source = format!(
                "(loop* [x 0 y 1] (let* [temporary [x]] (if (= x {iterations}) y (recur (inc x) (+ x y)))))"
            );
            c.eval_string(&source).unwrap();
            assert!(c.evaluation.locals.is_empty() && c.evaluation.values.is_empty());
            assert_eq!(c.active_calls, 0);
            assert_eq!(c.active_expressions, 0);
            assert!(c.evaluation.locals.capacity() <= 4);
            assert!(c.evaluation.values.capacity() <= 8);
        }
    }

    fn keywords(context: &Context) -> Vec<&str> {
        context
            .evaluation_trace
            .iter()
            .map(String::as_str)
            .filter(|s| s.starts_with(':'))
            .collect()
    }

    #[test]
    fn primitive_execution_visits_each_selected_member_once_in_source_order() {
        let mut c = Context::new();
        c.eval_string("(let* [x (do :a :x) y (do :b x)] [^{:m (do :meta :mv)} {(do :key :k) (do :value y)} (if (do :test false) :unused (do :selected :yes)) (do :last x)])").unwrap();
        assert_eq!(
            keywords(&c),
            [
                ":a",
                ":x",
                ":b",
                ":key",
                ":k",
                ":value",
                ":m",
                ":meta",
                ":mv",
                ":test",
                ":selected",
                ":yes",
                ":last"
            ]
        );
        assert!(c.evaluation.locals.is_empty() && c.evaluation.values.is_empty());
        let capacity = c.evaluation.locals.capacity();
        c.eval_string("(let* [x 2] x)").unwrap();
        assert_eq!(c.evaluation.locals.capacity(), capacity);
    }

    #[test]
    fn top_level_analysis_and_execution_are_interleaved_including_nested_do() {
        for source in [
            ":before (do :inner-before missing) :after",
            "(do :before (do :inner-before missing) :after)",
            "(do :before) :inner-before [1",
        ] {
            let mut c = Context::new();
            c.eval_string(source).unwrap_err();
            assert_eq!(keywords(&c), [":before", ":inner-before"], "{source}");
        }
        for source in [
            "[(do :before missing)]",
            "(let* [] :before missing)",
            "(if true :before missing)",
            "(do :before [1)",
        ] {
            let mut c = Context::new();
            c.eval_string(source).unwrap_err();
            assert!(c.evaluation_trace.is_empty(), "{source}");
        }
    }

    #[test]
    fn checked_collection_failure_follows_all_members_and_precedes_metadata() {
        let mut c = Context::new();
        let error = c.eval_string("(let* [x :k y :k] ^{:after :metadata} {x (do :one 1) y (do :two 2) (do :third-key :j) (do :three 3)})").unwrap_err();
        assert_eq!(error.kind, EvalErrorKind::DuplicateMapKey);
        assert_eq!(
            keywords(&c),
            [":k", ":k", ":one", ":two", ":third-key", ":j", ":three"]
        );
        assert!(c.evaluation.locals.is_empty() && c.evaluation.values.is_empty());
    }
}
