//! JavaScript source frontend: lexer, parser and compiler for the documented
//! subset (`docs/design.md` §§7, 12–14).
//!
//! The accepted grammar, the rejected forms and the known gaps are listed in
//! `docs/grammar.md`; that document and this parser are kept in agreement by
//! the tests in `frontend/tests.rs`.
//!
//! * [`compile_script`] / [`compile_function`]: source text to a validated
//!   [`Executable`], via binding resolution and lowering (`frontend/lower.rs`;
//!   `docs/grammar.md`, "Execution"). Forms that parse but cannot execute yet
//!   (strings, property access) are compile errors. Functions, calls,
//!   closures and the compiler-reserved intrinsic binding `__rt`
//!   (`frontend/intrinsics.rs`; `docs/grammar.md`, "Reserved intrinsic
//!   binding") compile to VM instructions.
//! * [`ScriptOptions`] and the `_with` entry points ([`parse_script_with`],
//!   [`compile_script_with`], [`compile_function_with`]): non-strict
//!   (sloppy) source and `"use strict"` directive prologues
//!   (clj-quickjs-95); the entry points without options are strict.
//! * [`parse_script`]: source text to an [`ast::Script`], or the first
//!   [`SyntaxError`]. No AST is produced for input that uses an unsupported
//!   form; the error names the form and its position.
//! * [`parse_module`]: always-strict module source to an immutable, shareable
//!   [`ModuleRecord`] containing its parsed body and static import/export tables.
//!   [`crate::module`] loads static graphs; acyclic graphs link and execute through `Runtime::spawn_module`.
//! * [`lexer::Lexer`]: on-demand tokenizer used by the parser. It never
//!   decides between division and a regular expression literal; the parser
//!   requests the regexp lexical goal from grammatical context. Regular
//!   expressions parse, but compilation requires deferred RegExp objects.
//!
//! Every AST node carries a byte-offset [`Span`] into the source.
//! [`line_column`] converts an offset into the 1-based line and column used by
//! diagnostics. Neither stage panics on any input: malformed input, unknown
//! characters, truncated input and nesting deeper than [`MAX_NESTING`] are all
//! reported as [`SyntaxError`]s.

pub mod ast;
mod intrinsics;
pub mod lexer;
mod lower;
pub(crate) use lower::{ModuleBinding, lower_module, module_bindings};
mod module;
mod parser;
mod unicode_ident;

#[cfg(test)]
mod tests;

use std::fmt;

use crate::vm::Executable;

pub use module::{ImportEntry, ImportName, IndirectExportEntry, LocalExportEntry, ModuleRecord};

/// Nesting budget, in units: each nesting construct (parentheses, blocks,
/// functions, prefix operators, statement bodies, links of operator, member
/// and call chains) charges a cost weighted by the Rust stack it uses
/// (`docs/grammar.md`, "Nesting limit"). Input over budget is rejected with
/// [`SyntaxErrorKind::TooDeep`], so parsing at the limit fits a 2 MiB thread
/// stack even unoptimized. The same number also bounds the height of the
/// syntax tree (`height` on every statement, function and expression), so
/// dropping it and later recursive passes have a fixed depth bound.
pub const MAX_NESTING: usize = 256;

/// Half-open byte range `start..end` into the source text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Span {
    pub start: usize,
    pub end: usize,
}

impl Span {
    pub const fn new(start: usize, end: usize) -> Span {
        Span { start, end }
    }

    /// The smallest span covering both `self` and `other`.
    pub fn to(self, other: Span) -> Span {
        Span::new(self.start.min(other.start), self.end.max(other.end))
    }

    /// The source text this span covers.
    pub fn text(self, source: &str) -> &str {
        source.get(self.start..self.end).unwrap_or("")
    }
}

/// Category of a [`SyntaxError`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyntaxErrorKind {
    /// A character or character sequence that is not a token of the subset
    /// (for example `@`, `#`, `€`, or a character that may continue but not
    /// start an identifier).
    UnknownToken,
    /// A malformed token: an unterminated string or comment, a bad escape, or
    /// a malformed number literal.
    InvalidToken,
    /// The input ended inside a construct (truncated input).
    UnexpectedEnd,
    /// A token the grammar does not allow at this position.
    UnexpectedToken,
    /// Valid JavaScript that this subset deliberately does not support
    /// (classes, `async`, regular expression literals, `for`, ...).
    Unsupported,
    /// A reserved word (or a name the subset reserves) used as a binding
    /// name, assignment target or reference.
    ReservedWord,
    /// Syntax that JavaScript itself rejects as an early error (for example
    /// `return` outside a function, a `const` without an initializer, or a
    /// duplicate parameter name).
    Invalid,
    /// Nesting deeper than [`MAX_NESTING`].
    TooDeep,
    /// The compiled code would exceed a bytecode limit (65,535 simultaneously
    /// live bindings, 65,536 distinct constants, or 4 GiB of code).
    TooLarge,
    /// The compiler produced bytecode that the validator rejected. This is a
    /// compiler bug, reported instead of panicking.
    Internal,
}

