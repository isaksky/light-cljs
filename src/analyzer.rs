//! Resolve the executable core to an arena with stable lexical binding IDs.

use std::{fmt, ops::Range};

use crate::{CollectionKind, NodeId, NodeKind, NumberKind, SyntaxTree};

pub type Span = Range<usize>;

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct ExprId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct LocalId(pub usize);

#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct StringId(pub usize);

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
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalBinding {
    /// Diagnostic spelling only. References and emitted names use LocalId.
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Program {
    pub exprs: Vec<Expr>,
    pub locals: Vec<LocalBinding>,
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
}

/// Analyze a parser-produced tree against its exact original source buffer.
/// No globals, namespace lookup, reader expansion, or macro execution yet.
pub fn analyze(source: &str, syntax: &SyntaxTree) -> Result<Program, AnalysisError> {
    let mut analyzer = Analyzer {
        source,
        syntax,
        program: Program {
            exprs: Vec::new(),
            locals: Vec::new(),
            strings: Vec::new(),
            root: ExprId(0),
        },
        env: Environment::default(),
    };
    analyzer.program.root = analyzer.body(&syntax.roots, 0..source.len(), ExprContext::Return)?;
    Ok(analyzer.program)
}

impl Analyzer<'_> {
    fn push(&mut self, kind: ExprKind, span: Span, context: ExprContext) -> ExprId {
        let id = ExprId(self.program.exprs.len());
        self.program.exprs.push(Expr {
            kind,
            span,
            context,
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
                let name = self.spelling(id);
                ExprKind::Local(self.lookup(name).ok_or_else(|| {
                    self.error(id, AnalysisErrorKind::UnresolvedSymbol(name.to_owned()))
                })?)
            }
            NodeKind::Collection {
                kind: CollectionKind::List,
                children,
            } => return self.list(id, children, context),
            other => {
                let description = match other {
                    NodeKind::Number(_) => "big integers, decimals, and ratios",
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
            match self.spelling(operator) {
                "if" => return self.if_form(id, args, context),
                "do" => return self.body(args, self.syntax.nodes[id.0].span.clone(), context),
                "let*" => return self.let_form(id, args, context),
                "fn*" => return self.fn_form(id, args, context),
                "def" | "ns" | "loop*" | "letfn*" | "recur" | "throw" | "try" | "set!" | "new"
                | "." | "js*" | "quote" | "var" | "deftype*" | "defrecord*" => {
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

    fn expand_macro(
        &self,
        operator: NodeId,
        _context: ExprContext,
    ) -> Result<Option<NodeId>, AnalysisError> {
        if !matches!(self.syntax.nodes[operator.0].kind, NodeKind::Symbol) {
            return Ok(None);
        }
        let name = self.spelling(operator);
        if self.lookup(name).is_some() {
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
