//! Recursive-descent parser for the documented JavaScript subset. Each
//! `parse_*` function implements the `docs/grammar.md` production of the same
//! name; forms outside the grammar are rejected where they would start, with
//! a positioned [`SyntaxError`].
//!
//! Structure follows the ECMAScript grammar (consulted alongside QuickJS's
//! `js_parse_statement`, `js_parse_assign_expr`, `js_parse_expr_binary` and
//! `js_parse_expect_semi` in `research/quickjs/quickjs.c` at 7d1c13c5d8f8);
//! no QuickJS code is adapted.

use super::ast::{
    BinaryOp, CatchClause, Declarator, Expr, ExprKind, Function, FunctionKind, Ident, LexicalDecl,
    LexicalKind, LogicalOp, Script, Stmt, StmtKind, UnaryOp, UpdateOp,
};
use super::intrinsics::RESERVED_BINDING;
use super::lexer::{Keyword, Lexer, Punct, Token, TokenKind};
use super::{MAX_NESTING, Span, SyntaxError, SyntaxErrorKind};

type PResult<T> = Result<T, SyntaxError>;

/// Nesting-budget cost of each construct, per nesting level. Weighted by the
/// measured Rust stack each one uses, so that the whole [`MAX_NESTING`]
/// budget fits a 2 MiB thread stack even in an unoptimized build (checked
/// by `frontend::tests::nesting_at_the_limit_fits_a_2_mib_stack`).
mod cost {
    pub const STATEMENT: usize = 1;
    /// `{ ... }`, including function bodies (on top of `STATEMENT`).
    pub const BLOCK: usize = 1;
    /// `if` / `while` (on top of `STATEMENT`).
    pub const BODY: usize = 1;
    pub const FUNCTION: usize = 3;
    pub const ASSIGNMENT: usize = 1;
    /// `? :` (on top of the `ASSIGNMENT` of each branch).
    pub const CONDITIONAL: usize = 1;
    /// A prefix operator, or one link of a binary-operator or member chain.
    pub const OPERATOR: usize = 1;
    /// One call's argument list (on top of each argument's `ASSIGNMENT`).
    pub const CALL: usize = 3;
    /// `( ... )` (on top of the inner `ASSIGNMENT`).
    pub const PAREN: usize = 3;
}

/// Global names the subset treats as reserved constants: they parse as
/// literals and cannot be bound or assigned.
const RESERVED_CONSTANTS: &[&str] = &["undefined", "NaN", "Infinity"];

/// Names strict mode forbids as binding names; the subset also does not
/// support referencing them.
const STRICT_RESTRICTED: &[&str] = &["eval", "arguments"];

pub(super) struct Parser<'a> {
    src: &'a str,
    lexer: Lexer<'a>,
    /// The current (not yet consumed) token.
    tok: Token,
    /// End offset of the last consumed token.
    prev_end: usize,
    depth: usize,
    in_function: bool,
    /// Inside the body of a loop of the current function (`break` and
    /// `continue` are allowed).
    in_loop: bool,
}