/// A positioned frontend diagnostic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntaxError {
    pub kind: SyntaxErrorKind,
    pub message: String,
    /// The offending source range.
    pub span: Span,
    /// 1-based line of `span.start`.
    pub line: u32,
    /// 1-based column of `span.start`, counted in Unicode scalar values.
    pub column: u32,
}

impl SyntaxError {
    pub(crate) fn new(
        source: &str,
        kind: SyntaxErrorKind,
        span: Span,
        message: impl Into<String>,
    ) -> SyntaxError {
        let (line, column) = line_column(source, span.start);
        SyntaxError {
            kind,
            message: message.into(),
            span,
            line,
            column,
        }
    }
}

impl fmt::Display for SyntaxError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}:{}: SyntaxError: {}",
            self.line, self.column, self.message
        )
    }
}

impl std::error::Error for SyntaxError {}

/// 1-based `(line, column)` of byte `offset` in `source`. Lines end at LF,
/// CR, CRLF (one terminator), U+2028 and U+2029; columns count Unicode scalar
/// values. An offset past the end (or inside a multi-byte character) is
/// clamped to the preceding character boundary.
pub fn line_column(source: &str, offset: usize) -> (u32, u32) {
    let mut offset = offset.min(source.len());
    while !source.is_char_boundary(offset) {
        offset -= 1;
    }
    let mut line: u32 = 1;
    let mut column: u32 = 1;
    let mut chars = source[..offset].chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                line = line.saturating_add(1);
                column = 1;
            }
            '\n' | '\u{2028}' | '\u{2029}' => {
                line = line.saturating_add(1);
                column = 1;
            }
            _ => column = column.saturating_add(1),
        }
    }
    (line, column)
}

/// Parse `source` as a script of the documented subset (`docs/grammar.md`).
///
/// All code is treated as strict-mode code ([`ScriptOptions::STRICT`]; see
/// [`parse_script_with`]). Returns the first error; no AST is produced for
/// input containing an unsupported or malformed form.
pub fn parse_script(source: &str) -> Result<ast::Script, SyntaxError> {
    parse_script_with(source, ScriptOptions::STRICT)
}

/// Parse a module as strict code and construct its immutable static record.
/// This performs module early-error checks, without loading, linking, or
/// executing anything. The CLI and script compilation remain script-only.
pub fn parse_module(source: &str) -> Result<ast::Module, SyntaxError> {
    parser::Parser::new(source, true).parse_module()
}

/// [`parse_script`] with the top level in the mode `options` gives.
pub fn parse_script_with(source: &str, options: ScriptOptions) -> Result<ast::Script, SyntaxError> {
    parser::Parser::new(source, options.strict).parse_script()
}

/// How a frontend entry point treats its source (clj-quickjs-95).
///
/// `strict` is the mode of the script's top level. `true` makes all of the
/// code strict-mode code. `false` makes it non-strict ("sloppy") code, in
/// which a `"use strict"` directive prologue (`docs/grammar.md`, "Directive
/// prologues and strictness") makes that script or function, and every
/// function nested in it, strict. The entry points without options
/// ([`parse_script`], [`compile_script`], [`compile_function`]) stay strict,
/// as all code was before sloppy mode existed. ECMAScript scripts are sloppy
/// by default, so an upstream test script runs as written with
/// [`ScriptOptions::SLOPPY`].
///
/// The resulting mode of each script and function is recorded in the tree
/// ([`ast::Script::strict`], [`ast::Function::strict`]) and in its compiled
/// code ([`Executable::is_strict`]); sloppy-mode semantics branch on it
/// there (in the compiler, `FnCtx::strict` of the function being lowered).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ScriptOptions {
    pub strict: bool,
}

impl ScriptOptions {
    /// Strict-mode code throughout: the mode of the entry points without
    /// options.
    pub const STRICT: ScriptOptions = ScriptOptions { strict: true };
    /// Non-strict code, except where a `"use strict"` directive applies.
    pub const SLOPPY: ScriptOptions = ScriptOptions { strict: false };
}

