//! Syntax tree for the documented JavaScript subset (`docs/grammar.md`).
//!
//! Every node carries the [`Span`] of the source text it was parsed from.
//! Parentheses are not represented: a parenthesized expression is its inner
//! expression, with a span that covers the parentheses. The tree only
//! contains supported forms; the parser rejects everything else before
//! building a node.
//!
//! Every [`Stmt`], [`Function`] and [`Expr`] records its `height` (nodes on
//! the longest path down to a leaf, itself included). The parser rejects
//! input whose tree would be higher than [`MAX_NESTING`](super::MAX_NESTING), so recursive
//! passes over a parsed tree have a fixed depth bound.

use super::Span;

/// A whole script: a statement list.
#[derive(Clone, Debug, PartialEq)]
pub struct Script {
    pub body: Vec<Stmt>,
    pub span: Span,
}

/// A binding name or property name with its span.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Ident {
    pub name: String,
    pub span: Span,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Stmt {
    pub kind: StmtKind,
    pub span: Span,
    /// Tree height; at most [`MAX_NESTING`](super::MAX_NESTING).
    pub height: usize,
}

impl Stmt {
    /// A statement node with its height computed from its children.
    pub(crate) fn new(kind: StmtKind, span: Span) -> Stmt {
        let child = match &kind {
            StmtKind::FunctionDecl(f) => f.height,
            StmtKind::Lexical(decl) => decl
                .declarators
                .iter()
                .filter_map(|d| d.init.as_ref().map(|e| e.height))
                .max()
                .unwrap_or(0),
            StmtKind::Expr(e) | StmtKind::Return(Some(e)) | StmtKind::Throw(e) => e.height,
            StmtKind::Block(body) => max_height(body),
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => test
                .height
                .max(consequent.height)
                .max(alternate.as_ref().map_or(0, |s| s.height)),
            StmtKind::While { test, body } => test.height.max(body.height),
            StmtKind::DoWhile { body, test } => body.height.max(test.height),
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => {
                let init = match init {
                    Some(ForInit::Expr(e)) => e.height,
                    Some(ForInit::Lexical(decl)) => decl.height(),
                    None => 0,
                };
                init.max(test.as_ref().map_or(0, |e| e.height))
                    .max(update.as_ref().map_or(0, |e| e.height))
                    .max(body.height)
            }
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => max_height(block)
                .max(handler.as_ref().map_or(0, |h| max_height(&h.body)))
                .max(finalizer.as_deref().map_or(0, max_height)),
            StmtKind::Labelled { body, .. } => body.height,
            StmtKind::Switch {
                discriminant,
                cases,
            } => cases.iter().fold(discriminant.height, |h, c| {
                h.max(c.test.as_ref().map_or(0, |e| e.height))
                    .max(max_height(&c.body))
            }),
            StmtKind::Return(None)
            | StmtKind::Break(_)
            | StmtKind::Continue(_)
            | StmtKind::Empty => 0,
        };
        Stmt {
            kind,
            span,
            height: child + 1,
        }
    }
}

fn max_height(body: &[Stmt]) -> usize {
    body.iter().map(|s| s.height).max().unwrap_or(0)
}

