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
    BinaryOp, CatchClause, Declarator, Expr, ExprKind, ForInHead, ForInit, Function, FunctionKind,
    Ident, LexicalDecl, LexicalKind, LogicalOp, PropertyDef, PropertyName, Script, Stmt, StmtKind,
    SwitchCase, TemplateElement, UnaryOp, UpdateOp,
};
use super::intrinsics::RESERVED_BINDING;
use super::lexer::{
    Keyword, Lexer, Punct, Token, TokenKind, is_leading_zero_literal, leading_zero_in_strict_code,
    legacy_escape_in_strict_code,
};
use super::{MAX_NESTING, Span, SyntaxError, SyntaxErrorKind};

type PResult<T> = Result<T, SyntaxError>;

/// A formal parameter list: the names, and per name its default value, if
/// any.
type Params = (Vec<Ident>, Vec<Option<Expr>>);

/// Nesting-budget cost of each construct, per nesting level. Weighted by the
/// measured Rust stack each one uses, so that the whole [`MAX_NESTING`]
/// budget fits a 2 MiB thread stack even in an unoptimized build, when
/// parsing and when lowering (checked for every accepted depth by
/// `frontend::tests::nesting_at_the_limit_fits_a_2_mib_stack`; the
/// lowering's per-level frames are kept below the parser's,
/// clj-quickjs-262).
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
    /// `{ ... }` object literal (on top of each value's `ASSIGNMENT`).
    pub const OBJECT: usize = 3;
    /// A template literal (on top of each substitution's `ASSIGNMENT`).
    pub const TEMPLATE: usize = 3;
    /// `[ ... ]` array literal (on top of each element's `ASSIGNMENT`).
    pub const ARRAY: usize = 3;
}

/// Global names the subset treats as reserved constants: they parse as
/// literals and cannot be bound or assigned.
const RESERVED_CONSTANTS: &[&str] = &["undefined", "NaN", "Infinity"];

/// Names strict mode forbids as binding names (sloppy code may bind them,
/// clj-quickjs-95); strict code cannot reference them in this subset
/// (see [`Parser::parse_identifier_reference`]).
const STRICT_RESTRICTED: &[&str] = &["eval", "arguments"];

/// The exact source text of a `"use strict"` directive: no escape sequence
/// or line continuation (ECMAScript "Use Strict Directive").
const USE_STRICT: [&str; 2] = ["\"use strict\"", "'use strict'"];

pub(super) struct Parser<'a> {
    src: &'a str,
    lexer: Lexer<'a>,
    /// The current (not yet consumed) token.
    tok: Token,
    /// End offset of the last consumed token.
    prev_end: usize,
    depth: usize,
    /// The code being parsed is strict-mode code (clj-quickjs-95): the
    /// script was parsed strict, or a `"use strict"` directive of the
    /// script or of an enclosing function applies. The parser consults it
    /// for mode-dependent early errors; the result is recorded as
    /// [`Script::strict`] and [`Function::strict`]. The lexer lexes number
    /// literals by mode (clj-quickjs-51) and is kept in step with it
    /// ([`Self::set_strict`]).
    strict: bool,
    in_function: bool,
    /// `this` is an expression here (clj-quickjs-116): inside the parameters
    /// or body of a non-arrow function, including nested arrows whose
    /// `this` is lexical (clj-quickjs-98). Script-level arrows still reject
    /// it until the global object exists (clj-quickjs-104).
    this_allowed: bool,
    /// Inside the body of a loop of the current function (`break` and
    /// `continue` are allowed).
    in_loop: bool,
    /// Inside the case block of a `switch` of the current function
    /// (clj-quickjs-83): an unlabelled `break` is allowed.
    in_switch: bool,
    /// The labels of the enclosing labelled statements of the current
    /// function, outermost first.
    labels: Vec<LabelEntry>,
    /// While a statement of a sloppy directive prologue that starts with a
    /// `"use strict"` string token is parsed, that token's span: the
    /// statement may turn out to be the directive, which makes the token
    /// after it strict code (see [`Self::directive_lex_error`]).
    use_strict_candidate: Option<Span>,
    /// The token of the last reserved constant (`undefined`, `NaN`,
    /// `Infinity`) parsed as an identifier reference: these parse as
    /// literals, and `delete` must still tell them from a literal
    /// (clj-quickjs-124, [`Self::check_delete_operand`]).
    reserved_constant: Option<Span>,
    /// Parsing the init of a `for` head, outside any nested parentheses,
    /// brackets, braces, template substitution or function (ECMAScript's
    /// `[~In]` parameter): an `in` there ends the expression instead of
    /// being the `in` operator, so it can start a for-in head
    /// (clj-quickjs-74). Every construct that ECMAScript parses with
    /// `[+In]` clears it while it parses its contents.
    no_in: bool,
}

/// A label of an enclosing labelled statement (clj-quickjs-82).
struct LabelEntry {
    name: String,
    /// It labels an iteration statement (directly or through a label set
    /// `L1: L2: while ...`), so `continue` may target it.
    iteration: bool,
}

impl<'a> Parser<'a> {
    /// A parser of `src` whose top level is strict-mode code if `strict`.
    pub(super) fn new(src: &'a str, strict: bool) -> Parser<'a> {
        let mut lexer = Lexer::new(src);
        lexer.set_strict(strict);
        Parser {
            src,
            lexer,
            tok: Token {
                kind: TokenKind::Eof,
                span: Span::new(0, 0),
                newline_before: false,
            },
            prev_end: 0,
            depth: 0,
            strict,
            in_function: false,
            this_allowed: false,
            in_loop: false,
            in_switch: false,
            labels: Vec::new(),
            use_strict_candidate: None,
            reserved_constant: None,
            no_in: false,
        }
    }

    // ----- token helpers -------------------------------------------------

    /// Parse what follows as strict-mode code if `strict`, and have the
    /// lexer lex the tokens after the current one in the same mode. The
    /// current token was lexed in the previous mode; when the mode becomes
    /// strict at a directive, [`Self::directive`] checks it again.
    fn set_strict(&mut self, strict: bool) {
        self.strict = strict;
        self.lexer.set_strict(strict);
    }

    fn advance(&mut self) -> PResult<Token> {
        let before = self.use_strict_candidate.map(|_| self.lexer.clone());
        let next = match self.lexer.next_token() {
            Ok(next) => next,
            Err(e) => return Err(self.directive_lex_error(before, e)),
        };
        let tok = std::mem::replace(&mut self.tok, next);
        self.prev_end = tok.span.end;
        Ok(tok)
    }

    /// `e`, the error lexing the next token in sloppy code, or the strict
    /// one if that token is strict code (clj-quickjs-51, -258). It is when
    /// it follows a `"use strict"` directive
    /// ([`Self::use_strict_candidate`], whose statement the parser has not
    /// finished yet): the current token is the candidate string and a line
    /// break comes before the next token (a number or string literal there
    /// ends the statement), or it is the `;` right after the string.
    /// Lexing as strict and as sloppy code differ only at a leading-zero
    /// number literal or a legacy octal or `\8` `\9` string escape, where
    /// strict lexing stops with its early error, so a strict error that is
    /// not `e` is that early error, which comes first in the token.
    /// `before` is the lexer as it was before the failed token.
    #[cold]
    fn directive_lex_error(&self, before: Option<Lexer<'a>>, e: SyntaxError) -> SyntaxError {
        let (Some(mut lexer), Some(candidate)) = (before, self.use_strict_candidate) else {
            return e;
        };
        lexer.set_strict(true);
        let newline_before = lexer.newline_before_next() == Some(true);
        let Err(strict) = lexer.next_token() else {
            return e;
        };
        if strict == e {
            return e;
        }
        let ends_directive = if self.tok.span == candidate {
            newline_before
        } else {
            self.is_punct(Punct::Semicolon) && self.prev_end == candidate.end
        };
        if ends_directive { strict } else { e }
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

    /// Script: Directive* StatementListItem* (the directive prologue is
    /// recognised by [`Self::directive`]).
    pub(super) fn parse_script(mut self) -> PResult<Script> {
        self.advance()?;
        let mut body = Vec::new();
        let mut prologue = true;
        while self.tok.kind != TokenKind::Eof {
            let stmt = self.parse_body_item(prologue)?;
            prologue = prologue && self.directive(&stmt, &body, None, &[])?;
            body.push(stmt);
        }
        Ok(Script {
            body,
            span: Span::new(0, self.src.len()),
            strict: self.strict,
        })
    }

    /// Directive prologues (clj-quickjs-95; QuickJS `js_parse_directives`).
    /// `stmt` was parsed while the prologue of a script or function body
    /// may still continue: returns whether it does, that is whether `stmt`
    /// is an expression statement made of a single string literal token
    /// (not parenthesized, and not an operand: `"a" + b;` ends the
    /// prologue). The directive whose exact source text is
    /// [`USE_STRICT`] makes the code strict from here on. The function's
    /// `name` and `params` were parsed before its body, in the enclosing
    /// mode, so they are checked again now (QuickJS
    /// `js_parse_function_check_names`): `eval` or `arguments` there is a
    /// [`SyntaxErrorKind::ReservedWord`] at that name. The `earlier`
    /// statements of the body, all directives of the prologue, were lexed
    /// as sloppy code, so they are checked again too, and so is the token
    /// after `stmt` (clj-quickjs-51, -258). Directives are kept as
    /// ordinary expression statements in the tree.
    #[inline(never)]
    fn directive(
        &mut self,
        stmt: &Stmt,
        earlier: &[Stmt],
        name: Option<&Ident>,
        params: &[Ident],
    ) -> PResult<bool> {
        let StmtKind::Expr(Expr {
            kind: ExprKind::String(_),
            span,
            ..
        }) = &stmt.kind
        else {
            return Ok(false);
        };
        let text = span.text(self.src);
        if text.starts_with('(') {
            return Ok(false);
        }
        if USE_STRICT.contains(&text) && !self.strict {
            self.set_strict(true);
            if let Some(ident) = name
                .into_iter()
                .chain(params)
                .find(|i| STRICT_RESTRICTED.contains(&i.name.as_str()))
            {
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    ident.span,
                    format!(
                        "`{}` cannot be used as a binding name in strict mode (the function has a \"use strict\" directive)",
                        ident.name
                    ),
                ));
            }
            // The directives before it are strict code too (ECMAScript
            // 12.9.4.1 and B.1.2 early errors; QuickJS lexes the whole
            // prologue again in strict mode): `"\07"; "use strict";`.
            for earlier in earlier {
                if let StmtKind::Expr(Expr { span, .. }) = &earlier.kind
                    && let Some(e) = legacy_escape_in_strict_code(self.src, *span)
                {
                    return Err(e);
                }
            }
            // The token after the directive was lexed as sloppy code,
            // before the directive was recognised (`"use strict"; 010`,
            // `"use strict"; "\07"`).
            match self.tok.kind {
                TokenKind::Number(_) => {
                    let start = self.tok.span.start;
                    if is_leading_zero_literal(&self.src[start..]) {
                        return Err(leading_zero_in_strict_code(self.src, start));
                    }
                }
                TokenKind::String(_) => {
                    if let Some(e) = legacy_escape_in_strict_code(self.src, self.tok.span) {
                        return Err(e);
                    }
                }
                _ => {}
            }
        }
        Ok(true)
    }