/// Compile `source` as a script of the documented subset into a validated
/// [`Executable`] named `name` (`docs/grammar.md`, "Execution").
///
/// A task running it completes with the script's completion value, as
/// JavaScript `eval` would return it: the value of the last expression
/// statement that ran, or `undefined` (an `if` or `while` statement resets it
/// to `undefined` before running). Every instruction carries the
/// [`SourceLocation`](crate::SourceLocation) of the construct it was lowered
/// from.
///
/// Errors: any [`parse_script`] error; a redeclared binding in one scope
/// ([`SyntaxErrorKind::Invalid`], a JavaScript early error); forms that parse
/// but cannot execute yet: string values and property access
/// ([`SyntaxErrorKind::Unsupported`]); misuse of the reserved intrinsic
/// binding `__rt` ([`SyntaxErrorKind::ReservedWord`] or, for unknown
/// intrinsics and bad arguments, [`SyntaxErrorKind::Invalid`]); and bytecode limits
/// ([`SyntaxErrorKind::TooLarge`]). Unknown names, use of a binding before its
/// declaration ran, and assignment to a `const` are not compile errors: as in
/// JavaScript, they fail the task when (and only if) that code runs.
pub fn compile_script(name: &str, source: &str) -> Result<Executable, SyntaxError> {
    compile_script_with(name, source, ScriptOptions::STRICT)
}

/// [`compile_script`] with the top level in the mode `options` gives. The
/// script's code and each function's code record whether they are strict
/// ([`Executable::is_strict`]).
pub fn compile_script_with(
    name: &str,
    source: &str,
    options: ScriptOptions,
) -> Result<Executable, SyntaxError> {
    let script = parse_script_with(source, options)?;
    lower::lower_script(name, source, &script)
}

/// Compile an indirect eval script in the global environment.
pub(crate) fn compile_eval(name: &str, source: &str) -> Result<Executable, SyntaxError> {
    let script = parse_script_with(source, ScriptOptions::SLOPPY)?;
    lower::lower_eval(name, source, &script)
}

pub(crate) fn compile_direct_eval(
    name: &str,
    source: &str,
    site: &crate::vm::executable::EvalSite,
) -> Result<Executable, SyntaxError> {
    let mut parser = parser::Parser::new(source, site.strict);
    parser.allow_new_target(site.new_target.is_some());
    parser.class_initializer_eval(site.class_initializer);
    let script = parser.parse_script()?;
    lower::lower_direct_eval(name, source, &script, site)
}

/// Compile `source`, which must consist of exactly one function declaration,
/// into an executable whose parameters are that function's parameters: the
/// entry point of a task, spawned with arguments. `return` completes the
/// task; falling off the end returns `undefined`. The body follows the same
/// rules as [`compile_script`]. Inside the body, the function's own name is
/// the running function object (so it can recurse); assigning it is
/// [`SyntaxErrorKind::Unsupported`]. The function is strict
/// ([`ScriptOptions::STRICT`]; see [`compile_function_with`]).
pub fn compile_function(source: &str) -> Result<Executable, SyntaxError> {
    compile_function_with(source, ScriptOptions::STRICT)
}

/// [`compile_function`] with the declaration parsed as a script in the mode
/// `options` gives. With [`ScriptOptions::SLOPPY`] the function is strict
/// only when its own body starts with a `"use strict"` directive (a
/// script-level directive would be a second statement, which this entry
/// point rejects).
pub fn compile_function_with(
    source: &str,
    options: ScriptOptions,
) -> Result<Executable, SyntaxError> {
    let script = parse_script_with(source, options)?;
    match script.body.as_slice() {
        [
            ast::Stmt {
                kind: ast::StmtKind::FunctionDecl(function),
                ..
            },
        ] => lower::lower_function(source, function),
        _ => Err(SyntaxError::new(
            source,
            SyntaxErrorKind::Unsupported,
            script.span,
            "compile_function expects exactly one function declaration",
        )),
    }
}

/// 1-based `(line, column)` of each offset in `offsets`, which must be sorted
/// ascending: the same result as [`line_column`] for each, in one pass over
/// the source.
pub(crate) fn line_columns(source: &str, offsets: &[usize]) -> Vec<(u32, u32)> {
    let (mut line, mut column): (u32, u32) = (1, 1);
    let mut pos = 0;
    let mut after_cr = false;
    let mut out = Vec::with_capacity(offsets.len());
    for &offset in offsets {
        let mut target = offset.min(source.len());
        while !source.is_char_boundary(target) {
            target -= 1;
        }
        while pos < target {
            let Some(c) = source[pos..].chars().next() else {
                break;
            };
            pos += c.len_utf8();
            match c {
                // The LF of a CRLF pair ends no further line.
                '\n' if after_cr => {}
                '\r' | '\n' | '\u{2028}' | '\u{2029}' => {
                    line = line.saturating_add(1);
                    column = 1;
                }
                _ => column = column.saturating_add(1),
            }
            after_cr = c == '\r';
        }
        out.push((line, column));
    }
    out
}