#[derive(Clone, Debug, PartialEq)]
pub enum StmtKind {
    /// `function name(params) { body }` (always named). Allowed only directly
    /// in a statement list (script, function body or block).
    FunctionDecl(Function),
    /// `let a = 1, b;` or `const c = 2;`. Allowed only directly in a
    /// statement list.
    Lexical(LexicalDecl),
    /// `expr;`
    Expr(Expr),
    /// `return;` or `return expr;` (only inside a function body).
    Return(Option<Expr>),
    /// `{ ... }`
    Block(Vec<Stmt>),
    If {
        test: Expr,
        consequent: Box<Stmt>,
        alternate: Option<Box<Stmt>>,
    },
    While {
        test: Expr,
        body: Box<Stmt>,
    },
    /// `do body while (test);` (clj-quickjs-80): the body runs before the
    /// first test.
    DoWhile {
        body: Box<Stmt>,
        test: Expr,
    },
    /// `for (init; test; update) body` (clj-quickjs-81). An absent test is
    /// true. A lexical head's bindings are scoped to the loop, and each
    /// iteration has its own copy of its `let` bindings.
    For {
        init: Option<ForInit>,
        test: Option<Expr>,
        update: Option<Expr>,
        body: Box<Stmt>,
    },
    /// `throw expr;`
    Throw(Expr),
    /// `try { ... }` with a `catch` clause, a `finally` block, or both (at
    /// least one is present).
    Try {
        block: Vec<Stmt>,
        handler: Option<CatchClause>,
        finalizer: Option<Vec<Stmt>>,
    },
    /// `break;` leaves the innermost enclosing loop or `switch`
    /// (clj-quickjs-83) (only inside one of the same function); `break L;`
    /// (clj-quickjs-82) leaves the enclosing statement labelled `L` of the
    /// same function.
    Break(Option<Ident>),
    /// `continue;` starts the next iteration of the innermost enclosing
    /// loop (only inside a loop of the same function); `continue L;`
    /// (clj-quickjs-82) that of the enclosing loop labelled `L` of the same
    /// function.
    Continue(Option<Ident>),
    /// `L: body` (clj-quickjs-82). The body is never a declaration. A label
    /// set `L1: L2: body` nests: the outer node's body is the inner one.
    Labelled {
        label: Ident,
        body: Box<Stmt>,
    },
    /// `switch (discriminant) { case e: ... default: ... }`
    /// (clj-quickjs-83). The clauses are in source order, with at most one
    /// `default`, anywhere among them. The clause bodies together form one
    /// statement list (one scope, the case block), which control may enter
    /// at the start of any clause.
    Switch {
        discriminant: Expr,
        cases: Vec<SwitchCase>,
    },
    /// `;`
    Empty,
}

/// A `case test:` clause (or, without a test, `default:`) of a `switch`
/// with the statements up to the next clause.
#[derive(Clone, Debug, PartialEq)]
pub struct SwitchCase {
    /// `None` for `default`.
    pub test: Option<Expr>,
    pub body: Vec<Stmt>,
    /// From the `case`/`default` keyword to the end of its last statement
    /// (or its `:`).
    pub span: Span,
}

/// The head of a `for` statement before its first `;`.
#[derive(Clone, Debug, PartialEq)]
pub enum ForInit {
    /// `for (i = 0; ...)`
    Expr(Expr),
    /// `for (let i = 0, j; ...)` or `for (const k = 1; ...)`
    Lexical(LexicalDecl),
}