impl<'a> Parser<'a> {
    pub(super) fn new(src: &'a str) -> Parser<'a> {
        Parser {
            src,
            lexer: Lexer::new(src),
            tok: Token {
                kind: TokenKind::Eof,
                span: Span::new(0, 0),
                newline_before: false,
            },
            prev_end: 0,
            depth: 0,
            in_function: false,
            in_loop: false,
        }
    }

    // ----- token helpers -------------------------------------------------

    fn advance(&mut self) -> PResult<Token> {
        let next = self.lexer.next_token()?;
        let tok = std::mem::replace(&mut self.tok, next);
        self.prev_end = tok.span.end;
        Ok(tok)
    }

    /// The token after the current one, if it lexes cleanly. Used only to
    /// choose a diagnostic or a statement form, never to consume input.
    fn peek_next(&self) -> Option<Token> {
        self.lexer.clone().next_token().ok()
    }

    fn is_punct(&self, p: Punct) -> bool {
        self.tok.kind == TokenKind::Punct(p)
    }

    fn is_keyword(&self, k: Keyword) -> bool {
        self.tok.kind == TokenKind::Keyword(k)
    }

    fn eat_punct(&mut self, p: Punct) -> PResult<bool> {
        if self.is_punct(p) {
            self.advance()?;
            Ok(true)
        } else {
            Ok(false)
        }
    }

    fn error(&self, kind: SyntaxErrorKind, span: Span, message: impl Into<String>) -> SyntaxError {
        SyntaxError::new(self.src, kind, span, message)
    }

    /// The current token is not what the grammar requires here.
    fn unexpected(&self, expected: &str) -> SyntaxError {
        if self.tok.kind == TokenKind::Eof {
            self.error(
                SyntaxErrorKind::UnexpectedEnd,
                self.tok.span,
                format!("unexpected end of input, expected {expected}"),
            )
        } else {
            self.error(
                SyntaxErrorKind::UnexpectedToken,
                self.tok.span,
                format!("expected {expected}, found {}", self.tok.kind.describe()),
            )
        }
    }

    fn unsupported(&self, span: Span, what: &str) -> SyntaxError {
        self.error(
            SyntaxErrorKind::Unsupported,
            span,
            format!("{what} not supported by this JavaScript subset"),
        )
    }

    fn expect_punct(&mut self, p: Punct) -> PResult<Span> {
        if self.is_punct(p) {
            Ok(self.advance()?.span)
        } else {
            Err(self.unexpected(&format!("`{}`", p.as_str())))
        }
    }

    /// Statement terminator with automatic semicolon insertion: an explicit
    /// `;`, or (inserted) before `}`, at the end of input, or before a token
    /// preceded by a line terminator.
    fn expect_semi(&mut self) -> PResult<()> {
        if self.eat_punct(Punct::Semicolon)? {
            return Ok(());
        }
        if self.is_punct(Punct::RBrace)
            || self.tok.kind == TokenKind::Eof
            || self.tok.newline_before
        {
            return Ok(());
        }
        Err(self.unexpected("`;` or a line break after the statement"))
    }

    /// Charge `cost` units of the nesting budget; fails past [`MAX_NESTING`].
    /// Costs are proportional to the Rust stack each construct uses per
    /// nesting level (see [`cost`]). The count is restored by the caller on
    /// success (`self.depth = saved`); on error the whole parse is
    /// abandoned, so no restore is needed.
    fn deeper(&mut self, cost: usize) -> PResult<()> {
        self.depth += cost;
        if self.depth > MAX_NESTING {
            return Err(self.too_deep());
        }
        Ok(())
    }

    /// Reject a tree higher than [`MAX_NESTING`] nodes. The unit budget
    /// above bounds the parser's own recursion; this bounds the tree it
    /// builds, which can be higher when finished subtrees are wrapped
    /// again (for example `(a.b.b).b.b`).
    fn check_height(&self, height: usize) -> PResult<()> {
        if height > MAX_NESTING {
            return Err(self.too_deep());
        }
        Ok(())
    }

    fn expr(&self, kind: ExprKind, span: Span) -> PResult<Expr> {
        let expr = Expr::new(kind, span);
        self.check_height(expr.height)?;
        Ok(expr)
    }

    fn bounded(&self, stmt: Stmt) -> PResult<Stmt> {
        self.check_height(stmt.height)?;
        Ok(stmt)
    }

    #[inline(never)]
    fn too_deep(&self) -> SyntaxError {
        self.error(
            SyntaxErrorKind::TooDeep,
            self.tok.span,
            format!("nesting exceeds the limit of {MAX_NESTING} units"),
        )
    }

    fn span_from(&self, start: usize) -> Span {
        Span::new(start, self.prev_end.max(start))
    }

    // ----- script and statements ----------------------------------------

    pub(super) fn parse_script(mut self) -> PResult<Script> {
        self.advance()?;
        let mut body = Vec::new();
        while self.tok.kind != TokenKind::Eof {
            body.push(self.parse_statement_list_item()?);
        }
        Ok(Script {
            body,
            span: Span::new(0, self.src.len()),
        })
    }

    /// StatementListItem: a declaration or a statement.
    fn parse_statement_list_item(&mut self) -> PResult<Stmt> {
        match self.tok.kind {
            TokenKind::Keyword(Keyword::Function) => {
                let start = self.tok.span.start;
                let function = self.parse_function(true)?;
                self.bounded(Stmt::new(
                    StmtKind::FunctionDecl(function),
                    self.span_from(start),
                ))
            }
            TokenKind::Keyword(Keyword::Let | Keyword::Const) => self.parse_lexical_decl(),
            _ => self.parse_statement(),
        }
    }

    /// Statement (single-statement position: `if`/`while`/`do` bodies).
    fn parse_statement(&mut self) -> PResult<Stmt> {
        let saved = self.depth;
        self.deeper(cost::STATEMENT)?;
        let start = self.tok.span.start;
        let kind = match &self.tok.kind {
            TokenKind::Punct(Punct::LBrace) => StmtKind::Block(self.parse_block()?),
            TokenKind::Punct(Punct::Semicolon) => {
                self.advance()?;
                StmtKind::Empty
            }
            TokenKind::Keyword(Keyword::If) => self.parse_if()?,
            TokenKind::Keyword(Keyword::While) => self.parse_while()?,
            TokenKind::Keyword(Keyword::Do) => self.parse_do_while()?,
            TokenKind::Keyword(Keyword::Return) => self.parse_return()?,
            TokenKind::Keyword(Keyword::Throw) => self.parse_throw()?,
            TokenKind::Keyword(Keyword::Try) => self.parse_try()?,
            TokenKind::Keyword(Keyword::Break | Keyword::Continue) => {
                self.parse_break_continue()?
            }
            _ => {
                if let Some(error) = self.statement_start_error() {
                    return Err(error);
                }
                self.parse_expression_statement()?
            }
        };
        self.depth = saved;
        self.bounded(Stmt::new(kind, self.span_from(start)))
    }

    /// Rejects statement forms that start with the current token but are not
    /// expression statements of the subset. Kept out of line (as are the
    /// other `*_error` helpers) so the recursive parse functions keep small
    /// stack frames.
    #[inline(never)]
    fn statement_start_error(&self) -> Option<SyntaxError> {
        let span = self.tok.span;
        match &self.tok.kind {
            TokenKind::Keyword(Keyword::Function) => Some(self.error(
                SyntaxErrorKind::Invalid,
                span,
                "a function declaration cannot be the body of `if`/`while`/`do`; wrap it in a block",
            )),
            TokenKind::Keyword(k @ (Keyword::Let | Keyword::Const)) => Some(self.error(
                SyntaxErrorKind::Invalid,
                span,
                format!(
                    "a `{}` declaration cannot be the body of `if`/`while`/`do`; wrap it in a block",
                    k.as_str()
                ),
            )),
            TokenKind::Keyword(k) => {
                unsupported_statement(*k).map(|what| self.unsupported(span, what))
            }
            TokenKind::Identifier(_)
                if self
                    .peek_next()
                    .is_some_and(|t| t.kind == TokenKind::Punct(Punct::Colon)) =>
            {
                Some(self.unsupported(span, "labelled statements are"))
            }
            _ => None,
        }
    }

    fn parse_expression_statement(&mut self) -> PResult<StmtKind> {
        let expr = self.parse_expression()?;
        self.expect_semi()?;
        Ok(StmtKind::Expr(expr))
    }

    /// Block: `{` StatementListItem* `}`
    fn parse_block(&mut self) -> PResult<Vec<Stmt>> {
        let saved = self.depth;
        self.deeper(cost::BLOCK)?;
        self.expect_punct(Punct::LBrace)?;
        let mut body = Vec::new();
        while !self.is_punct(Punct::RBrace) {
            if self.tok.kind == TokenKind::Eof {
                return Err(self.unexpected("`}`"));
            }
            body.push(self.parse_statement_list_item()?);
        }
        self.advance()?;
        self.depth = saved;
        Ok(body)
    }

    /// `(` Expression `)` after `if` / `while` (also `do` ... `while`).
    fn parse_paren_condition(&mut self) -> PResult<Expr> {
        self.expect_punct(Punct::LParen)?;
        let test = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        Ok(test)
    }

    fn parse_if(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        self.deeper(cost::BODY)?;
        self.advance()?;
        let test = self.parse_paren_condition()?;
        let consequent = Box::new(self.parse_statement()?);
        let alternate = if self.is_keyword(Keyword::Else) {
            self.advance()?;
            Some(Box::new(self.parse_statement()?))
        } else {
            None
        };
        self.depth = saved;
        Ok(StmtKind::If {
            test,
            consequent,
            alternate,
        })
    }

    fn parse_while(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        self.deeper(cost::BODY)?;
        self.advance()?;
        let test = self.parse_paren_condition()?;
        let outer = std::mem::replace(&mut self.in_loop, true);
        let body = Box::new(self.parse_statement()?);
        self.in_loop = outer;
        self.depth = saved;
        Ok(StmtKind::While { test, body })
    }

    /// DoWhileStatement: `do` Statement `while` `(` Expression `)` `;`
    /// (clj-quickjs-80). The `;` is consumed if present and otherwise always
    /// inserted, even without a line break (the ECMAScript ASI rule for
    /// `do`-`while`; QuickJS `TOK_DO` in `js_parse_statement_or_decl`), so
    /// `do x; while (0) y;` is two statements.
    fn parse_do_while(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        self.deeper(cost::BODY)?;
        self.advance()?;
        let outer = std::mem::replace(&mut self.in_loop, true);
        let body = Box::new(self.parse_statement()?);
        self.in_loop = outer;
        if !self.is_keyword(Keyword::While) {
            return Err(self.unexpected("`while`"));
        }
        self.advance()?;
        let test = self.parse_paren_condition()?;
        self.eat_punct(Punct::Semicolon)?;
        self.depth = saved;
        Ok(StmtKind::DoWhile { body, test })
    }

    fn parse_return(&mut self) -> PResult<StmtKind> {
        let keyword = self.advance()?;
        if !self.in_function {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                keyword.span,
                "`return` outside of a function",
            ));
        }
        // Restricted production: `return [no LineTerminator here] Expression`.
        let argument = if self.is_punct(Punct::Semicolon)
            || self.is_punct(Punct::RBrace)
            || self.tok.kind == TokenKind::Eof
            || self.tok.newline_before
        {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_semi()?;
        Ok(StmtKind::Return(argument))
    }

