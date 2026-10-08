//! Resolve the executable core to an arena with stable lexical binding IDs.

use std::{collections::HashMap, fmt, ops::Range};

use crate::{CollectionKind, NodeId, NodeKind, NumberKind, SyntaxTree};

pub type Span = Range<usize>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExprId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LocalId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StringId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct GlobalId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct JsGlobalId(pub usize);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GlobalBinding {
    pub namespace: String,
    pub name: String,
    pub span: Span,
    pub source_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JsGlobal {
    /// Validated ASCII JavaScript identifiers, never raw source interpolation.
    pub path: Vec<String>,
}

/// A namespace whose dependencies have already been placed earlier in the slice.
/// The caller validates its first `ns` form and the dependency graph.
pub struct NamespaceInput<'a> {
    pub name: &'a str,
    pub source: &'a str,
    pub syntax: &'a SyntaxTree,
    pub aliases: &'a [(String, String)],
    pub requires: &'a [String],
}

/// Both analysis (including the future macro hook) and emission track value use.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ExprContext {
    Value,
    Discard,
    Return,
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    Nil,
    Bool(bool),
    Number(f64),
    String(StringId),
    Local(LocalId),
    Global(GlobalId),
    JsGlobal(JsGlobalId),
    Def {
        global: GlobalId,
        init: Option<ExprId>,
    },
    Property {
        receiver: ExprId,
        property: String,
    },
    MethodCall {
        receiver: ExprId,
        method: String,
        args: Vec<ExprId>,
    },
    If {
        test: ExprId,
        yes: ExprId,
        no: ExprId,
    },
    Do(Vec<ExprId>),
    Let {
        bindings: Vec<(LocalId, ExprId)>,
        body: ExprId,
    },
    Fn {
        name: Option<LocalId>,
        params: Vec<LocalId>,
        body: ExprId,
    },
    Call {
        callee: ExprId,
        args: Vec<ExprId>,
    },
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    pub context: ExprContext,
    pub source_index: usize,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBinding {
    /// Diagnostic spelling only. References and emitted names use LocalId.
    pub name: String,
    pub span: Span,
    pub source_index: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub exprs: Vec<Expr>,
    pub locals: Vec<LocalBinding>,
    pub globals: Vec<GlobalBinding>,
    pub js_globals: Vec<JsGlobal>,
    /// Owned UTF-16 preserves even lone surrogate escapes.
    pub strings: Vec<Vec<u16>>,
    pub root: ExprId,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum AnalysisErrorKind {
    UnresolvedSymbol(String),
    Unsupported(String),
    InvalidForm(&'static str),
    InvalidBinding,
    DuplicateParameter(String),
    NumberOutOfRange,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AnalysisError {
    pub kind: AnalysisErrorKind,
    pub span: Span,
}

impl fmt::Display for AnalysisError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.kind {
            AnalysisErrorKind::UnresolvedSymbol(name) => write!(f, "unresolved symbol: {name}"),
            AnalysisErrorKind::Unsupported(what) => write!(f, "unsupported construct: {what}"),
            AnalysisErrorKind::InvalidForm(message) => f.write_str(message),
            AnalysisErrorKind::InvalidBinding => {
                f.write_str("binding must be a simple symbol without '/', '.', or '&'")
            }
            AnalysisErrorKind::DuplicateParameter(name) => write!(f, "duplicate parameter: {name}"),
            AnalysisErrorKind::NumberOutOfRange => {
                f.write_str("number outside supported range (safe integers and finite doubles)")
            }
        }
    }
}

impl std::error::Error for AnalysisError {}

/// A scope holds IDs, while records live for the entire compilation unit.
#[derive(Default)]
struct Environment {
    scopes: Vec<Vec<LocalId>>,
}

struct Analyzer<'a> {
    source: &'a str,
    syntax: &'a SyntaxTree,
    program: Program,
    env: Environment,
    globals: HashMap<(String, String), GlobalId>,
    namespace: &'a str,
    aliases: &'a [(String, String)],
    requires: &'a [String],
    source_index: usize,
}

fn empty_program() -> Program {
    Program {
        exprs: Vec::new(),
        locals: Vec::new(),
        globals: Vec::new(),
        js_globals: Vec::new(),
        strings: Vec::new(),
        root: ExprId(0),
    }
}