/// `catch (param) { body }` or `catch { body }`.
#[derive(Clone, Debug, PartialEq)]
pub struct CatchClause {
    /// A plain identifier; `None` for an omitted catch binding.
    pub param: Option<Ident>,
    pub body: Vec<Stmt>,
    /// From the `catch` keyword to the closing `}`.
    pub span: Span,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LexicalKind {
    Let,
    Const,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LexicalDecl {
    pub kind: LexicalKind,
    /// At least one. Every `const` declarator has an initializer.
    pub declarators: Vec<Declarator>,
}

impl LexicalDecl {
    /// The height of its highest initializer (0 without one).
    pub(crate) fn height(&self) -> usize {
        self.declarators
            .iter()
            .filter_map(|d| d.init.as_ref().map(|e| e.height))
            .max()
            .unwrap_or(0)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Declarator {
    pub name: Ident,
    pub init: Option<Expr>,
    pub span: Span,
}

/// A function declaration or expression, or an arrow function.
#[derive(Clone, Debug, PartialEq)]
pub struct Function {
    /// The syntax that produced it.
    pub kind: FunctionKind,
    /// Always present for a declaration; optional for an expression;
    /// `None` for an arrow function.
    pub name: Option<Ident>,
    /// Plain identifier parameters with distinct names.
    pub params: Vec<Ident>,
    pub body: Vec<Stmt>,
    /// From the `function` keyword (for an arrow function, its first
    /// token) to the end of the body.
    pub span: Span,
    /// Tree height; at most [`MAX_NESTING`](super::MAX_NESTING).
    pub height: usize,
}

impl Function {
    pub(crate) fn new(
        kind: FunctionKind,
        name: Option<Ident>,
        params: Vec<Ident>,
        body: Vec<Stmt>,
        span: Span,
    ) -> Function {
        let height = max_height(&body) + 1;
        Function {
            kind,
            name,
            params,
            body,
            span,
            height,
        }
    }
}

/// Which syntax produced a [`Function`]. The subset has no `this`,
/// `arguments`, `new.target` or `super`, so an arrow function behaves as an
/// anonymous function expression with the same parameters and body.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FunctionKind {
    /// A `function` declaration or expression.
    Ordinary,
    /// `params => { body }`.
    Arrow,
    /// `params => expression`: `body` is exactly one `return expression`
    /// statement whose span is the expression's.
    ConciseArrow,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Expr {
    pub kind: ExprKind,
    pub span: Span,
    /// Tree height; at most [`MAX_NESTING`](super::MAX_NESTING).
    pub height: usize,
}

impl Expr {
    /// An expression node with its height computed from its children.
    pub(crate) fn new(kind: ExprKind, span: Span) -> Expr {
        let child = match &kind {
            ExprKind::Number(_)
            | ExprKind::String(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::Undefined
            | ExprKind::Identifier(_)
            | ExprKind::Update { .. } => 0,
            ExprKind::Unary { operand, .. } => operand.height,
            ExprKind::Binary { left, right, .. } | ExprKind::Logical { left, right, .. } => {
                left.height.max(right.height)
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => test.height.max(consequent.height).max(alternate.height),
            ExprKind::Assign { value, .. } => value.height,
            ExprKind::Call { callee, args } => args
                .iter()
                .map(|a| a.height)
                .fold(callee.height, usize::max),
            ExprKind::Member { object, .. } => object.height,
            ExprKind::Function(f) => f.height,
            ExprKind::Sequence { left, right } => left.height.max(right.height),
        };
        Expr {
            kind,
            span,
            height: child + 1,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ExprKind {
    /// A numeric literal, or `NaN` / `Infinity` (which the subset reserves).
    Number(f64),
    String(String),
    Bool(bool),
    Null,
    /// `undefined` (which the subset reserves).
    Undefined,
    /// A reference to a binding.
    Identifier(String),
    Unary {
        op: UnaryOp,
        operand: Box<Expr>,
    },
    Binary {
        op: BinaryOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `&&` and `||` (short-circuiting, value-returning).
    Logical {
        op: LogicalOp,
        left: Box<Expr>,
        right: Box<Expr>,
    },
    /// `test ? consequent : alternate`
    Conditional {
        test: Box<Expr>,
        consequent: Box<Expr>,
        alternate: Box<Expr>,
    },
    /// `name = value`. Only identifier targets are supported.
    Assign {
        target: Ident,
        value: Box<Expr>,
    },
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    /// `object.property` (static property name only). Parsed so later
    /// stages can resolve reserved intrinsic bindings; the subset has no
    /// object values of its own.
    Member {
        object: Box<Expr>,
        property: Ident,
    },
    /// `++x`, `x++`, `--x` or `x--` on an identifier binding (property
    /// targets are not supported yet). The target may be parenthesized.
    Update {
        op: UpdateOp,
        /// `++x` / `--x` (the value is the new one) rather than `x++` /
        /// `x--` (the value is the old one, converted to a number).
        prefix: bool,
        target: Ident,
        /// The `++` / `--` token, where run-time failures of the update
        /// itself are reported.
        op_span: Span,
    },
    Function(Box<Function>),
    /// `left, right` (the comma operator): evaluates `left` and discards its
    /// value, then evaluates `right`, whose value is the result. Left-
    /// associative: `a, b, c` is `(a, b), c`.
    Sequence {
        left: Box<Expr>,
        right: Box<Expr>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnaryOp {
    /// `!`
    Not,
    /// `-`
    Neg,
    /// `+`
    Plus,
    /// `~`
    BitNot,
}

/// The operator of an [`ExprKind::Update`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UpdateOp {
    /// `++`
    Inc,
    /// `--`
    Dec,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinaryOp {
    Mul,
    Div,
    Rem,
    Add,
    Sub,
    Lt,
    Gt,
    Le,
    Ge,
    /// `==`
    LooseEq,
    /// `!=`
    LooseNe,
    /// `===`
    StrictEq,
    /// `!==`
    StrictNe,
    /// `&`
    BitAnd,
    /// `|`
    BitOr,
    /// `^`
    BitXor,
    /// `<<`
    Shl,
    /// `>>`
    Sar,
    /// `>>>`
    UShr,
    /// `**`
    Exp,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}