    /// ThrowStatement: `throw` [no LineTerminator here] Expression `;`
    fn parse_throw(&mut self) -> PResult<StmtKind> {
        let keyword = self.advance()?;
        if self.tok.newline_before && self.tok.kind != TokenKind::Eof {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                keyword.span,
                "a line break is not allowed between `throw` and its expression",
            ));
        }
        let argument = self.parse_expression()?;
        self.expect_semi()?;
        Ok(StmtKind::Throw(argument))
    }

    /// BreakStatement: `break` `;` and ContinueStatement: `continue` `;`,
    /// unlabelled. Restricted productions (`break` [no LineTerminator here]
    /// LabelIdentifier): a line break after the keyword ends the statement.
    /// Early error: outside a loop of the same function (QuickJS
    /// `emit_break`: "break must be inside loop or switch", "continue must
    /// be inside loop"; the subset has no `switch` yet).
    #[inline(never)]
    fn parse_break_continue(&mut self) -> PResult<StmtKind> {
        let keyword = self.advance()?;
        let (name, kind) = if keyword.kind == TokenKind::Keyword(Keyword::Break) {
            ("break", StmtKind::Break)
        } else {
            ("continue", StmtKind::Continue)
        };
        if matches!(self.tok.kind, TokenKind::Identifier(_)) && !self.tok.newline_before {
            return Err(self.unsupported(
                keyword.span.to(self.tok.span),
                &format!("labelled `{name}` is"),
            ));
        }
        if !self.in_loop {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                keyword.span,
                format!("`{name}` must be inside a loop of the same function"),
            ));
        }
        self.expect_semi()?;
        Ok(kind)
    }

    /// TryStatement: `try` Block (Catch | Finally | Catch Finally), where
    /// Catch is `catch` (`(` BindingIdentifier `)`)? Block and Finally is
    /// `finally` Block.
    fn parse_try(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        self.deeper(cost::BODY)?;
        self.advance()?;
        let block = self.parse_block()?;
        let handler = if self.is_keyword(Keyword::Catch) {
            Some(self.parse_catch()?)
        } else {
            None
        };
        let finalizer = if self.is_keyword(Keyword::Finally) {
            self.advance()?;
            Some(self.parse_block()?)
        } else {
            None
        };
        if handler.is_none() && finalizer.is_none() {
            return Err(self.unexpected("`catch` or `finally`"));
        }
        self.depth = saved;
        Ok(StmtKind::Try {
            block,
            handler,
            finalizer,
        })
    }

    #[inline(never)]
    fn parse_catch(&mut self) -> PResult<CatchClause> {
        let start = self.advance()?.span.start;
        let param = if self.eat_punct(Punct::LParen)? {
            let param = self.parse_binding_identifier("a catch parameter")?;
            self.expect_punct(Punct::RParen)?;
            Some(param)
        } else {
            None
        };
        let body = self.parse_block()?;
        // Early error: the block may not redeclare the catch parameter.
        if let Some(param) = &param {
            let redeclared = body.iter().find_map(|s| match &s.kind {
                StmtKind::Lexical(decl) => decl
                    .declarators
                    .iter()
                    .map(|d| &d.name)
                    .find(|n| n.name == param.name),
                StmtKind::FunctionDecl(f) => f.name.as_ref().filter(|n| n.name == param.name),
                _ => None,
            });
            if let Some(name) = redeclared {
                return Err(self.error(
                    SyntaxErrorKind::Invalid,
                    name.span,
                    format!("'{}' is already declared as the catch parameter", name.name),
                ));
            }
        }
        Ok(CatchClause {
            param,
            body,
            span: self.span_from(start),
        })
    }

    /// LexicalDeclaration: (`let` | `const`) Declarator (`,` Declarator)* `;`
    fn parse_lexical_decl(&mut self) -> PResult<Stmt> {
        let start = self.tok.span.start;
        let kind = if self.advance()?.kind == TokenKind::Keyword(Keyword::Let) {
            LexicalKind::Let
        } else {
            LexicalKind::Const
        };
        let mut declarators = Vec::new();
        loop {
            let name = self.parse_binding_identifier("a variable name")?;
            let init = if self.eat_punct(Punct::Assign)? {
                Some(self.parse_assignment()?)
            } else {
                if kind == LexicalKind::Const {
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        name.span,
                        format!(
                            "missing initializer in `const` declaration of `{}`",
                            name.name
                        ),
                    ));
                }
                None
            };
            declarators.push(Declarator {
                span: self.span_from(name.span.start),
                name,
                init,
            });
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.expect_semi()?;
        self.bounded(Stmt::new(
            StmtKind::Lexical(LexicalDecl { kind, declarators }),
            self.span_from(start),
        ))
    }

    /// BindingIdentifier: an identifier that is not a reserved word, a
    /// reserved constant, `eval` or `arguments`.
    fn parse_binding_identifier(&mut self, what: &str) -> PResult<Ident> {
        match &self.tok.kind {
            TokenKind::Identifier(name) => {
                if RESERVED_CONSTANTS.contains(&name.as_str()) {
                    return Err(self.error(
                        SyntaxErrorKind::ReservedWord,
                        self.tok.span,
                        format!("`{name}` is reserved by this subset and cannot be rebound"),
                    ));
                }
                if STRICT_RESTRICTED.contains(&name.as_str()) {
                    return Err(self.error(
                        SyntaxErrorKind::ReservedWord,
                        self.tok.span,
                        format!("`{name}` cannot be used as a binding name in strict mode"),
                    ));
                }
                let name = name.clone();
                let span = self.advance()?.span;
                Ok(Ident { name, span })
            }
            TokenKind::Keyword(k) => Err(self.error(
                SyntaxErrorKind::ReservedWord,
                self.tok.span,
                format!(
                    "`{}` is a reserved word and cannot be used as {what}",
                    k.as_str()
                ),
            )),
            TokenKind::Punct(Punct::LBracket | Punct::LBrace) => {
                Err(self.unsupported(self.tok.span, "destructuring patterns are"))
            }
            _ => Err(self.unexpected(what)),
        }
    }

    /// Function declaration or expression, from the `function` keyword:
    /// `function` Name? `(` Params `)` `{` FunctionBody `}`.
    fn parse_function(&mut self, declaration: bool) -> PResult<Function> {
        let saved = self.depth;
        self.deeper(cost::FUNCTION)?;
        let start = self.tok.span.start;
        let (name, params) = self.parse_function_head(declaration)?;
        let outer = std::mem::replace(&mut self.in_function, true);
        let outer_loop = std::mem::replace(&mut self.in_loop, false);
        let body = self.parse_block()?;
        self.in_function = outer;
        self.in_loop = outer_loop;
        self.depth = saved;
        let function = Function::new(
            FunctionKind::Ordinary,
            name,
            params,
            body,
            self.span_from(start),
        );
        self.check_height(function.height)?;
        Ok(function)
    }

    /// `function` Name? `(` Params `)`, up to the body.
    #[inline(never)]
    fn parse_function_head(&mut self, declaration: bool) -> PResult<(Option<Ident>, Vec<Ident>)> {
        let start = self.advance()?.span.start;
        if self.is_punct(Punct::Star) {
            return Err(self.unsupported(self.span_from(start), "generator functions are"));
        }
        let name = if declaration || !self.is_punct(Punct::LParen) {
            Some(self.parse_binding_identifier("a function name")?)
        } else {
            None
        };
        let params = self.parse_formal_parameters()?;
        Ok((name, params))
    }

    /// `(` [ BindingIdentifier { `,` BindingIdentifier } [ `,` ] ] `)`:
    /// distinct plain identifiers (strict mode; always so for an arrow).
    fn parse_formal_parameters(&mut self) -> PResult<Vec<Ident>> {
        self.expect_punct(Punct::LParen)?;
        let mut params: Vec<Ident> = Vec::new();
        while !self.is_punct(Punct::RParen) {
            if self.is_punct(Punct::Ellipsis) {
                return Err(self.unsupported(self.tok.span, "rest parameters are"));
            }
            let param = self.parse_binding_identifier("a parameter name")?;
            if self.is_punct(Punct::Assign) {
                return Err(self.unsupported(self.tok.span, "default parameter values are"));
            }
            if params.iter().any(|p| p.name == param.name) {
                return Err(self.error(
                    SyntaxErrorKind::Invalid,
                    param.span,
                    format!("duplicate parameter name `{}`", param.name),
                ));
            }
            params.push(param);
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(params)
    }

    /// ArrowFunction: ArrowParameters [no LineTerminator here] `=>`
    /// ConciseBody, where ArrowParameters is a BindingIdentifier or a
    /// parenthesized parameter list and ConciseBody is a function body block
    /// or an AssignmentExpression. Called only where [`Self::at_arrow`]
    /// holds, at the start of an AssignmentExpression.
    #[inline(never)]
    fn parse_arrow_function(&mut self) -> PResult<Expr> {
        let saved = self.depth;
        self.deeper(cost::FUNCTION)?;
        let start = self.tok.span.start;
        let params = if self.is_punct(Punct::LParen) {
            self.parse_formal_parameters()?
        } else {
            vec![self.parse_binding_identifier("a parameter name")?]
        };
        if !self.is_punct(Punct::Arrow) {
            return Err(self.unexpected("`=>`"));
        }
        if self.tok.newline_before {
            return Err(self.misplaced_arrow(&self.tok));
        }
        self.advance()?;
        let (kind, body) = if self.is_punct(Punct::LBrace) {
            let outer = std::mem::replace(&mut self.in_function, true);
            let outer_loop = std::mem::replace(&mut self.in_loop, false);
            let body = self.parse_block()?;
            self.in_function = outer;
            self.in_loop = outer_loop;
            (FunctionKind::Arrow, body)
        } else {
            // The body is an AssignmentExpression, so the arrow ends with
            // it: a `,` after it separates arguments or declarators, or is
            // the comma operator of an enclosing Expression.
            let value = self.parse_assignment()?;
            let span = value.span;
            let body = self.bounded(Stmt::new(StmtKind::Return(Some(value)), span))?;
            (FunctionKind::ConciseArrow, vec![body])
        };
        self.depth = saved;
        let function = Function::new(kind, None, params, body, self.span_from(start));
        self.check_height(function.height)?;
        let span = function.span;
        self.expr(ExprKind::Function(Box::new(function)), span)
    }

    /// True when the current token starts an arrow function: an identifier
    /// followed by `=>`, or a `(` whose matching `)` is followed by `=>`
    /// (the cover grammar: such a parenthesized list is a parameter list,
    /// never an expression). Lookahead only.
    #[inline(never)]
    fn at_arrow(&self) -> bool {
        match &self.tok.kind {
            TokenKind::Identifier(_) => self
                .peek_next()
                .is_some_and(|t| t.kind == TokenKind::Punct(Punct::Arrow)),
            TokenKind::Punct(Punct::LParen) => self.arrow_after_parens().is_some(),
            _ => false,
        }
    }

    // ----- expressions --------------------------------------------------

    /// Expression: AssignmentExpression (`,` AssignmentExpression)*, the
    /// comma operator, left-associative. Each `,` is charged like one link
    /// of a binary-operator chain. Only callers whose grammar position is
    /// Expression use this; argument lists, declarators, `? :` branches and
    /// concise arrow bodies take an AssignmentExpression.
    fn parse_expression(&mut self) -> PResult<Expr> {
        let saved = self.depth;
        let mut expr = self.parse_assignment()?;
        while self.is_punct(Punct::Comma) {
            self.deeper(cost::OPERATOR)?;
            self.advance()?;
            let right = self.parse_assignment()?;
            let span = expr.span.to(right.span);
            expr = self.expr(
                ExprKind::Sequence {
                    left: Box::new(expr),
                    right: Box::new(right),
                },
                span,
            )?;
        }
        self.depth = saved;
        Ok(expr)
    }

    /// AssignmentExpression: ConditionalExpression, ArrowFunction, or
    /// Identifier `=` AssignmentExpression (right-associative).
    fn parse_assignment(&mut self) -> PResult<Expr> {
        let saved = self.depth;
        self.deeper(cost::ASSIGNMENT)?;
        if self.at_arrow() {
            let arrow = self.parse_arrow_function()?;
            self.depth = saved;
            return Ok(arrow);
        }
        let left = self.parse_conditional()?;
        if !self.is_punct(Punct::Assign) {
            if let Some(error) = self.assignment_operator_error(&left) {
                return Err(error);
            }
            self.depth = saved;
            return Ok(left);
        }
        let target = self.simple_target(&left, TargetUse::Assign)?;
        self.advance()?;
        let value = self.parse_assignment()?;
        self.depth = saved;
        let span = left.span.to(value.span);
        self.expr(
            ExprKind::Assign {
                target,
                value: Box::new(value),
            },
            span,
        )
    }

    /// Rejects compound assignment and an arrow after an expression.
    #[inline(never)]
    fn assignment_operator_error(&self, left: &Expr) -> Option<SyntaxError> {
        match &self.tok.kind {
            TokenKind::Punct(p) if is_compound_assign(*p) => Some(self.unsupported(
                self.tok.span,
                &format!("compound assignment `{}` is", p.as_str()),
            )),
            TokenKind::Punct(Punct::Arrow) if is_async_call(left) => {
                Some(self.unsupported(left.span.to(self.tok.span), "async arrow functions are"))
            }
            TokenKind::Punct(Punct::Arrow) => Some(self.misplaced_arrow(&self.tok)),
            _ => None,
        }
    }

    /// `arrow` is a `=>` token that cannot start an arrow function's body:
    /// its parameters are not an identifier or a parenthesized list
    /// at the start of an AssignmentExpression (an arrow function cannot be
    /// an operand, callee or member object without parentheses), or a line
    /// break precedes it.
    #[inline(never)]
    fn misplaced_arrow(&self, arrow: &Token) -> SyntaxError {
        let message = if arrow.newline_before {
            "a line break is not allowed before `=>`"
        } else {
            "unexpected `=>`: an arrow function's parameters must be an identifier or a parenthesized list, and an arrow function used as an operand must be parenthesized"
        };
        self.error(SyntaxErrorKind::UnexpectedToken, arrow.span, message)
    }

    /// The identifier binding `left` names, as the target of an assignment
    /// or of `++`/`--` (`use_`, which selects the diagnostics); anything
    /// else is rejected.
    #[inline(never)]
    fn simple_target(&self, left: &Expr, use_: TargetUse) -> PResult<Ident> {
        let text = strip_parens(left.span.text(self.src));
        let (verb, properties, invalid) = match use_ {
            TargetUse::Assign => (
                "assigned",
                "assignment to properties is",
                "invalid assignment target",
            ),
            TargetUse::Update => (
                "incremented or decremented",
                "increment and decrement of properties are",
                "invalid increment/decrement operand",
            ),
        };
        match &left.kind {
            ExprKind::Identifier(name) => Ok(Ident {
                name: name.clone(),
                span: left.span,
            }),
            ExprKind::Member { object, .. } if is_reserved_binding(object) => Err(self.error(
                SyntaxErrorKind::ReservedWord,
                left.span,
                format!("intrinsics of the reserved binding `{RESERVED_BINDING}` cannot be {verb}"),
            )),
            ExprKind::Member { .. } => Err(self.unsupported(left.span, properties)),
            ExprKind::Undefined | ExprKind::Number(_) if RESERVED_CONSTANTS.contains(&text) => {
                Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    left.span,
                    format!("`{text}` is reserved by this subset and cannot be {verb}"),
                ))
            }
            _ => Err(self.error(SyntaxErrorKind::Invalid, left.span, invalid)),
        }
    }

    /// The UpdateExpression `++operand` / `operand++` (or `--`): the
    /// operand must be an identifier binding, possibly parenthesized
    /// (property targets are not supported yet), else the error
    /// [`Self::simple_target`] gives.
    #[inline(never)]
    fn update(&self, op: UpdateOp, prefix: bool, operand: Expr, op_span: Span) -> PResult<Expr> {
        let target = self.simple_target(&operand, TargetUse::Update)?;
        self.expr(
            ExprKind::Update {
                op,
                prefix,
                target,
                op_span,
            },
            op_span.to(operand.span),
        )
    }

    /// ConditionalExpression: LogicalOr (`?` Assignment `:` Assignment)?
    fn parse_conditional(&mut self) -> PResult<Expr> {
        let test = self.parse_binary(1)?;
        if !self.is_punct(Punct::Question) {
            return Ok(test);
        }
        let saved = self.depth;
        self.deeper(cost::CONDITIONAL)?;
        self.advance()?;
        let consequent = self.parse_assignment()?;
        self.expect_punct(Punct::Colon)?;
        let alternate = self.parse_assignment()?;
        self.depth = saved;
        let span = test.span.to(alternate.span);
        self.expr(
            ExprKind::Conditional {
                test: Box::new(test),
                consequent: Box::new(consequent),
                alternate: Box::new(alternate),
            },
            span,
        )
    }

    /// Binary operators by precedence climbing; all are left-associative.
    /// (`**` binds tighter than all of them and is right-associative; see
    /// [`Self::parse_exponent`].)
    fn parse_binary(&mut self, min_prec: u8) -> PResult<Expr> {
        let saved = self.depth;
        let mut left = self.parse_exponent()?;
        loop {
            self.reject_unsupported_binary()?;
            let Some((prec, op)) = binary_op(&self.tok.kind) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.deeper(cost::OPERATOR)?;
            self.advance()?;
            let right = self.parse_binary(prec + 1)?;
            left = make_binary(op, left, right);
            self.check_height(left.height)?;
        }
        self.depth = saved;
        Ok(left)
    }

    #[inline(never)]
    fn reject_unsupported_binary(&self) -> PResult<()> {
        let what = match &self.tok.kind {
            TokenKind::Punct(p @ Punct::Nullish) => p.as_str(),
            TokenKind::Keyword(k @ (Keyword::In | Keyword::Instanceof)) => k.as_str(),
            _ => return Ok(()),
        };
        Err(self.unsupported(self.tok.span, &format!("the `{what}` operator is")))
    }

    /// ExponentiationExpression:
    ///   UnaryExpression | CallExpression `**` ExponentiationExpression
    ///
    /// Right-associative, binding tighter than every other binary operator.
    /// The left operand may not be an unparenthesized prefix-operator
    /// expression: `-a ** b` is a SyntaxError (reported by
    /// [`Self::parse_unary`] at the `**`), while `(-a) ** b`, `-(a ** b)`
    /// and `a ** -b` are valid.
    fn parse_exponent(&mut self) -> PResult<Expr> {
        let base = self.parse_unary()?;
        if !self.is_punct(Punct::StarStar) {
            return Ok(base);
        }
        let saved = self.depth;
        self.deeper(cost::OPERATOR)?;
        self.advance()?;
        let exponent = self.parse_exponent()?;
        self.depth = saved;
        let span = base.span.to(exponent.span);
        self.expr(
            ExprKind::Binary {
                op: BinaryOp::Exp,
                left: Box::new(base),
                right: Box::new(exponent),
            },
            span,
        )
    }

    /// UnaryExpression: (`!` | `-` | `+` | `~`) UnaryExpression
    ///   | (`++` | `--`) UnaryExpression | PostfixExpression
    ///
    /// A prefix-operator expression followed by `**` is rejected here, at
    /// the `**`: ECMAScript does not let a UnaryExpression be the base of
    /// an exponentiation (`-2 ** 2` is ambiguous to a reader). A prefix
    /// `++`/`--` expression is an UpdateExpression, which may be one
    /// (`++a ** 2`); its operand must be an identifier binding
    /// ([`Self::update`]).
    fn parse_unary(&mut self) -> PResult<Expr> {
        let op = match &self.tok.kind {
            TokenKind::Punct(Punct::Bang) => Prefix::Unary(UnaryOp::Not),
            TokenKind::Punct(Punct::Minus) => Prefix::Unary(UnaryOp::Neg),
            TokenKind::Punct(Punct::Plus) => Prefix::Unary(UnaryOp::Plus),
            TokenKind::Punct(Punct::Tilde) => Prefix::Unary(UnaryOp::BitNot),
            TokenKind::Punct(Punct::PlusPlus) => Prefix::Update(UpdateOp::Inc),
            TokenKind::Punct(Punct::MinusMinus) => Prefix::Update(UpdateOp::Dec),
            _ => {
                self.reject_unsupported_prefix()?;
                return self.parse_call();
            }
        };
        let saved = self.depth;
        self.deeper(cost::OPERATOR)?;
        let op_span = self.advance()?.span;
        let operand = self.parse_unary()?;
        self.depth = saved;
        let op = match op {
            Prefix::Unary(op) => op,
            Prefix::Update(op) => return self.update(op, true, operand, op_span),
        };
        if self.is_punct(Punct::StarStar) {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                self.tok.span,
                "a unary expression cannot be the left operand of `**`; parenthesize it",
            ));
        }
        let span = op_span.to(operand.span);
        self.expr(
            ExprKind::Unary {
                op,
                operand: Box::new(operand),
            },
            span,
        )
    }

    #[inline(never)]
    fn reject_unsupported_prefix(&self) -> PResult<()> {
        let span = self.tok.span;
        match &self.tok.kind {
            TokenKind::Keyword(k @ (Keyword::Typeof | Keyword::Void | Keyword::Delete)) => {
                Err(self.unsupported(span, &format!("the `{}` operator is", k.as_str())))
            }
            TokenKind::Keyword(Keyword::Await) => {
                Err(self.unsupported(span, "`await` (async functions) is"))
            }
            _ => Ok(()),
        }
    }

    /// PostfixExpression: CallExpression [no LineTerminator here] (`++` | `--`)?
    /// CallExpression: Primary (Arguments | `.` IdentifierName)*
    ///
    /// Both are parsed here. A postfix update ends the expression: its
    /// value is not a callee or member object (`a++()` is a syntax error).
    fn parse_call(&mut self) -> PResult<Expr> {
        let saved = self.depth;
        let mut expr = self.parse_primary()?;
        loop {
            if self.is_punct(Punct::LParen) {
                self.deeper(cost::CALL)?;
                let args = self.parse_arguments()?;
                let span = self.span_from(expr.span.start);
                expr = self.expr(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                    },
                    span,
                )?;
            } else if self.is_punct(Punct::Dot) {
                self.deeper(cost::OPERATOR)?;
                let property = self.parse_property_name()?;
                let span = expr.span.to(property.span);
                expr = self.expr(
                    ExprKind::Member {
                        object: Box::new(expr),
                        property,
                    },
                    span,
                )?;
            } else {
                if let Some(op) = self.postfix_update_op() {
                    let op_span = self.advance()?.span;
                    expr = self.update(op, false, expr, op_span)?;
                } else {
                    self.reject_unsupported_postfix(&expr)?;
                }
                break;
            }
        }
        self.depth = saved;
        Ok(expr)
    }

    /// The operator of a postfix `++`/`--` at the current token. Postfix
    /// `++`/`--` is a restricted production: after a line break it starts
    /// the next statement instead (`a` newline `++b` is `a; ++b;`).
    fn postfix_update_op(&self) -> Option<UpdateOp> {
        match self.tok.kind {
            _ if self.tok.newline_before => None,
            TokenKind::Punct(Punct::PlusPlus) => Some(UpdateOp::Inc),
            TokenKind::Punct(Punct::MinusMinus) => Some(UpdateOp::Dec),
            _ => None,
        }
    }

    /// `.` IdentifierName (reserved words allowed).
    #[inline(never)]
    fn parse_property_name(&mut self) -> PResult<Ident> {
        self.advance()?;
        let name = match &self.tok.kind {
            TokenKind::Identifier(name) => name.clone(),
            TokenKind::Keyword(k) => k.as_str().to_string(),
            _ => return Err(self.unexpected("a property name after `.`")),
        };
        let span = self.advance()?.span;
        Ok(Ident { name, span })
    }

    #[inline(never)]
    fn reject_unsupported_postfix(&self, object: &Expr) -> PResult<()> {
        let span = self.tok.span;
        match &self.tok.kind {
            TokenKind::Punct(Punct::LBracket) if is_reserved_binding(object) => Err(self.error(
                SyntaxErrorKind::ReservedWord,
                span,
                format!(
                    "computed access to the reserved intrinsic binding `{RESERVED_BINDING}` is not allowed: name the intrinsic statically, as `{RESERVED_BINDING}.name(...)`"
                ),
            )),
            TokenKind::Punct(Punct::LBracket) => {
                Err(self.unsupported(span, "computed member access is"))
            }
            TokenKind::Punct(Punct::OptionalChain) => {
                Err(self.unsupported(span, "optional chaining is"))
            }
            _ => Ok(()),
        }
    }

    /// Arguments: `(` (AssignmentExpression (`,` AssignmentExpression)* `,`?)? `)`
    fn parse_arguments(&mut self) -> PResult<Vec<Expr>> {
        self.expect_punct(Punct::LParen)?;
        let mut args = Vec::new();
        while !self.is_punct(Punct::RParen) {
            if self.is_punct(Punct::Ellipsis) {
                return Err(self.unsupported(self.tok.span, "spread arguments are"));
            }
            args.push(self.parse_assignment()?);
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.expect_punct(Punct::RParen)?;
        Ok(args)
    }

    fn parse_primary(&mut self) -> PResult<Expr> {
        let span = self.tok.span;
        let kind = match &self.tok.kind {
            TokenKind::Number(n) => ExprKind::Number(*n),
            TokenKind::String(s) => ExprKind::String(s.clone()),
            TokenKind::Keyword(Keyword::True) => ExprKind::Bool(true),
            TokenKind::Keyword(Keyword::False) => ExprKind::Bool(false),
            TokenKind::Keyword(Keyword::Null) => ExprKind::Null,
            TokenKind::Keyword(Keyword::Function) => {
                let function = self.parse_function(false)?;
                let span = function.span;
                return self.expr(ExprKind::Function(Box::new(function)), span);
            }
            TokenKind::Identifier(_) => return self.parse_identifier_reference(),
            TokenKind::Punct(Punct::LParen) => return self.parse_parenthesized(),
            _ => return Err(self.primary_error()),
        };
        self.advance()?;
        Ok(Expr::new(kind, span))
    }

    /// An identifier in expression position, including the reserved
    /// constants and the `async` / arrow rejections.
    #[inline(never)]
    fn parse_identifier_reference(&mut self) -> PResult<Expr> {
        let span = self.tok.span;
        let TokenKind::Identifier(name) = &self.tok.kind else {
            return Err(self.unexpected("an identifier"));
        };
        let kind = match name.as_str() {
            "undefined" => ExprKind::Undefined,
            "NaN" => ExprKind::Number(f64::NAN),
            "Infinity" => ExprKind::Number(f64::INFINITY),
            "eval" => return Err(self.unsupported(span, "`eval` is")),
            "arguments" => return Err(self.unsupported(span, "the `arguments` object is")),
            "async" => {
                let next = self.peek_next().filter(|t| !t.newline_before);
                match next.map(|t| t.kind) {
                    Some(TokenKind::Keyword(Keyword::Function)) => {
                        return Err(self.unsupported(span, "async functions are"));
                    }
                    Some(TokenKind::Identifier(_)) => {
                        return Err(self.unsupported(span, "async arrow functions are"));
                    }
                    _ => ExprKind::Identifier(name.clone()),
                }
            }
            _ => ExprKind::Identifier(name.clone()),
        };
        self.advance()?;
        Ok(Expr::new(kind, span))
    }

    /// The current token cannot start a supported expression.
    #[inline(never)]
    fn primary_error(&self) -> SyntaxError {
        let span = self.tok.span;
        match &self.tok.kind {
            TokenKind::Punct(Punct::Slash | Punct::SlashAssign) => {
                self.unsupported(span, "regular expression literals are")
            }
            TokenKind::Punct(Punct::LBrace) => self.unsupported(span, "object literals are"),
            TokenKind::Punct(Punct::LBracket) => self.unsupported(span, "array literals are"),
            TokenKind::Punct(Punct::Ellipsis) => self.unsupported(span, "spread syntax is"),
            TokenKind::Keyword(k) => {
                if let Some(what) = unsupported_expression_keyword(*k) {
                    self.unsupported(span, what)
                } else if is_strict_reserved(*k) {
                    self.error(
                        SyntaxErrorKind::ReservedWord,
                        span,
                        format!(
                            "`{}` is a reserved word and cannot be used as an identifier",
                            k.as_str()
                        ),
                    )
                } else {
                    self.unexpected("an expression")
                }
            }
            _ => self.unexpected("an expression"),
        }
    }

    /// `(` Expression `)`; rejects an arrow function in operand position.
    fn parse_parenthesized(&mut self) -> PResult<Expr> {
        let start = self.tok.span.start;
        let saved = self.depth;
        self.deeper(cost::PAREN)?;
        self.open_paren()?;
        let mut inner = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        self.depth = saved;
        inner.span = self.span_from(start);
        Ok(inner)
    }

    /// Consume the `(` of a parenthesized expression, rejecting an arrow
    /// parameter list (an arrow function reaching here is an operand, callee
    /// or member object: [`Self::parse_assignment`] takes every arrow at the
    /// start of an AssignmentExpression) and empty parentheses.
    #[inline(never)]
    fn open_paren(&mut self) -> PResult<()> {
        if let Some(arrow) = self.arrow_after_parens() {
            return Err(self.misplaced_arrow(&arrow));
        }
        self.advance()?;
        if self.is_punct(Punct::RParen) {
            return Err(self.unexpected("an expression"));
        }
        Ok(())
    }

    /// With the current token `(`, the `=>` token following the matching
    /// `)`, if any. Lookahead only; a lexical error ends the search (the
    /// normal parse then reports it at its position).
    fn arrow_after_parens(&self) -> Option<Token> {
        let mut lexer = self.lexer.clone();
        let mut open = 1usize;
        while open > 0 {
            match lexer.next_token().ok()?.kind {
                TokenKind::Punct(Punct::LParen | Punct::LBracket | Punct::LBrace) => open += 1,
                TokenKind::Punct(Punct::RParen | Punct::RBracket | Punct::RBrace) => open -= 1,
                TokenKind::Eof => return None,
                _ => {}
            }
        }
        let next = lexer.next_token().ok()?;
        (next.kind == TokenKind::Punct(Punct::Arrow)).then_some(next)
    }
}