/// Analyze a parser-produced tree against its exact original source buffer.
/// Standalone input has namespace `cljs.user`; filesystem loading is explicit.
pub fn analyze(source: &str, syntax: &SyntaxTree) -> Result<Program, AnalysisError> {
    let mut analyzer = Analyzer {
        source,
        syntax,
        program: empty_program(),
        env: Environment::default(),
        globals: HashMap::new(),
        namespace: "cljs.user",
        aliases: &[],
        requires: &[],
        source_index: 0,
    };
    analyzer.program.root = analyzer.body(&syntax.roots, 0..source.len(), ExprContext::Return)?;
    Ok(analyzer.program)
}

/// Analyze validated namespaces in dependency-first order into one shared unit.
pub fn analyze_namespaces(
    inputs: &[NamespaceInput<'_>],
) -> Result<Program, (usize, AnalysisError)> {
    let mut program = empty_program();
    let mut globals = HashMap::new();
    let mut roots = Vec::new();
    for (source_index, input) in inputs.iter().enumerate() {
        let mut analyzer = Analyzer {
            source: input.source,
            syntax: input.syntax,
            program,
            env: Environment::default(),
            globals,
            namespace: input.name,
            aliases: input.aliases,
            requires: input.requires,
            source_index,
        };
        let context = if source_index + 1 == inputs.len() {
            ExprContext::Return
        } else {
            ExprContext::Discard
        };
        let root = analyzer
            .body(&input.syntax.roots[1..], 0..input.source.len(), context)
            .map_err(|error| (source_index, error))?;
        roots.push(root);
        program = analyzer.program;
        globals = analyzer.globals;
    }
    program.root = ExprId(program.exprs.len());
    program.exprs.push(Expr {
        kind: ExprKind::Do(roots),
        span: 0..0,
        context: ExprContext::Return,
        source_index: 0,
    });
    Ok(program)
}

impl Analyzer<'_> {
    fn push(&mut self, kind: ExprKind, span: Span, context: ExprContext) -> ExprId {
        let id = ExprId(self.program.exprs.len());
        self.program.exprs.push(Expr {
            kind,
            span,
            context,
            source_index: self.source_index,
        });
        id
    }

    fn error(&self, node: NodeId, kind: AnalysisErrorKind) -> AnalysisError {
        AnalysisError {
            kind,
            span: self.syntax.nodes[node.0].span.clone(),
        }
    }

    fn spelling(&self, node: NodeId) -> &str {
        &self.source[self.syntax.nodes[node.0].span.clone()]
    }

    fn lookup(&self, name: &str) -> Option<LocalId> {
        self.env
            .scopes
            .iter()
            .rev()
            .flat_map(|scope| scope.iter().rev())
            .copied()
            .find(|id| self.program.locals[id.0].name == name)
    }

    fn binding_name(&self, node: NodeId) -> Result<&str, AnalysisError> {
        let name = self.spelling(node);
        if !matches!(self.syntax.nodes[node.0].kind, NodeKind::Symbol)
            || name.contains(['/', '.'])
            || name == "&"
        {
            return Err(self.error(node, AnalysisErrorKind::InvalidBinding));
        }
        Ok(name)
    }

    fn bind(&mut self, node: NodeId) -> Result<LocalId, AnalysisError> {
        let name = self.binding_name(node)?.to_owned();
        let id = LocalId(self.program.locals.len());
        self.program.locals.push(LocalBinding {
            name,
            span: self.syntax.nodes[node.0].span.clone(),
            source_index: self.source_index,
        });
        self.env.scopes.last_mut().expect("binding scope").push(id);
        Ok(id)
    }

    fn form(&mut self, id: NodeId, context: ExprContext) -> Result<ExprId, AnalysisError> {
        let node = &self.syntax.nodes[id.0];
        let kind = match &node.kind {
            NodeKind::Nil => ExprKind::Nil,
            NodeKind::Boolean(value) => ExprKind::Bool(*value),
            NodeKind::Number(kind @ (NumberKind::Integer | NumberKind::Float)) => {
                let value = number(self.spelling(id), *kind)
                    .ok_or_else(|| self.error(id, AnalysisErrorKind::NumberOutOfRange))?;
                ExprKind::Number(value)
            }
            NodeKind::String(value) => {
                let id = StringId(self.program.strings.len());
                self.program.strings.push(value.to_utf16(self.source));
                ExprKind::String(id)
            }
            NodeKind::Symbol => {
                let name = self.spelling(id).to_owned();
                self.resolve(id, &name)?
            }
            NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } => return self.list(id, children, context),
            other => {
                let description = match other {
                    NodeKind::Number(NumberKind::BigInteger) => "big integers",
                    NodeKind::Number(NumberKind::Decimal) => "decimal literals",
                    NodeKind::Number(NumberKind::Ratio) => "ratios",
                    NodeKind::Keyword { .. } => "keywords",
                    NodeKind::Character(_) => "characters",
                    NodeKind::SymbolicValue(_) => "symbolic numbers",
                    NodeKind::Regex => "regular expressions",
                    NodeKind::Collection { .. } => {
                        "collection literals or anonymous reader functions"
                    }
                    NodeKind::Prefix { .. } => "reader prefixes (including quote)",
                    NodeKind::Metadata { .. } => "metadata",
                    NodeKind::Tagged { .. } => "tagged literals",
                    NodeKind::Conditional { .. } => "reader conditionals",
                    NodeKind::NamespacedMap { .. } => "namespaced maps",
                    NodeKind::Argument { .. } => "anonymous reader function arguments",
                    _ => unreachable!(),
                };
                return Err(self.error(id, AnalysisErrorKind::Unsupported(description.into())));
            }
        };
        Ok(self.push(kind, node.span.clone(), context))
    }

    fn list(
        &mut self,
        id: NodeId,
        children: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&operator, args)) = children.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::Unsupported("empty list literal".into()),
            ));
        };
        if matches!(self.syntax.nodes[operator.0].kind, NodeKind::Nil) {
            return Err(self.error(operator, AnalysisErrorKind::InvalidForm("cannot call nil")));
        }
        // Binding forms must be recognized before their children are analyzed.
        if matches!(self.syntax.nodes[operator.0].kind, NodeKind::Symbol) {
            let operator_name = self.spelling(operator).to_owned();
            match operator_name.as_str() {
                "if" => return self.if_form(id, args, context),
                "do" => return self.body(args, self.syntax.nodes[id.0].span.clone(), context),
                "let*" => return self.let_form(id, args, context),
                "fn*" => return self.fn_form(id, args, context),
                "def" => return self.def_form(id, args, context),
                name if name.starts_with('.') && name != "." => {
                    return self.interop(id, name, args, context);
                }
                "ns" | "loop*" | "letfn*" | "recur" | "throw" | "try" | "set!" | "new" | "."
                | "js*" | "quote" | "var" | "deftype*" | "defrecord*" => {
                    return Err(self.error(
                        operator,
                        AnalysisErrorKind::Unsupported(self.spelling(operator).into()),
                    ));
                }
                _ => {}
            }
        }
        // Macro lookup belongs HERE, with the current lexical environment and
        // expression context, before ordinary invocation analysis.
        if let Some(expanded) = self.expand_macro(operator, context)? {
            return self.form(expanded, context);
        }
        let callee = self.form(operator, ExprContext::Value)?;
        let args = args
            .iter()
            .map(|&arg| self.form(arg, ExprContext::Value))
            .collect::<Result<_, _>>()?;
        Ok(self.push(
            ExprKind::Call { callee, args },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn resolve(&mut self, id: NodeId, name: &str) -> Result<ExprKind, AnalysisError> {
        if let Some(local) = self.lookup(name) {
            return Ok(ExprKind::Local(local));
        }
        if let Some(path) = name.strip_prefix("js/") {
            // Standard ClojureScript resolves js/x to a same-named lexical x.
            if let Some(local) = self.lookup(path) {
                return Ok(ExprKind::Local(local));
            }
            let path: Vec<_> = path.split('.').map(str::to_owned).collect();
            if path.iter().any(|part| !js_identifier(part)) {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::Unsupported(
                        "JavaScript paths require ASCII identifier components".into(),
                    ),
                ));
            }
            let global = self
                .program
                .js_globals
                .iter()
                .position(|global| global.path == path)
                .map(JsGlobalId)
                .unwrap_or_else(|| {
                    let id = JsGlobalId(self.program.js_globals.len());
                    self.program.js_globals.push(JsGlobal { path });
                    id
                });
            return Ok(ExprKind::JsGlobal(global));
        }
        let (namespace, symbol) = if let Some((qualifier, symbol)) = name.split_once('/') {
            let namespace = self
                .aliases
                .iter()
                .find(|(alias, _)| alias == qualifier)
                .map(|(_, namespace)| namespace.as_str())
                .unwrap_or(qualifier);
            if namespace != self.namespace
                && !self.requires.iter().any(|required| required == namespace)
            {
                return Err(self.error(
                    id,
                    AnalysisErrorKind::UnresolvedSymbol(format!(
                        "{name} (unknown namespace or alias {qualifier})"
                    )),
                ));
            }
            (namespace, symbol)
        } else {
            (self.namespace, name)
        };
        self.globals
            .get(&(namespace.into(), symbol.into()))
            .copied()
            .map(ExprKind::Global)
            .ok_or_else(|| self.error(id, AnalysisErrorKind::UnresolvedSymbol(name.into())))
    }

    fn def_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !(1..=2).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("def requires a name and optional initializer"),
            ));
        }
        if args.len() == 1 && context == ExprContext::Value {
            return Err(self.error(id, AnalysisErrorKind::Unsupported(
                "uninitialized def in a value position (the reference emits invalid JavaScript)".into())));
        }
        let name = self.binding_name(args[0])?.to_owned();
        let key = (self.namespace.to_owned(), name.clone());
        let global = if let Some(&global) = self.globals.get(&key) {
            global
        } else {
            let global = GlobalId(self.program.globals.len());
            self.program.globals.push(GlobalBinding {
                namespace: self.namespace.into(),
                name,
                span: self.syntax.nodes[args[0].0].span.clone(),
                source_index: self.source_index,
            });
            self.globals.insert(key, global);
            global
        };
        // Registration precedes analysis, so self-reference sees stable storage.
        let init = args
            .get(1)
            .map(|&init| self.form(init, ExprContext::Value))
            .transpose()?;
        Ok(self.push(
            ExprKind::Def { global, init },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn interop(
        &mut self,
        id: NodeId,
        operator: &str,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let (property, field) = operator
            .strip_prefix(".-")
            .map(|name| (name, true))
            .unwrap_or((&operator[1..], false));
        if !js_identifier(property) {
            return Err(self.error(
                id,
                AnalysisErrorKind::Unsupported(
                    "JavaScript members require an ASCII identifier".into(),
                ),
            ));
        }
        if args.is_empty() || (field && args.len() != 1) {
            return Err(self.error(id, AnalysisErrorKind::InvalidForm("property reads require one receiver; method calls require a receiver followed by arguments")));
        }
        let receiver = self.form(args[0], ExprContext::Value)?;
        let kind = if field {
            ExprKind::Property {
                receiver,
                property: property.into(),
            }
        } else {
            let args = args[1..]
                .iter()
                .map(|&arg| self.form(arg, ExprContext::Value))
                .collect::<Result<_, _>>()?;
            ExprKind::MethodCall {
                receiver,
                method: property.into(),
                args,
            }
        };
        Ok(self.push(kind, self.syntax.nodes[id.0].span.clone(), context))
    }

    fn expand_macro(
        &self,
        operator: NodeId,
        _context: ExprContext,
    ) -> Result<Option<NodeId>, AnalysisError> {
        if !matches!(self.syntax.nodes[operator.0].kind, NodeKind::Symbol) {
            return Ok(None);
        }
        let name = self.spelling(operator);
        if self.lookup(name).is_some()
            || self
                .globals
                .contains_key(&(self.namespace.into(), name.into()))
        {
            return Ok(None);
        }
        // A small diagnostic catalog, not a macro implementation or resolver.
        if matches!(
            name.strip_prefix("cljs.core/").unwrap_or(name),
            "let"
                | "fn"
                | "defn"
                | "defmacro"
                | "when"
                | "when-not"
                | "cond"
                | "and"
                | "or"
                | "loop"
                | "letfn"
                | "->"
                | "->>"
                | "if-let"
                | "when-let"
        ) {
            return Err(self.error(
                operator,
                AnalysisErrorKind::Unsupported(format!(
                    "macro execution ({name}); use primitive forms"
                )),
            ));
        }
        Ok(None)
    }

    fn body(
        &mut self,
        forms: &[NodeId],
        span: Span,
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let mut exprs = Vec::with_capacity(forms.len());
        for (index, &form) in forms.iter().enumerate() {
            exprs.push(self.form(
                form,
                if index + 1 == forms.len() {
                    context
                } else {
                    ExprContext::Discard
                },
            )?);
        }
        Ok(self.push(ExprKind::Do(exprs), span, context))
    }

    fn if_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        if !(2..=3).contains(&args.len()) {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm(
                    "if requires a test, a then branch, and an optional else branch",
                ),
            ));
        }
        let test = self.form(args[0], ExprContext::Value)?;
        let yes = self.form(args[1], context)?;
        let no = if let Some(&no) = args.get(2) {
            self.form(no, context)?
        } else {
            self.push(ExprKind::Nil, self.syntax.nodes[id.0].span.clone(), context)
        };
        Ok(self.push(
            ExprKind::If { test, yes, no },
            self.syntax.nodes[id.0].span.clone(),
            context,
        ))
    }

    fn vector(&self, id: NodeId, message: &'static str) -> Result<&[NodeId], AnalysisError> {
        match &self.syntax.nodes[id.0].kind {
            NodeKind::Collection {
                kind: CollectionKind::Vector,
                children,
            } => Ok(children),
            _ => Err(self.error(id, AnalysisErrorKind::InvalidForm(message))),
        }
    }

    fn let_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let Some((&bindings, body)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("let* requires a binding vector"),
            ));
        };
        let pairs = self
            .vector(bindings, "let* requires a binding vector")?
            .to_vec();
        if !pairs.len().is_multiple_of(2) {
            return Err(self.error(
                bindings,
                AnalysisErrorKind::InvalidForm("let* requires an even number of binding forms"),
            ));
        }
        self.env.scopes.push(Vec::new());
        let result = (|| {
            let mut bindings = Vec::with_capacity(pairs.len() / 2);
            for pair in pairs.as_chunks::<2>().0 {
                self.binding_name(pair[0])?;
                // Own initializer sees outer bindings, but never its new binding.
                let init = self.form(pair[1], ExprContext::Value)?;
                let local = self.bind(pair[0])?;
                bindings.push((local, init));
            }
            let span = self.syntax.nodes[id.0].span.clone();
            let body = self.body(body, span.clone(), context)?;
            Ok(self.push(ExprKind::Let { bindings, body }, span, context))
        })();
        self.env.scopes.pop();
        result
    }

    fn fn_form(
        &mut self,
        id: NodeId,
        args: &[NodeId],
        context: ExprContext,
    ) -> Result<ExprId, AnalysisError> {
        let (name, args) = match args.split_first() {
            Some((&name, rest)) if matches!(self.syntax.nodes[name.0].kind, NodeKind::Symbol) => {
                (Some(name), rest)
            }
            _ => (None, args),
        };
        // Accept both (fn* [x] ...) and the single-method (fn* ([x] ...)).
        let args = match args.first().map(|id| &self.syntax.nodes[id.0].kind) {
            Some(NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            }) => {
                if args.len() != 1 {
                    return Err(self.error(
                        id,
                        AnalysisErrorKind::Unsupported("multiple function arities".into()),
                    ));
                }
                children.as_slice()
            }
            _ => args,
        };
        let Some((&params, body)) = args.split_first() else {
            return Err(self.error(
                id,
                AnalysisErrorKind::InvalidForm("fn* requires a parameter vector"),
            ));
        };
        let parameters = self
            .vector(params, "fn* requires a parameter vector")?
            .to_vec();
        self.env.scopes.push(Vec::new());
        let result = (|| {
            let name = name.map(|name| self.bind(name)).transpose()?;
            let mut params: Vec<LocalId> = Vec::with_capacity(parameters.len());
            for parameter in parameters {
                if self.spelling(parameter) == "&" {
                    return Err(self.error(
                        parameter,
                        AnalysisErrorKind::Unsupported("variadic parameters".into()),
                    ));
                }
                let spelling = self.binding_name(parameter)?;
                if params
                    .iter()
                    .any(|id| self.program.locals[id.0].name == spelling)
                {
                    return Err(self.error(
                        parameter,
                        AnalysisErrorKind::DuplicateParameter(spelling.into()),
                    ));
                }
                params.push(self.bind(parameter)?);
            }
            let span = self.syntax.nodes[id.0].span.clone();
            let body = self.body(body, span.clone(), ExprContext::Return)?;
            Ok(self.push(ExprKind::Fn { name, params, body }, span, context))
        })();
        self.env.scopes.pop();
        result
    }
}

fn number(text: &str, kind: NumberKind) -> Option<f64> {
    if kind == NumberKind::Float {
        return text.parse::<f64>().ok().filter(|n| n.is_finite());
    }
    let unsigned = text.strip_prefix(['+', '-']).unwrap_or(text);
    let (radix, digits) = if let Some(hex) = unsigned
        .strip_prefix("0x")
        .or_else(|| unsigned.strip_prefix("0X"))
    {
        (16, hex)
    } else if let Some((base, digits)) = unsigned.split_once(['r', 'R']) {
        (base.parse().ok()?, digits)
    } else if unsigned.starts_with('0') {
        (8, unsigned)
    } else {
        (10, unsigned)
    };
    let magnitude = u64::from_str_radix(digits, radix).ok()?;
    if magnitude > (1_u64 << 53) - 1 {
        return None;
    }
    // The JVM reader's integer -0 is ordinary positive zero.
    Some(if text.starts_with('-') && magnitude != 0 {
        -(magnitude as f64)
    } else {
        magnitude as f64
    })
}

fn js_identifier(name: &str) -> bool {
    let mut bytes = name.bytes();
    matches!(bytes.next(), Some(b'a'..=b'z' | b'A'..=b'Z' | b'_' | b'$'))
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'$'))
}