    /// A StatementListItem of a script or function body, which is part of
    /// its directive prologue if `prologue` (the prologue has not ended).
    fn parse_body_item(&mut self, prologue: bool) -> PResult<Stmt> {
        let candidate = prologue
            && !self.strict
            && matches!(self.tok.kind, TokenKind::String(_))
            && USE_STRICT.contains(&self.tok.span.text(self.src));
        self.use_strict_candidate = candidate.then_some(self.tok.span);
        let stmt = self.parse_statement_list_item();
        self.use_strict_candidate = None;
        stmt
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

    /// Statement (single-statement position: `if`/`while`/`do`/`for` bodies).
    fn parse_statement(&mut self) -> PResult<Stmt> {
        let saved = self.depth;
        self.deeper(cost::STATEMENT)?;
        let start = self.tok.span.start;
        let kind = match &self.tok.kind {
            TokenKind::Punct(Punct::LBrace) => self.parse_block().map(StmtKind::Block),
            TokenKind::Punct(Punct::Semicolon) => self.advance().map(|_| StmtKind::Empty),
            TokenKind::Keyword(Keyword::If) => self.parse_if(),
            TokenKind::Keyword(Keyword::While) => self.parse_while(),
            TokenKind::Keyword(Keyword::Do) => self.parse_do_while(),
            TokenKind::Keyword(Keyword::For) => self.parse_for(),
            TokenKind::Keyword(Keyword::Var) => self.parse_var(),
            TokenKind::Keyword(Keyword::Return) => self.parse_return(),
            TokenKind::Keyword(Keyword::Throw) => self.parse_throw(),
            TokenKind::Keyword(Keyword::Try) => self.parse_try(),
            TokenKind::Keyword(Keyword::Switch) => self.parse_switch(),
            TokenKind::Keyword(Keyword::Break | Keyword::Continue) => self.parse_break_continue(),
            TokenKind::Identifier(_) if self.at_label() => self.parse_labelled(),
            _ => match self.statement_start_error() {
                Some(error) => Err(error),
                None => self.parse_expression_statement(),
            },
        }?;
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
                "a function declaration cannot be the body of `if`/`while`/`do`/`for` or a labelled statement; wrap it in a block",
            )),
            TokenKind::Keyword(k @ (Keyword::Let | Keyword::Const)) => Some(self.error(
                SyntaxErrorKind::Invalid,
                span,
                format!(
                    "a `{}` declaration cannot be the body of `if`/`while`/`do`/`for` or a labelled statement; wrap it in a block",
                    k.as_str()
                ),
            )),
            TokenKind::Keyword(k) => {
                unsupported_statement(*k).map(|what| self.unsupported(span, what))
            }
            _ => None,
        }
    }

    /// The current identifier is followed by `:`: it starts a labelled
    /// statement (one token of lookahead; QuickJS `is_label`). Reserved
    /// words lex as keywords, so they never get here.
    fn at_label(&self) -> bool {
        self.peek_next()
            .is_some_and(|t| t.kind == TokenKind::Punct(Punct::Colon))
    }

    /// LabelledStatement: LabelIdentifier `:` Statement (clj-quickjs-82),
    /// from the first label of a label set `L1: L2: ... body`, which is
    /// parsed in one loop and nested outermost first. The labels are in
    /// scope in the body; they mark an iteration statement when the body
    /// after the whole set is a loop, so `continue` may target any label of
    /// the set. Early errors: a label already in scope in the same function
    /// (QuickJS `js_parse_statement_or_decl`: "duplicate label name"); a
    /// declaration as the body (strict mode, reported by
    /// [`Self::statement_start_error`]).
    #[inline(never)]
    fn parse_labelled(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        let outer = self.labels.len();
        let mut set = Vec::new();
        while let TokenKind::Identifier(name) = &self.tok.kind {
            let label = Ident {
                name: name.clone(),
                span: self.tok.span,
            };
            if !self.at_label() {
                break;
            }
            self.deeper(cost::STATEMENT)?;
            if self.labels.iter().any(|l| l.name == label.name) {
                return Err(self.error(
                    SyntaxErrorKind::Invalid,
                    label.span,
                    format!(
                        "label `{}` is already declared by an enclosing labelled statement",
                        label.name
                    ),
                ));
            }
            self.advance()?;
            self.advance()?; // `:`
            self.labels.push(LabelEntry {
                name: label.name.clone(),
                iteration: false,
            });
            set.push(label);
        }
        let iteration = matches!(
            self.tok.kind,
            TokenKind::Keyword(Keyword::While | Keyword::Do | Keyword::For)
        );
        for entry in &mut self.labels[outer..] {
            entry.iteration = iteration;
        }
        let mut body = self.parse_statement()?;
        self.labels.truncate(outer);
        let outermost = set.remove(0);
        while let Some(label) = set.pop() {
            let span = self.span_from(label.span.start);
            body = self.bounded(Stmt::new(
                StmtKind::Labelled {
                    label,
                    body: Box::new(body),
                },
                span,
            ))?;
        }
        self.depth = saved;
        Ok(StmtKind::Labelled {
            label: outermost,
            body: Box::new(body),
        })
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

    /// ForStatement (clj-quickjs-81): `for` `(` ForInit? `;` Expression? `;`
    /// Expression? `)` Statement, where ForInit is an Expression or a
    /// `let`/`const` declaration list (QuickJS `TOK_FOR` in
    /// `js_parse_statement_or_decl`). Or ForInStatement (clj-quickjs-85):
    /// `for` `(` (LeftHandSideExpression | (`let` | `const` | `var`)
    /// BindingIdentifier) `in` Expression `)` Statement (QuickJS
    /// `js_parse_for_in_of`), told apart from the three-part form after the
    /// head's first element, which is parsed without a top-level `in`
    /// operator (`[~In]`, [`Self::no_in`], clj-quickjs-74). ForInit may also
    /// be a `var` declaration list (clj-quickjs-89). For-of heads are
    /// rejected as unsupported at the `for` keyword.
    fn parse_for(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        self.deeper(cost::BODY)?;
        let head = self.parse_for_head()?;
        let outer = std::mem::replace(&mut self.in_loop, true);
        let body = Box::new(self.parse_statement()?);
        self.in_loop = outer;
        self.depth = saved;
        Ok(match head {
            ForHead::Classic(init, test, update) => StmtKind::For {
                init,
                test,
                update,
                body,
            },
            ForHead::In(head, object) => StmtKind::ForIn { head, object, body },
        })
    }

    /// SwitchStatement (clj-quickjs-83): `switch` `(` Expression `)` `{`
    /// CaseClause* `}`, where a CaseClause is `case` Expression `:` or
    /// `default` `:`, followed by StatementListItems up to the next clause
    /// (QuickJS `TOK_SWITCH` in `js_parse_statement_or_decl`). The case
    /// block is charged as a block. Early error (`Invalid`): a second
    /// `default`, at its keyword ("duplicate default"). Redeclarations
    /// across clauses (the case block is one scope) are compiler errors, as
    /// for any statement list. An unlabelled `break` in a clause leaves the
    /// switch; `continue` still needs an enclosing loop.
    #[inline(never)]
    fn parse_switch(&mut self) -> PResult<StmtKind> {
        let saved = self.depth;
        self.deeper(cost::BODY + cost::BLOCK)?;
        self.advance()?;
        let discriminant = self.parse_paren_condition()?;
        self.expect_punct(Punct::LBrace)?;
        let outer = std::mem::replace(&mut self.in_switch, true);
        let mut cases: Vec<SwitchCase> = Vec::new();
        let mut has_default = false;
        while !self.is_punct(Punct::RBrace) {
            let start = self.tok.span.start;
            let test = match self.tok.kind {
                TokenKind::Keyword(Keyword::Case) => {
                    self.advance()?;
                    Some(self.parse_expression()?)
                }
                TokenKind::Keyword(Keyword::Default) => {
                    if has_default {
                        return Err(self.error(
                            SyntaxErrorKind::Invalid,
                            self.tok.span,
                            "a `switch` statement can have only one `default` clause",
                        ));
                    }
                    has_default = true;
                    self.advance()?;
                    None
                }
                TokenKind::Eof => return Err(self.unexpected("`}`")),
                _ => return Err(self.unexpected("`case`, `default` or `}`")),
            };
            self.expect_punct(Punct::Colon)?;
            let mut body = Vec::new();
            while !matches!(
                self.tok.kind,
                TokenKind::Keyword(Keyword::Case | Keyword::Default)
                    | TokenKind::Punct(Punct::RBrace)
                    | TokenKind::Eof
            ) {
                body.push(self.parse_statement_list_item()?);
            }
            cases.push(SwitchCase {
                test,
                body,
                span: self.span_from(start),
            });
        }
        self.advance()?;
        self.in_switch = outer;
        self.depth = saved;
        Ok(StmtKind::Switch {
            discriminant,
            cases,
        })
    }

    /// `for` `(` ForInit? `;` Expression? `;` Expression? `)`, or `for` `(`
    /// ForInHead `in` Expression `)` (clj-quickjs-85). Out of line, so the
    /// frame of [`Self::parse_for`], which recurses through the body, stays
    /// small. An expression head and the initializers of a declaration head
    /// are parsed with `[~In]` ([`Self::no_in`]), so an `in` after the
    /// head's first element starts a for-in head; a `let`/`const`/`var` head
    /// is a for-in head when `in` follows its first name.
    #[inline(never)]
    fn parse_for_head(&mut self) -> PResult<ForHead> {
        let keyword = self.advance()?.span;
        self.expect_punct(Punct::LParen)?;
        let init = match &self.tok.kind {
            TokenKind::Punct(Punct::Semicolon) => None,
            TokenKind::Keyword(k @ (Keyword::Let | Keyword::Const)) => {
                let kind = match k {
                    Keyword::Let => LexicalKind::Let,
                    _ => LexicalKind::Const,
                };
                self.advance()?;
                let name = self.parse_binding_identifier("a variable name")?;
                if self.is_keyword(Keyword::In) {
                    return self.parse_for_in_rest(ForInHead::Lexical(kind, name));
                }
                let declarators = self.parse_declarators(Some(kind), Some(keyword), Some(name))?;
                Some(ForInit::Lexical(LexicalDecl { kind, declarators }))
            }
            TokenKind::Keyword(Keyword::Var) => {
                self.advance()?;
                let name = self.parse_binding_identifier("a variable name")?;
                if self.is_keyword(Keyword::In) {
                    return self.parse_for_in_rest(ForInHead::Var(name));
                }
                Some(ForInit::Var(self.parse_declarators(
                    None,
                    Some(keyword),
                    Some(name),
                )?))
            }
            _ => {
                let outer = std::mem::replace(&mut self.no_in, true);
                let init = self.parse_expression()?;
                self.no_in = outer;
                if self.is_keyword(Keyword::In) {
                    let target = self.for_in_target(&init)?;
                    return self.parse_for_in_rest(ForInHead::Binding(target));
                }
                if let Some(error) = self.for_of_error(keyword) {
                    return Err(error);
                }
                Some(ForInit::Expr(init))
            }
        };
        self.expect_punct(Punct::Semicolon)?;
        let test = if self.is_punct(Punct::Semicolon) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_punct(Punct::Semicolon)?;
        let update = if self.is_punct(Punct::RParen) {
            None
        } else {
            Some(self.parse_expression()?)
        };
        self.expect_punct(Punct::RParen)?;
        Ok(ForHead::Classic(init, test, update))
    }

    /// `in` Expression `)` after the left side `head` of a for-in head
    /// (clj-quickjs-85). The object is an Expression with `in` allowed.
    #[inline(never)]
    fn parse_for_in_rest(&mut self, head: ForInHead) -> PResult<ForHead> {
        self.advance()?;
        let object = self.parse_expression()?;
        self.expect_punct(Punct::RParen)?;
        Ok(ForHead::In(head, object))
    }

    /// The binding that the left-hand side `left` of a for-in head assigns:
    /// an identifier, possibly parenthesized. A property reference is
    /// unsupported (clj-quickjs-87); anything else is rejected as the
    /// target of `=` is (`for (1 in o)` is `Invalid`).
    #[inline(never)]
    fn for_in_target(&self, left: &Expr) -> PResult<Ident> {
        if is_property_target(left) {
            return Err(
                self.unsupported(left.span, "property references as `for`-`in` targets are")
            );
        }
        self.simple_target(left, TargetUse::Assign)
    }

    /// The current token is the `of` after the left side of a for-of head:
    /// reject the loop as unsupported at the `for` keyword.
    #[inline(never)]
    fn for_of_error(&self, keyword: Span) -> Option<SyntaxError> {
        match &self.tok.kind {
            TokenKind::Identifier(name) if name == "of" => {
                Some(self.unsupported(keyword, "`for`-`of` loops are"))
            }
            _ => None,
        }
    }

    /// The current token is an `in` after a later declarator or an
    /// initializer of a `for` head's declaration list: a for-in head
    /// declares exactly one binding, without an initializer (an early
    /// error; QuickJS `js_parse_for_in_of`).
    #[inline(never)]
    fn for_in_declaration_error(&self) -> SyntaxError {
        self.error(
            SyntaxErrorKind::Invalid,
            self.tok.span,
            "the declaration in a `for`-`in` head must be a single binding without an initializer",
        )
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

    /// BreakStatement: `break` [no LineTerminator here] LabelIdentifier? `;`
    /// and ContinueStatement likewise. Restricted productions: a line break
    /// after the keyword ends the statement. Early errors (QuickJS
    /// `emit_break`): an unlabelled `break` outside a loop or `switch`
    /// (clj-quickjs-83) of the same function ("break must be inside loop or
    /// switch"), an unlabelled `continue` outside a loop of the same
    /// function ("continue must be inside loop"); a label that no enclosing
    /// labelled statement of the same function declares ("break/continue
    /// label not found"); `continue` to a label of a statement that is not
    /// a loop (clj-quickjs-82).
    #[inline(never)]
    fn parse_break_continue(&mut self) -> PResult<StmtKind> {
        let keyword = self.advance()?;
        let is_break = keyword.kind == TokenKind::Keyword(Keyword::Break);
        let name = if is_break { "break" } else { "continue" };
        if let TokenKind::Identifier(label) = &self.tok.kind
            && !self.tok.newline_before
        {
            let label = Ident {
                name: label.clone(),
                span: self.tok.span,
            };
            match self.labels.iter().rev().find(|l| l.name == label.name) {
                None => {
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        label.span,
                        format!(
                            "undefined label `{}`: no enclosing statement of the same function has it",
                            label.name
                        ),
                    ));
                }
                Some(entry) if !is_break && !entry.iteration => {
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        label.span,
                        format!(
                            "`continue {}` must target a label of an enclosing loop",
                            label.name
                        ),
                    ));
                }
                Some(_) => {}
            }
            self.advance()?;
            self.expect_semi()?;
            return Ok(if is_break {
                StmtKind::Break(Some(label))
            } else {
                StmtKind::Continue(Some(label))
            });
        }
        let kind = if is_break {
            StmtKind::Break(None)
        } else {
            StmtKind::Continue(None)
        };
        let allowed = self.in_loop || (is_break && self.in_switch);
        if !allowed {
            let place = if is_break {
                "a loop or `switch`"
            } else {
                "a loop"
            };
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                keyword.span,
                format!("`{name}` must be inside {place} of the same function"),
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
        let decl = self.parse_lexical_declarators()?;
        self.expect_semi()?;
        self.bounded(Stmt::new(StmtKind::Lexical(decl), self.span_from(start)))
    }

    /// VariableStatement: `var` Declarator (`,` Declarator)* `;`
    /// (clj-quickjs-94), in any statement position. Out of line, so the
    /// frame of [`Self::parse_statement`] stays small.
    #[inline(never)]
    fn parse_var(&mut self) -> PResult<StmtKind> {
        self.advance()?;
        let declarators = self.parse_declarators(None, None, None)?;
        self.expect_semi()?;
        Ok(StmtKind::Var(declarators))
    }

    /// (`let` | `const`) Declarator (`,` Declarator)*, without the `;`.
    fn parse_lexical_declarators(&mut self) -> PResult<LexicalDecl> {
        let kind = if self.advance()?.kind == TokenKind::Keyword(Keyword::Let) {
            LexicalKind::Let
        } else {
            LexicalKind::Const
        };
        let declarators = self.parse_declarators(Some(kind), None, None)?;
        Ok(LexicalDecl { kind, declarators })
    }

    /// Declarator (`,` Declarator)* after the declaration keyword: a binding
    /// identifier with an optional `= AssignmentExpression` initializer,
    /// required for `const`. `lexical`: `None` for `var`. `for_keyword`: in
    /// the head of the `for` statement at that span, whose initializers are
    /// parsed with `[~In]`; there `of` after a name starts an unsupported
    /// for-of head, and `in` after a later name or after an initializer is
    /// an invalid for-in head. `first`: the first name, already parsed.
    fn parse_declarators(
        &mut self,
        lexical: Option<LexicalKind>,
        for_keyword: Option<Span>,
        mut first: Option<Ident>,
    ) -> PResult<Vec<Declarator>> {
        let mut declarators = Vec::new();
        loop {
            let name = match first.take() {
                Some(name) => name,
                None => self.parse_binding_identifier("a variable name")?,
            };
            if let Some(keyword) = for_keyword {
                if let Some(error) = self.for_of_error(keyword) {
                    return Err(error);
                }
                if self.is_keyword(Keyword::In) {
                    return Err(self.for_in_declaration_error());
                }
            }
            let init = if self.eat_punct(Punct::Assign)? {
                let outer = std::mem::replace(&mut self.no_in, for_keyword.is_some());
                let init = self.parse_assignment()?;
                self.no_in = outer;
                if for_keyword.is_some() && self.is_keyword(Keyword::In) {
                    return Err(self.for_in_declaration_error());
                }
                Some(init)
            } else {
                if lexical == Some(LexicalKind::Const) {
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
        Ok(declarators)
    }

    /// BindingIdentifier: an identifier that is not a reserved word, a
    /// reserved constant, or (in strict code) `eval` or `arguments`.
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
                if self.strict && STRICT_RESTRICTED.contains(&name.as_str()) {
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
        let outer_this = std::mem::replace(&mut self.this_allowed, true);
        let (name, (params, defaults)) = self.parse_function_head(declaration)?;
        let outer = std::mem::replace(&mut self.in_function, true);
        let outer_loop = std::mem::replace(&mut self.in_loop, false);
        let outer_switch = std::mem::replace(&mut self.in_switch, false);
        let outer_labels = std::mem::take(&mut self.labels);
        let simple = defaults.iter().all(Option::is_none);
        let (body, strict) =
            self.parse_function_body(name.as_ref(), &params, simple, self.strict)?;
        self.in_function = outer;
        self.this_allowed = outer_this;
        self.in_loop = outer_loop;
        self.in_switch = outer_switch;
        self.labels = outer_labels;
        self.depth = saved;
        let function = Function::new(
            FunctionKind::Ordinary,
            name,
            params,
            defaults,
            body,
            self.span_from(start),
            strict,
        );
        self.check_height(function.height)?;
        Ok(function)
    }

    /// `{` FunctionBody `}`: a [`Self::parse_block`] whose leading
    /// statements are a directive prologue ([`Self::directive`], with the
    /// function's `name` and `params` for its retroactive check). Returns
    /// the body and whether the function is strict; the enclosing mode is
    /// restored after it. Unless the parameter list is `simple` (it has a
    /// default value, clj-quickjs-100), a `"use strict"` directive is an
    /// early error, strict code or not (ECMAScript FunctionBody static
    /// semantics; QuickJS `js_parse_function_check_names`).
    fn parse_function_body(
        &mut self,
        name: Option<&Ident>,
        params: &[Ident],
        simple: bool,
        outer_strict: bool,
    ) -> PResult<(Vec<Stmt>, bool)> {
        let saved = self.depth;
        self.deeper(cost::BLOCK)?;
        self.expect_punct(Punct::LBrace)?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        let mut body = Vec::new();
        let mut prologue = true;
        while !self.is_punct(Punct::RBrace) {
            if self.tok.kind == TokenKind::Eof {
                return Err(self.unexpected("`}`"));
            }
            let stmt = self.parse_body_item(prologue)?;
            if prologue && !simple {
                self.reject_use_strict(&stmt)?;
            }
            prologue = prologue && self.directive(&stmt, &body, name, params)?;
            body.push(stmt);
        }
        self.no_in = no_in;
        // Restore the enclosing mode before the token after `}` is lexed.
        let strict = self.strict;
        self.set_strict(outer_strict);
        self.advance()?;
        self.depth = saved;
        Ok((body, strict))
    }

    /// Reject `stmt`, a statement of the directive prologue of a function
    /// with a default parameter value, if it is a `"use strict"` directive
    /// (exact source text, as for [`Self::directive`]).
    #[inline(never)]
    fn reject_use_strict(&self, stmt: &Stmt) -> PResult<()> {
        if let StmtKind::Expr(Expr {
            kind: ExprKind::String(_),
            span,
            ..
        }) = &stmt.kind
            && USE_STRICT.contains(&span.text(self.src))
        {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                *span,
                "a \"use strict\" directive is not allowed in a function with default parameter values",
            ));
        }
        Ok(())
    }

    /// `function` Name? `(` Params `)`, up to the body.
    #[inline(never)]
    fn parse_function_head(&mut self, declaration: bool) -> PResult<(Option<Ident>, Params)> {
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

    /// `(` [ Param { `,` Param } [ `,` ] ] `)` with Param =
    /// BindingIdentifier [ `=` AssignmentExpression ] (a default value,
    /// clj-quickjs-100): distinct plain identifiers (strict mode, arrows and
    /// lists with a default; the subset also rejects duplicates in sloppy
    /// functions, `docs/grammar.md`), each with its default, if any.
    #[inline(never)]
    fn parse_formal_parameters(&mut self) -> PResult<Params> {
        self.expect_punct(Punct::LParen)?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        let mut params: Vec<Ident> = Vec::new();
        let mut defaults: Vec<Option<Expr>> = Vec::new();
        while !self.is_punct(Punct::RParen) {
            if self.is_punct(Punct::Ellipsis) {
                return Err(self.unsupported(self.tok.span, "rest parameters are"));
            }
            let param = self.parse_binding_identifier("a parameter name")?;
            if params.iter().any(|p| p.name == param.name) {
                return Err(self.error(
                    SyntaxErrorKind::Invalid,
                    param.span,
                    format!("duplicate parameter name `{}`", param.name),
                ));
            }
            params.push(param);
            defaults.push(if self.eat_punct(Punct::Assign)? {
                Some(self.parse_assignment()?)
            } else {
                None
            });
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.no_in = no_in;
        self.expect_punct(Punct::RParen)?;
        Ok((params, defaults))
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
        let (params, defaults) = if self.is_punct(Punct::LParen) {
            self.parse_formal_parameters()?
        } else {
            (
                vec![self.parse_binding_identifier("a parameter name")?],
                vec![None],
            )
        };
        if !self.is_punct(Punct::Arrow) {
            return Err(self.unexpected("`=>`"));
        }
        if self.tok.newline_before {
            return Err(self.misplaced_arrow(&self.tok));
        }
        self.advance()?;
        let (kind, body, strict) = if self.is_punct(Punct::LBrace) {
            let outer = std::mem::replace(&mut self.in_function, true);
            let outer_loop = std::mem::replace(&mut self.in_loop, false);
            let outer_switch = std::mem::replace(&mut self.in_switch, false);
            let outer_labels = std::mem::take(&mut self.labels);
            let simple = defaults.iter().all(Option::is_none);
            let (body, strict) = self.parse_function_body(None, &params, simple, self.strict)?;
            self.in_function = outer;
            self.in_loop = outer_loop;
            self.in_switch = outer_switch;
            self.labels = outer_labels;
            (FunctionKind::Arrow, body, strict)
        } else {
            // The body is an AssignmentExpression, so the arrow ends with
            // it: a `,` after it separates arguments or declarators, or is
            // the comma operator of an enclosing Expression.
            let value = self.parse_assignment()?;
            let span = value.span;
            let body = self.bounded(Stmt::new(StmtKind::Return(Some(value)), span))?;
            (FunctionKind::ConciseArrow, vec![body], self.strict)
        };
        self.depth = saved;
        let function = Function::new(
            kind,
            None,
            params,
            defaults,
            body,
            self.span_from(start),
            strict,
        );
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

    /// AssignmentExpression: ConditionalExpression, ArrowFunction, or an
    /// identifier or a property reference `o.p` / `o[k]` (`=` | an
    /// arithmetic, bitwise, shift or `**` compound operator)
    /// AssignmentExpression (right-associative).
    fn parse_assignment(&mut self) -> PResult<Expr> {
        let saved = self.depth;
        self.deeper(cost::ASSIGNMENT)?;
        if self.at_arrow() {
            let arrow = self.parse_arrow_function()?;
            self.depth = saved;
            return Ok(arrow);
        }
        let left = self.parse_conditional()?;
        if let TokenKind::Punct(p) = self.tok.kind
            && let Some(op) = compound_assign_op(p)
        {
            if is_property_target(&left) {
                return self.compound_assign_property(op, left, saved);
            }
            let target = self.simple_target(&left, TargetUse::Assign)?;
            let op_span = self.advance()?.span;
            let value = self.parse_assignment()?;
            self.depth = saved;
            let span = left.span.to(value.span);
            return self.expr(
                ExprKind::CompoundAssign {
                    op,
                    target,
                    value: Box::new(value),
                    op_span,
                },
                span,
            );
        }
        if !self.is_punct(Punct::Assign) {
            if let Some(error) = self.assignment_operator_error(&left) {
                return Err(error);
            }
            self.depth = saved;
            return Ok(left);
        }
        if is_property_target(&left) {
            return self.assign_member(left, saved);
        }
        if matches!(&left.kind, ExprKind::Array(_)) && left.span.text(self.src).starts_with('[') {
            // An array assignment pattern `[a, b] = v` (clj-quickjs-118
            // parses the literal; destructuring is its own pellet).
            return Err(self.unsupported(left.span, "destructuring assignment is"));
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

    /// `o.p op= v` or `o[k] op= v` (clj-quickjs-260), with the current
    /// token the `op=`; `left` is the member expression (possibly
    /// parenthesized), whose object is not the reserved intrinsic binding.
    #[inline(never)]
    fn compound_assign_property(
        &mut self,
        op: BinaryOp,
        left: Expr,
        saved: usize,
    ) -> PResult<Expr> {
        let op_span = self.advance()?.span;
        let value = self.parse_assignment()?;
        self.depth = saved;
        let span = left.span.to(value.span);
        self.expr(
            ExprKind::CompoundAssignProperty {
                op,
                target: Box::new(left),
                value: Box::new(value),
                op_span,
            },
            span,
        )
    }

    /// Rejects logical assignment and an arrow after an expression.
    #[inline(never)]
    fn assignment_operator_error(&self, left: &Expr) -> Option<SyntaxError> {
        match &self.tok.kind {
            TokenKind::Punct(p) if is_logical_assign(*p) => Some(self.unsupported(
                self.tok.span,
                &format!("logical assignment `{}` is", p.as_str()),
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
    /// else is rejected. (A property other than an intrinsic never gets
    /// here: [`Self::update`] (clj-quickjs-76), [`Self::assign_member`] and
    /// [`Self::compound_assign_property`] (clj-quickjs-260) take it first.)
    #[inline(never)]
    fn simple_target(&self, left: &Expr, use_: TargetUse) -> PResult<Ident> {
        let text = strip_parens(left.span.text(self.src));
        let (verb, invalid) = match use_ {
            TargetUse::Assign => ("assigned", "invalid assignment target"),
            TargetUse::Update => (
                "incremented or decremented",
                "invalid increment/decrement operand",
            ),
        };
        match &left.kind {
            ExprKind::Identifier(name)
                if self.strict && STRICT_RESTRICTED.contains(&name.as_str()) =>
            {
                Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    left.span,
                    format!("`{name}` cannot be {verb} in strict mode"),
                ))
            }
            ExprKind::Identifier(name) => Ok(Ident {
                name: name.clone(),
                span: left.span,
            }),
            ExprKind::Member { object, .. } if is_reserved_binding(object) => Err(self.error(
                SyntaxErrorKind::ReservedWord,
                left.span,
                format!("intrinsics of the reserved binding `{RESERVED_BINDING}` cannot be {verb}"),
            )),
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

    /// `object.property = value` (clj-quickjs-114) or `object[index] =
    /// value` (clj-quickjs-118), with the current token the `=`; `left` is
    /// the member expression (possibly parenthesized), whose object is not
    /// the reserved intrinsic binding. Compound assignment is
    /// [`Self::compound_assign_property`], `++`/`--` [`Self::update`].
    #[inline(never)]
    fn assign_member(&mut self, left: Expr, saved: usize) -> PResult<Expr> {
        let span = left.span;
        self.advance()?;
        let value = Box::new(self.parse_assignment()?);
        self.depth = saved;
        let span = span.to(value.span);
        let kind = match left.kind {
            ExprKind::Member {
                object,
                property,
                op_span,
            } => ExprKind::AssignMember {
                object,
                property,
                value,
                op_span,
            },
            ExprKind::Index {
                object,
                index,
                op_span,
            } => ExprKind::AssignIndex {
                object,
                index,
                value,
                op_span,
            },
            _ => return Err(self.unexpected("a property assignment")),
        };
        self.expr(kind, span)
    }

    /// ArrayLiteral: `[` (Elision? AssignmentExpression `,`)* Elision?
    /// `]` (clj-quickjs-118), with the current token the `[`. Each comma
    /// not directly after an element is a hole; a final comma after an
    /// element adds nothing. Spread elements are rejected explicitly.
    #[inline(never)]
    fn parse_array_literal(&mut self) -> PResult<Expr> {
        let start = self.tok.span.start;
        let saved = self.depth;
        self.deeper(cost::ARRAY)?;
        self.advance()?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        let mut elements = Vec::new();
        loop {
            match self.tok.kind {
                TokenKind::Punct(Punct::RBracket) => break,
                TokenKind::Punct(Punct::Comma) => {
                    elements.push(None);
                    self.advance()?;
                    continue;
                }
                TokenKind::Punct(Punct::Ellipsis) => {
                    return Err(self.unsupported(self.tok.span, "spread elements are"));
                }
                _ => {}
            }
            elements.push(Some(self.parse_assignment()?));
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.no_in = no_in;
        self.expect_punct(Punct::RBracket)?;
        self.depth = saved;
        let span = self.span_from(start);
        self.expr(ExprKind::Array(elements), span)
    }

    /// `[` Expression `]` after a member object (clj-quickjs-118), with the
    /// current token the `[`. The link costs one unit of the chain, like
    /// `.`; the bracketed expression is charged like a parenthesized one
    /// while it is parsed.
    #[inline(never)]
    fn parse_index(&mut self, object: Expr) -> PResult<Expr> {
        self.deeper(cost::OPERATOR)?;
        let link = self.depth;
        self.deeper(cost::PAREN)?;
        let op_span = self.tok.span;
        self.advance()?;
        let no_in = std::mem::replace(&mut self.no_in, false);
        let index = self.parse_expression()?;
        self.no_in = no_in;
        self.expect_punct(Punct::RBracket)?;
        self.depth = link;
        let span = self.span_from(object.span.start);
        self.expr(
            ExprKind::Index {
                object: Box::new(object),
                index: Box::new(index),
                op_span,
            },
            span,
        )
    }

    /// ObjectLiteral: `{` (PropertyDefinition (`,` PropertyDefinition)*
    /// `,`?)? `}` (clj-quickjs-114), with the current token the `{`.
    #[inline(never)]
    fn parse_object_literal(&mut self) -> PResult<Expr> {
        let start = self.tok.span.start;
        let saved = self.depth;
        self.deeper(cost::OBJECT)?;
        // A key is an IdentifierName, where a reserved word written with
        // escapes is an ordinary name (`{\u0069f: 1}`).
        self.lexer.expect_property_name();
        self.advance()?;
        let mut properties = Vec::new();
        let mut proto_seen = false;
        let no_in = std::mem::replace(&mut self.no_in, false);
        while !self.is_punct(Punct::RBrace) {
            properties.push(self.parse_property_definition(&mut proto_seen)?);
            if self.is_punct(Punct::Comma) {
                self.lexer.expect_property_name();
            }
            if !self.eat_punct(Punct::Comma)? {
                break;
            }
        }
        self.no_in = no_in;
        self.expect_punct(Punct::RBrace)?;
        self.depth = saved;
        let span = self.span_from(start);
        self.expr(ExprKind::Object(properties), span)
    }

    /// PropertyDefinition: PropertyName `:` AssignmentExpression, or a
    /// shorthand IdentifierReference. A PropertyName is an IdentifierName
    /// (reserved words included), a string literal or a numeric literal.
    /// Concise methods are functions with a home object. Computed names,
    /// spread and getters/setters are rejected
    /// explicitly. A `__proto__: value` (identifier or string name, not a
    /// shorthand) sets the prototype (clj-quickjs-121); a second one in the
    /// same literal (`proto_seen`) is an early error.
    fn parse_property_definition(&mut self, proto_seen: &mut bool) -> PResult<PropertyDef> {
        let span = self.tok.span;
        let next = self.peek_next();
        let ends_shorthand = next
            .as_ref()
            .is_some_and(|t| matches!(t.kind, TokenKind::Punct(Punct::Comma | Punct::RBrace)));
        if let Some(error) = self.property_definition_error(next.as_ref(), ends_shorthand) {
            return Err(error);
        }
        if ends_shorthand {
            // `{x}` is `{x: x}`: an identifier reference, with the same rules
            // as anywhere else (`undefined`, `eval`, ...).
            let TokenKind::Identifier(name) = &self.tok.kind else {
                return Err(self.unexpected("a shorthand property name"));
            };
            if let Some(k) = Keyword::from_text(name) {
                // A reserved word written with escapes (`{\u0069f}`).
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    span,
                    format!(
                        "`{}` is a reserved word and cannot be a shorthand property: write `{0}: value`",
                        k.as_str()
                    ),
                ));
            }
            let name = PropertyName::Identifier(name.clone());
            let value = self.parse_identifier_reference()?;
            return Ok(PropertyDef {
                name,
                name_span: span,
                value,
                shorthand: true,
            });
        }
        let name = match &self.tok.kind {
            TokenKind::Identifier(name) => PropertyName::Identifier(name.clone()),
            TokenKind::Keyword(k) => PropertyName::Identifier(k.as_str().to_string()),
            TokenKind::String(s) => PropertyName::String(s.clone()),
            TokenKind::Number(n) => PropertyName::Number(*n),
            _ => return Err(self.unexpected("a property name")),
        };
        self.advance()?;
        if self.is_punct(Punct::LParen) {
            let function = self.parse_object_method(span.start)?;
            let value = self.expr(
                ExprKind::Function(Box::new(function)),
                self.span_from(span.start),
            )?;
            return Ok(PropertyDef {
                name,
                name_span: span,
                value,
                shorthand: false,
            });
        }
        if !self.is_punct(Punct::Colon) {
            return Err(self.unexpected("`:` after the property name"));
        }
        if name.is_proto() {
            if *proto_seen {
                return Err(self.error(
                    SyntaxErrorKind::Invalid,
                    span,
                    "duplicate `__proto__` in an object literal: only one `__proto__: value` may set the prototype",
                ));
            }
            *proto_seen = true;
        }
        self.advance()?;
        let value = self.parse_assignment()?;
        Ok(PropertyDef {
            name,
            name_span: span,
            value,
            shorthand: false,
        })
    }

    /// A concise method after its literal key. Keep function parsing out of
    /// the object-literal dispatcher to bound native stack use.
    #[inline(never)]
    fn parse_object_method(&mut self, start: usize) -> PResult<Function> {
        let saved = self.depth;
        self.deeper(cost::FUNCTION)?;
        let outer_strict = self.strict;
        self.set_strict(true);
        let outer_this = std::mem::replace(&mut self.this_allowed, true);
        let (params, defaults) = self.parse_formal_parameters()?;
        let outer_function = std::mem::replace(&mut self.in_function, true);
        let outer_loop = std::mem::replace(&mut self.in_loop, false);
        let outer_switch = std::mem::replace(&mut self.in_switch, false);
        let outer_labels = std::mem::take(&mut self.labels);
        let simple = defaults.iter().all(Option::is_none);
        let (body, strict) = self.parse_function_body(None, &params, simple, outer_strict)?;
        self.in_function = outer_function;
        self.this_allowed = outer_this;
        self.in_loop = outer_loop;
        self.in_switch = outer_switch;
        self.labels = outer_labels;
        self.depth = saved;
        let function = Function::new(
            FunctionKind::Method,
            None,
            params,
            defaults,
            body,
            self.span_from(start),
            strict,
        );
        self.check_height(function.height)?;
        Ok(function)
    }

    /// The error of a property definition that starts at the current token
    /// and that the subset does not support or JavaScript forbids; `next`
    /// is the following token, `ends_shorthand` whether it ends a
    /// shorthand property (`,` or `}`).
    #[inline(never)]
    fn property_definition_error(
        &self,
        next: Option<&Token>,
        ends_shorthand: bool,
    ) -> Option<SyntaxError> {
        let span = self.tok.span;
        let starts_name = |t: &Token| {
            matches!(
                t.kind,
                TokenKind::Identifier(_)
                    | TokenKind::Keyword(_)
                    | TokenKind::String(_)
                    | TokenKind::Number(_)
                    | TokenKind::Punct(Punct::LBracket | Punct::Star)
            )
        };
        let next_is = |p: Punct| next.is_some_and(|t| t.kind == TokenKind::Punct(p));
        match &self.tok.kind {
            TokenKind::Punct(Punct::LBracket) => {
                Some(self.unsupported(span, "computed property names are"))
            }
            TokenKind::Punct(Punct::Ellipsis) => {
                Some(self.unsupported(span, "spread properties are"))
            }
            TokenKind::Punct(Punct::Star) => Some(self.unsupported(span, "generator methods are")),
            TokenKind::Identifier(word)
                if (word == "get" || word == "set") && next.is_some_and(starts_name) =>
            {
                Some(self.unsupported(span, "getters and setters are"))
            }
            TokenKind::Identifier(word)
                if word == "async" && next.is_some_and(|t| !t.newline_before && starts_name(t)) =>
            {
                Some(self.unsupported(span, "async methods are"))
            }
            TokenKind::Identifier(_) if next_is(Punct::Assign) => Some(self.error(
                SyntaxErrorKind::Invalid,
                span,
                "a shorthand property initializer `name = value` is only valid in a destructuring pattern",
            )),
            TokenKind::Keyword(k) if ends_shorthand => Some(self.error(
                SyntaxErrorKind::ReservedWord,
                span,
                format!(
                    "`{}` is a reserved word and cannot be a shorthand property: write `{0}: value`",
                    k.as_str()
                ),
            )),
            TokenKind::String(_) | TokenKind::Number(_) if ends_shorthand => Some(self.error(
                SyntaxErrorKind::UnexpectedToken,
                span,
                "a string or numeric property name needs a value: write `name: value`",
            )),
            _ => None,
        }
    }

    /// The UpdateExpression `++operand` / `operand++` (or `--`): the
    /// operand must be an identifier binding or a property reference
    /// `o.p` / `o[k]` (clj-quickjs-76), possibly parenthesized, else the
    /// error [`Self::simple_target`] gives (a property of the reserved
    /// intrinsic binding is a `ReservedWord` error there).
    #[inline(never)]
    fn update(&self, op: UpdateOp, prefix: bool, operand: Expr, op_span: Span) -> PResult<Expr> {
        if is_property_target(&operand) {
            let span = op_span.to(operand.span);
            return self.expr(
                ExprKind::UpdateProperty {
                    op,
                    prefix,
                    target: Box::new(operand),
                    op_span,
                },
                span,
            );
        }
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
        let no_in = std::mem::replace(&mut self.no_in, false);
        let consequent = self.parse_assignment()?;
        self.no_in = no_in;
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
            // `[~In]` (a `for` head's init): `in` ends the expression.
            if self.no_in && self.is_keyword(Keyword::In) {
                break;
            }
            let Some((prec, op)) = binary_op(&self.tok.kind) else {
                break;
            };
            if prec < min_prec {
                break;
            }
            self.deeper(cost::OPERATOR)?;
            let op_span = self.advance()?.span;
            let right = self.parse_binary(prec + 1)?;
            left = make_binary(op, left, right, op_span);
            self.check_height(left.height)?;
        }
        self.depth = saved;
        Ok(left)
    }

    #[inline(never)]
    fn reject_unsupported_binary(&self) -> PResult<()> {
        let what = match &self.tok.kind {
            TokenKind::Punct(p @ Punct::Nullish) => p.as_str(),
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
        let op_span = self.advance()?.span;
        let exponent = self.parse_exponent()?;
        self.depth = saved;
        let span = base.span.to(exponent.span);
        self.expr(
            ExprKind::Binary {
                op: BinaryOp::Exp,
                left: Box::new(base),
                right: Box::new(exponent),
                op_span,
            },
            span,
        )
    }

    /// UnaryExpression: (`!` | `-` | `+` | `~` | `typeof` | `void` | `delete`) UnaryExpression
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
            TokenKind::Keyword(Keyword::Typeof) => Prefix::Unary(UnaryOp::Typeof),
            TokenKind::Keyword(Keyword::Void) => Prefix::Unary(UnaryOp::Void),
            TokenKind::Keyword(Keyword::Delete) => Prefix::Delete,
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
            Prefix::Unary(op) => Some(op),
            Prefix::Delete => None,
            Prefix::Update(op) => return self.update(op, true, operand, op_span),
        };
        if op.is_none() {
            self.check_delete_operand(&operand, op_span)?;
        }
        if self.is_punct(Punct::StarStar) {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                self.tok.span,
                "a unary expression cannot be the left operand of `**`; parenthesize it",
            ));
        }
        let span = op_span.to(operand.span);
        let operand = Box::new(operand);
        let kind = match op {
            Some(op) => ExprKind::Unary { op, operand },
            None => ExprKind::Delete { operand, op_span },
        };
        self.expr(kind, span)
    }

    /// The early errors of `delete operand` (clj-quickjs-124), reported at
    /// the `delete` token: an identifier reference (possibly parenthesized,
    /// including the reserved constants `undefined`, `NaN` and `Infinity`)
    /// is an early `Invalid` SyntaxError in strict code (ECMAScript 13.5.1.1)
    /// and, in sloppy code, `Unsupported` until clj-quickjs-105 deletes
    /// global bindings; a property of the reserved intrinsic binding cannot
    /// be deleted.
    #[inline(never)]
    fn check_delete_operand(&self, operand: &Expr, op_span: Span) -> PResult<()> {
        let identifier = match &operand.kind {
            ExprKind::Identifier(_) => true,
            // A literal operand spans one primary token (inside any
            // parentheses), so it is a reserved constant exactly when the
            // last one parsed lies inside it: a comment or an escaped
            // spelling does not matter.
            ExprKind::Undefined | ExprKind::Number(_) => self
                .reserved_constant
                .is_some_and(|c| operand.span.start <= c.start && c.end <= operand.span.end),
            ExprKind::Member { object, .. } if is_reserved_binding(object) => {
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    operand.span,
                    format!(
                        "intrinsics of the reserved binding `{RESERVED_BINDING}` cannot be deleted"
                    ),
                ));
            }
            _ => false,
        };
        match (identifier, self.strict) {
            (false, _) => Ok(()),
            (true, true) => Err(self.error(
                SyntaxErrorKind::Invalid,
                op_span,
                "`delete` of an identifier is not allowed in strict mode code",
            )),
            (true, false) => Err(self.unsupported(
                op_span,
                "`delete` of an identifier in sloppy code (clj-quickjs-105) is",
            )),
        }
    }

    #[inline(never)]
    fn reject_unsupported_prefix(&self) -> PResult<()> {
        let span = self.tok.span;
        match &self.tok.kind {
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
        let head = if self.is_keyword(Keyword::New) {
            self.parse_new()
        } else {
            self.parse_primary()
        };
        let mut expr = head?;
        loop {
            if self.is_punct(Punct::LParen) {
                self.deeper(cost::CALL)?;
                let op_span = self.tok.span;
                let args = self.parse_arguments()?;
                let span = self.span_from(expr.span.start);
                expr = self.expr(
                    ExprKind::Call {
                        callee: Box::new(expr),
                        args,
                        op_span,
                    },
                    span,
                )?;
            } else if self.is_punct(Punct::Dot) {
                self.deeper(cost::OPERATOR)?;
                let op_span = self.tok.span;
                let property = self.parse_property_name()?;
                let span = expr.span.to(property.span);
                expr = self.expr(
                    ExprKind::Member {
                        object: Box::new(expr),
                        property,
                        op_span,
                    },
                    span,
                )?;
            } else if self.is_punct(Punct::LBracket) && !is_reserved_binding(&expr) {
                expr = self.parse_index(expr)?;
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

    /// `new` MemberExpression Arguments? (clj-quickjs-71; QuickJS
    /// `js_parse_postfix_expr`, `TOK_NEW`, revision 7d1c13c5d8f8), at the
    /// `new` token. The MemberExpression is a primary expression or another
    /// `new` expression followed by `.` and `[ ]` accesses only, so the
    /// first argument list belongs to this `new` (`new F()()` calls the
    /// constructed value, `new new F()()` constructs with the result of
    /// `new F()`). Without an argument list there are no arguments
    /// (`new F`). `new.target` is not supported (clj-quickjs-99).
    #[inline(never)]
    fn parse_new(&mut self) -> PResult<Expr> {
        let saved = self.depth;
        self.deeper(cost::CALL)?;
        let op_span = self.advance()?.span;
        if self.is_punct(Punct::Dot) {
            return Err(self.unsupported(self.span_from(op_span.start), "`new.target` is"));
        }
        let mut callee = if self.is_keyword(Keyword::New) {
            self.parse_new()?
        } else {
            self.parse_primary()?
        };
        loop {
            if self.is_punct(Punct::Dot) {
                self.deeper(cost::OPERATOR)?;
                let dot = self.tok.span;
                let property = self.parse_property_name()?;
                let span = callee.span.to(property.span);
                callee = self.expr(
                    ExprKind::Member {
                        object: Box::new(callee),
                        property,
                        op_span: dot,
                    },
                    span,
                )?;
            } else if self.is_punct(Punct::LBracket) && !is_reserved_binding(&callee) {
                callee = self.parse_index(callee)?;
            } else {
                break;
            }
        }
        let args = if self.is_punct(Punct::LParen) {
            self.parse_arguments()?
        } else {
            Vec::new()
        };
        let span = self.span_from(op_span.start);
        let expr = self.expr(
            ExprKind::New {
                callee: Box::new(callee),
                args,
                op_span,
            },
            span,
        )?;
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
            TokenKind::Punct(Punct::OptionalChain) => {
                Err(self.unsupported(span, "optional chaining is"))
            }
            // A template after a member or call expression, also across a
            // line break, is a tagged template (clj-quickjs-58).
            TokenKind::Template(part) if part.head => {
                Err(self.unsupported(span, "tagged templates are"))
            }
            _ => Ok(()),
        }
    }

    /// TemplateLiteral (clj-quickjs-57): a NoSubstitutionTemplate, or a
    /// TemplateHead, then Expression (TemplateMiddle Expression)*
    /// TemplateTail. The lexer starts the part after a substitution at its
    /// closing `}`. An invalid escape in any part is a SyntaxError, as the
    /// template is untagged.
    #[inline(never)]
    fn parse_template(&mut self) -> PResult<Expr> {
        let start = self.tok.span.start;
        let saved = self.depth;
        self.deeper(cost::TEMPLATE)?;
        let mut quasis = Vec::new();
        let mut exprs = Vec::new();
        loop {
            let TokenKind::Template(part) = &self.tok.kind else {
                return Err(self.unexpected("`}` closing the template substitution"));
            };
            let element = TemplateElement {
                cooked: part.cooked.clone()?,
                raw: part.raw.clone(),
                span: self.tok.span,
            };
            let tail = part.tail;
            quasis.push(element);
            self.advance()?;
            if tail {
                break;
            }
            let no_in = std::mem::replace(&mut self.no_in, false);
            exprs.push(self.parse_expression()?);
            self.no_in = no_in;
            if matches!(&self.tok.kind, TokenKind::Template(part) if part.head) {
                return Err(self.unexpected("`}` closing the template substitution"));
            }
        }
        self.depth = saved;
        let span = self.span_from(start);
        self.expr(ExprKind::Template { quasis, exprs }, span)
    }

    /// Arguments: `(` (AssignmentExpression (`,` AssignmentExpression)* `,`?)? `)`
    fn parse_arguments(&mut self) -> PResult<Vec<Expr>> {
        self.expect_punct(Punct::LParen)?;
        let no_in = std::mem::replace(&mut self.no_in, false);
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
        self.no_in = no_in;
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
            TokenKind::Keyword(Keyword::This) if self.this_allowed => ExprKind::This,
            TokenKind::Keyword(Keyword::Function) => {
                let function = self.parse_function(false)?;
                let span = function.span;
                return self.expr(ExprKind::Function(Box::new(function)), span);
            }
            TokenKind::Identifier(_) => return self.parse_identifier_reference(),
            TokenKind::Punct(Punct::LParen) => return self.parse_parenthesized(),
            TokenKind::Punct(Punct::LBrace) => return self.parse_object_literal(),
            TokenKind::Template(part) if part.head => return self.parse_template(),
            TokenKind::Punct(Punct::LBracket) => return self.parse_array_literal(),
            _ => return Err(self.primary_error()),
        };
        self.advance()?;
        Ok(Expr::new(kind, span))
    }

    /// An identifier in expression position, including the reserved
    /// constants and the `async` / arrow rejections. `arguments` is an
    /// ordinary reference in either mode. `eval` remains unsupported in
    /// strict code, and must resolve to a declared binding in sloppy code.
    #[inline(never)]
    fn parse_identifier_reference(&mut self) -> PResult<Expr> {
        let span = self.tok.span;
        let TokenKind::Identifier(name) = &self.tok.kind else {
            return Err(self.unexpected("an identifier"));
        };
        if RESERVED_CONSTANTS.contains(&name.as_str()) {
            self.reserved_constant = Some(span);
        }
        let kind = match name.as_str() {
            "undefined" => ExprKind::Undefined,
            "NaN" => ExprKind::Number(f64::NAN),
            "Infinity" => ExprKind::Number(f64::INFINITY),
            "eval" if self.strict => return Err(self.unsupported(span, "`eval` is")),
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
        let no_in = std::mem::replace(&mut self.no_in, false);
        let mut inner = self.parse_expression()?;
        self.no_in = no_in;
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
    /// normal parse then reports it at its position). A token after `{` or
    /// `,` is lexed as a possible property name, so a reserved word written
    /// with escapes as an object literal key in a default parameter value
    /// (`(a = {if: 1}) => a`, clj-quickjs-100) does not end the search;
    /// the normal parse checks every such token in its real position.
    fn arrow_after_parens(&self) -> Option<Token> {
        let mut lexer = self.lexer.clone();
        let mut open = 1usize;
        while open > 0 {
            match lexer.next_token().ok()?.kind {
                TokenKind::Punct(Punct::LBrace) => {
                    open += 1;
                    lexer.expect_property_name();
                }
                TokenKind::Punct(Punct::Comma) => lexer.expect_property_name(),
                TokenKind::Punct(Punct::LParen | Punct::LBracket) => open += 1,
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
    /// `delete` (clj-quickjs-124), whose node keeps its token.
    Delete,
}

/// A parsed `for` head (clj-quickjs-85).
enum ForHead {
    /// `(init; test; update)`
    Classic(Option<ForInit>, Option<Expr>, Option<Expr>),
    /// `(head in object)`
    In(ForInHead, Expr),
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
    let p = match kind {
        TokenKind::Punct(p) => p,
        TokenKind::Keyword(Keyword::Instanceof) => return Some((7, Binary(BinaryOp::Instanceof))),
        TokenKind::Keyword(Keyword::In) => return Some((7, Binary(BinaryOp::In))),
        _ => return None,
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

fn make_binary(op: BinOrLogical, left: Expr, right: Expr, op_span: Span) -> Expr {
    let span = left.span.to(right.span);
    let (left, right) = (Box::new(left), Box::new(right));
    let kind = match op {
        BinOrLogical::Binary(op) => ExprKind::Binary {
            op,
            left,
            right,
            op_span,
        },
        BinOrLogical::Logical(op) => ExprKind::Logical { op, left, right },
    };
    Expr::new(kind, span)
}

/// The binary operator of an arithmetic, bitwise, shift or `**` compound
/// assignment token (`+=` applies [`BinaryOp::Add`]).
fn compound_assign_op(p: Punct) -> Option<BinaryOp> {
    Some(match p {
        Punct::PlusAssign => BinaryOp::Add,
        Punct::MinusAssign => BinaryOp::Sub,
        Punct::StarAssign => BinaryOp::Mul,
        Punct::SlashAssign => BinaryOp::Div,
        Punct::PercentAssign => BinaryOp::Rem,
        Punct::StarStarAssign => BinaryOp::Exp,
        Punct::ShlAssign => BinaryOp::Shl,
        Punct::ShrAssign => BinaryOp::Sar,
        Punct::UShrAssign => BinaryOp::UShr,
        Punct::AmpAssign => BinaryOp::BitAnd,
        Punct::PipeAssign => BinaryOp::BitOr,
        Punct::CaretAssign => BinaryOp::BitXor,
        _ => return None,
    })
}

/// `&&=`, `||=` and `??=`, which the subset rejects.
fn is_logical_assign(p: Punct) -> bool {
    matches!(
        p,
        Punct::AmpAmpAssign | Punct::PipePipeAssign | Punct::NullishAssign
    )
}

/// Statements of JavaScript that the subset rejects, keyed by their first
/// keyword. (`let`, `const` and `function` are handled by the caller.)
fn unsupported_statement(k: Keyword) -> Option<&'static str> {
    Some(match k {
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

/// True for a property reference that can be assigned, compound-assigned
/// or incremented: `o.p` whose object is not the reserved intrinsic
/// binding, or `o[k]` (possibly parenthesized).
fn is_property_target(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Member { object, .. } => !is_reserved_binding(object),
        ExprKind::Index { .. } => true,
        _ => false,
    }
}