/// True for `async(...)`: before `=>` it is an async arrow function's head.
fn is_async_call(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Call { callee, .. }
        if matches!(&callee.kind, ExprKind::Identifier(name) if name == "async"))
}

/// A prefix operator: a UnaryExpression operator or `++`/`--`.
enum Prefix {
    Unary(UnaryOp),
    Update(UpdateOp),
}

/// What an identifier target is used for (it selects the diagnostics).
#[derive(Clone, Copy)]
enum TargetUse {
    Assign,
    Update,
}

enum BinOrLogical {
    Binary(BinaryOp),
    Logical(LogicalOp),
}

/// Precedence (higher binds tighter) of the supported binary operators.
fn binary_op(kind: &TokenKind) -> Option<(u8, BinOrLogical)> {
    use BinOrLogical::{Binary, Logical};
    let TokenKind::Punct(p) = kind else {
        return None;
    };
    Some(match p {
        Punct::PipePipe => (1, Logical(LogicalOp::Or)),
        Punct::AmpAmp => (2, Logical(LogicalOp::And)),
        Punct::Pipe => (3, Binary(BinaryOp::BitOr)),
        Punct::Caret => (4, Binary(BinaryOp::BitXor)),
        Punct::Amp => (5, Binary(BinaryOp::BitAnd)),
        Punct::Eq => (6, Binary(BinaryOp::LooseEq)),
        Punct::NotEq => (6, Binary(BinaryOp::LooseNe)),
        Punct::StrictEq => (6, Binary(BinaryOp::StrictEq)),
        Punct::StrictNotEq => (6, Binary(BinaryOp::StrictNe)),
        Punct::Lt => (7, Binary(BinaryOp::Lt)),
        Punct::Gt => (7, Binary(BinaryOp::Gt)),
        Punct::Le => (7, Binary(BinaryOp::Le)),
        Punct::Ge => (7, Binary(BinaryOp::Ge)),
        Punct::Shl => (8, Binary(BinaryOp::Shl)),
        Punct::Shr => (8, Binary(BinaryOp::Sar)),
        Punct::UShr => (8, Binary(BinaryOp::UShr)),
        Punct::Plus => (9, Binary(BinaryOp::Add)),
        Punct::Minus => (9, Binary(BinaryOp::Sub)),
        Punct::Star => (10, Binary(BinaryOp::Mul)),
        Punct::Slash => (10, Binary(BinaryOp::Div)),
        Punct::Percent => (10, Binary(BinaryOp::Rem)),
        _ => return None,
    })
}

