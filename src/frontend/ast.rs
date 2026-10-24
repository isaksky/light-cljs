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
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => max_height(block)
                .max(handler.as_ref().map_or(0, |h| max_height(&h.body)))
                .max(finalizer.as_deref().map_or(0, max_height)),
            StmtKind::Return(None) | StmtKind::Empty => 0,
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
    /// `throw expr;`
    Throw(Expr),
    /// `try { ... }` with a `catch` clause, a `finally` block, or both (at
    /// least one is present).
    Try {
        block: Vec<Stmt>,
        handler: Option<CatchClause>,
        finalizer: Option<Vec<Stmt>>,
    },
    /// `;`
    Empty,
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
            | ExprKind::Identifier(_) => 0,
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
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogicalOp {
    And,
    Or,
}