fn make_binary(op: BinOrLogical, left: Expr, right: Expr) -> Expr {
    let span = left.span.to(right.span);
    let (left, right) = (Box::new(left), Box::new(right));
    let kind = match op {
        BinOrLogical::Binary(op) => ExprKind::Binary { op, left, right },
        BinOrLogical::Logical(op) => ExprKind::Logical { op, left, right },
    };
    Expr::new(kind, span)
}

fn is_compound_assign(p: Punct) -> bool {
    matches!(
        p,
        Punct::PlusAssign
            | Punct::MinusAssign
            | Punct::StarAssign
            | Punct::SlashAssign
            | Punct::PercentAssign
            | Punct::StarStarAssign
            | Punct::ShlAssign
            | Punct::ShrAssign
            | Punct::UShrAssign
            | Punct::AmpAssign
            | Punct::PipeAssign
            | Punct::CaretAssign
            | Punct::AmpAmpAssign
            | Punct::PipePipeAssign
            | Punct::NullishAssign
    )
}

/// Statements of JavaScript that the subset rejects, keyed by their first
/// keyword. (`let`, `const` and `function` are handled by the caller.)
fn unsupported_statement(k: Keyword) -> Option<&'static str> {
    Some(match k {
        Keyword::Var => "`var` declarations are (use `let` or `const`)",
        Keyword::For => "`for` loops are (use `while`)",
        Keyword::Switch => "`switch` is",
        Keyword::Class => "classes are",
        Keyword::Import => "modules (`import`) are",
        Keyword::Export => "modules (`export`) are",
        Keyword::With => "`with` is",
        Keyword::Debugger => "`debugger` is",
        _ => return None,
    })
}

/// Expression-starting keywords of JavaScript that the subset rejects.
fn unsupported_expression_keyword(k: Keyword) -> Option<&'static str> {
    Some(match k {
        Keyword::Class => "classes are",
        Keyword::New => "the `new` operator is",
        Keyword::This => "`this` is",
        Keyword::Super => "`super` is",
        Keyword::Import => "dynamic `import` is",
        _ => return None,
    })
}

/// Words reserved only in strict mode (or by this subset); a use as an
/// identifier is a reserved-word error rather than a plain syntax error.
fn is_strict_reserved(k: Keyword) -> bool {
    matches!(
        k,
        Keyword::Let
            | Keyword::Static
            | Keyword::Yield
            | Keyword::Implements
            | Keyword::Interface
            | Keyword::Package
            | Keyword::Private
            | Keyword::Protected
            | Keyword::Public
            | Keyword::Enum
    )
}

fn strip_parens(text: &str) -> &str {
    text.trim_matches(|c: char| c == '(' || c == ')' || c.is_whitespace())
}

/// True for a reference to the compiler-reserved intrinsic binding
/// (`super::intrinsics`), which no program can shadow.
fn is_reserved_binding(e: &Expr) -> bool {
    matches!(&e.kind, ExprKind::Identifier(name) if name == RESERVED_BINDING)
}
