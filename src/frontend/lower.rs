//! Binding resolution and lowering of the syntax tree to VM bytecode
//! (`docs/grammar.md`, "Execution").
//!
//! One in-order pass over the tree, which is at most
//! [`MAX_NESTING`](super::MAX_NESTING) nodes high, so recursion depth is bounded.
//!
//! **Bindings.** Every `let`/`const` binding and every parameter lives in a
//! frame local. Entering a statement list (script, function body, block)
//! first declares all of its lexical bindings, so a reference anywhere in
//! that list resolves to them, even before their declaration; a redeclared
//! name in one list (or a `let` that repeats a parameter name) is a
//! JavaScript early error, reported as [`SyntaxErrorKind::Invalid`]. Sibling
//! blocks reuse local slots.
//!
//! **Temporal dead zone.** The subset has no closures, `break`, `continue` or
//! labels, so whether a binding is initialized at a reference is decided by
//! position alone: a reference inside the binding's own initializer or before
//! its declaration (in source order, within the statement list that declares
//! it) always runs before initialization, and any later reference always runs
//! after it. Such references, assignments to `const` bindings and references
//! to undeclared names lower to `ThrowError`, so they fail the task with a
//! ReferenceError or TypeError exactly when JavaScript would throw: only if
//! that code runs, after evaluating what JavaScript evaluates first (the
//! right-hand side of an assignment). Closures (clj-quickjs-20/21) make this
//! dynamic and will need a run-time check.
//!
//! **Numbers.** `+ - * / %` and `< > <= >=` apply JavaScript ToNumber to
//! operands that are not statically numbers (`ToNumber`/`ToNumberPair`)
//! before the number-only arithmetic instruction; with no string values in
//! the subset, that is exactly JavaScript's semantics for primitives. When
//! strings arrive, `+` and the relational operators must lower differently.
//!
//! **Completion value.** A script keeps its completion value in local 0 (see
//! [`super::compile_script`]); a function body discards expression values.

use std::collections::HashMap;
use std::sync::Arc;

use super::ast::{
    BinaryOp, Expr, ExprKind, Function, Ident, LexicalKind, LogicalOp, Script, Stmt, StmtKind,
    UnaryOp,
};
use super::lexer::{Lexer, TokenKind};
use super::{Span, SyntaxError, SyntaxErrorKind, line_columns};
use crate::error::SourceLocation;
use crate::value::Primitive;
use crate::vm::executable::{Executable, ExecutableSpec};
use crate::vm::instr::{Opcode, ThrowKind};

/// Locals are counted by a u16.
const MAX_LOCALS: usize = u16::MAX as usize;
/// Constants are indexed by a u16.
const MAX_CONSTANTS: usize = 1 << 16;

pub(crate) fn lower_script(
    name: &str,
    source: &str,
    script: &Script,
) -> Result<Executable, SyntaxError> {
    let mut lower = Lowerer::new(source, true);
    lower.statement_list(&script.body, &[])?;
    let end = Span::new(script.span.end, script.span.end);
    lower.out.op_u16(Opcode::GetLocal, 0, end);
    lower.out.op(Opcode::Return, end);
    lower.finish(name, 0)
}

pub(crate) fn lower_function(source: &str, function: &Function) -> Result<Executable, SyntaxError> {
    let mut lower = Lowerer::new(source, false);
    if function.params.len() > MAX_LOCALS {
        return Err(lower.error(
            SyntaxErrorKind::TooLarge,
            function.span,
            "too many parameters",
        ));
    }
    if let Some(name) = &function.name {
        // The declaration binds its name in the enclosing (script) scope.
        let binding = Binding {
            kind: BindingKind::EntryFunction,
            slot: u16::MAX,
            initialized: true,
        };
        lower
            .scopes
            .push(Scope::from([(name.name.clone(), binding)]));
    }
    lower.statement_list(&function.body, &function.params)?;
    let end = Span::new(function.span.end.saturating_sub(1), function.span.end);
    lower.out.op(Opcode::Undefined, end);
    lower.out.op(Opcode::Return, end);
    let name = function
        .name
        .as_ref()
        .map_or("anonymous", |n| n.name.as_str());
    lower.finish(name, function.params.len() as u16)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingKind {
    Let,
    Const,
    Param,
    /// The entry function's own name (`compile_function`). Function values
    /// are not supported yet, so using it is a compile error; it has no slot.
    EntryFunction,
}

struct Binding {
    kind: BindingKind,
    slot: u16,
    initialized: bool,
}

/// The bindings of one statement list, by name. A compiler-private map:
/// lookups never run user code.
type Scope = HashMap<String, Binding>;

#[derive(Clone, Copy)]
enum Resolved {
    Local {
        slot: u16,
        kind: BindingKind,
        initialized: bool,
    },
    Unresolvable,
}

/// Constant-pool deduplication key. Numbers compare by bit pattern, so `0`
/// and `-0` (and distinct NaN payloads) stay distinct constants.
#[derive(PartialEq, Eq, Hash)]
enum ConstKey {
    Null,
    Bool(bool),
    Number(u64),
}

/// Internal bytecode emitter: byte code, one span per instruction, a
/// deduplicated constant pool, and labels. Nothing outside this module can
/// construct code through it; its output goes through the validator like any
/// other code.
#[derive(Default)]
struct Emitter {
    code: Vec<u8>,
    spans: Vec<Span>,
    constants: Vec<Primitive>,
    const_index: HashMap<ConstKey, u16>,
    labels: Vec<Option<usize>>,
    /// (byte position of a u32 target operand, label)
    fixups: Vec<(usize, usize)>,
}

impl Emitter {
    fn op(&mut self, op: Opcode, span: Span) {
        self.code.push(op as u8);
        self.spans.push(span);
    }

    fn op_u16(&mut self, op: Opcode, operand: u16, span: Span) {
        self.op(op, span);
        self.code.extend_from_slice(&operand.to_le_bytes());
    }

    fn label(&mut self) -> usize {
        self.labels.push(None);
        self.labels.len() - 1
    }

    fn bind(&mut self, label: usize) {
        self.labels[label] = Some(self.code.len());
    }

    fn branch(&mut self, op: Opcode, label: usize, span: Span) {
        self.op(op, span);
        self.fixups.push((self.code.len(), label));
        self.code.extend_from_slice(&u32::MAX.to_le_bytes());
    }
}

struct Lowerer<'s> {
    source: &'s str,
    out: Emitter,
    /// Innermost last.
    scopes: Vec<Scope>,
    next_slot: usize,
    max_slots: usize,
    /// The completion-value local of a script; `None` in a function.
    completion: Option<u16>,
}

impl<'s> Lowerer<'s> {
    fn new(source: &'s str, script: bool) -> Lowerer<'s> {
        let completion = script.then_some(0);
        let slots = usize::from(script);
        Lowerer {
            source,
            out: Emitter::default(),
            scopes: Vec::new(),
            next_slot: slots,
            max_slots: slots,
            completion,
        }
    }

    fn error(&self, kind: SyntaxErrorKind, span: Span, message: impl Into<String>) -> SyntaxError {
        SyntaxError::new(self.source, kind, span, message)
    }

    fn unsupported(&self, span: Span, what: &str) -> SyntaxError {
        self.error(
            SyntaxErrorKind::Unsupported,
            span,
            format!("{what} cannot be executed yet"),
        )
    }

    fn finish(self, name: &str, params: u16) -> Result<Executable, SyntaxError> {
        let Lowerer {
            source,
            mut out,
            max_slots,
            ..
        } = self;
        let too_large = |what: &str| {
            let span = Span::new(0, source.len());
            SyntaxError::new(source, SyntaxErrorKind::TooLarge, span, what.to_string())
        };
        if u32::try_from(out.code.len()).is_err() {
            return Err(too_large("compiled code exceeds 4 GiB"));
        }
        for &(at, label) in &out.fixups {
            // Every label is bound before `finish`; an unbound one keeps the
            // invalid target, which validation rejects.
            if let Some(target) = out.labels[label] {
                out.code[at..at + 4].copy_from_slice(&(target as u32).to_le_bytes());
            }
        }
        let mut starts: Vec<usize> = out.spans.iter().map(|s| s.start).collect();
        starts.sort_unstable();
        starts.dedup();
        let positions = line_columns(source, &starts);
        let locations = out
            .spans
            .iter()
            .map(|&span| {
                let i = starts.partition_point(|&s| s < span.start);
                let (line, column) = positions[i];
                SourceLocation { span, line, column }
            })
            .collect();
        let spec = ExecutableSpec {
            name: name.to_string(),
            params,
            locals: max_slots as u16,
            code: out.code,
            constants: out.constants,
            functions: Vec::new(),
        };
        Executable::load_compiled(spec, Arc::from(source), locations).map_err(|e| {
            let span = Span::new(0, source.len());
            SyntaxError::new(
                source,
                SyntaxErrorKind::Internal,
                span,
                format!("compiler produced invalid bytecode: {e}"),
            )
        })
    }

    // ----- bindings -----

    /// Enter a statement list: declare `params` (a function body) and every
    /// lexical binding of `body`, then lower `body`.
    fn statement_list(&mut self, body: &[Stmt], params: &[Ident]) -> Result<(), SyntaxError> {
        let saved_slot = self.next_slot;
        self.scopes.push(Scope::new());
        for p in params {
            self.declare(p, BindingKind::Param)?;
        }
        for stmt in body {
            match &stmt.kind {
                StmtKind::Lexical(decl) => {
                    let kind = match decl.kind {
                        LexicalKind::Let => BindingKind::Let,
                        LexicalKind::Const => BindingKind::Const,
                    };
                    for d in &decl.declarators {
                        self.declare(&d.name, kind)?;
                    }
                }
                StmtKind::FunctionDecl(_) => {
                    return Err(self.unsupported(stmt.span, "a function declaration"));
                }
                _ => {}
            }
        }
        for stmt in body {
            self.statement(stmt)?;
        }
        self.scopes.pop();
        self.next_slot = saved_slot;
        Ok(())
    }

    fn declare(&mut self, name: &Ident, kind: BindingKind) -> Result<(), SyntaxError> {
        let prior = self.scopes.last().and_then(|scope| scope.get(&name.name));
        if let Some(prior) = prior {
            let what = if prior.kind == BindingKind::Param {
                "a parameter"
            } else {
                "a binding in this scope"
            };
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                name.span,
                format!("'{}' is already declared as {what}", name.name),
            ));
        }
        if self.next_slot >= MAX_LOCALS {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                name.span,
                "more than 65,535 bindings are live at once",
            ));
        }
        let slot = self.next_slot as u16;
        self.next_slot += 1;
        self.max_slots = self.max_slots.max(self.next_slot);
        let binding = Binding {
            kind,
            slot,
            initialized: kind == BindingKind::Param,
        };
        if let Some(scope) = self.scopes.last_mut() {
            scope.insert(name.name.clone(), binding);
        }
        Ok(())
    }

    fn resolve(&self, name: &str) -> Resolved {
        self.scopes
            .iter()
            .rev()
            .find_map(|scope| scope.get(name))
            .map_or(Resolved::Unresolvable, |b| Resolved::Local {
                slot: b.slot,
                kind: b.kind,
                initialized: b.initialized,
            })
    }

    /// Mark the innermost binding `name` initialized; returns its slot.
    fn initialize(&mut self, name: &Ident) -> Result<u16, SyntaxError> {
        let binding = self
            .scopes
            .last_mut()
            .and_then(|scope| scope.get_mut(&name.name));
        match binding {
            Some(b) => {
                b.initialized = true;
                Ok(b.slot)
            }
            None => Err(self.error(
                SyntaxErrorKind::Internal,
                name.span,
                "declaration was not pre-declared",
            )),
        }
    }

    /// The span of the identifier token `name` within `span`, which may also
    /// cover parentheses and comments around it (`( /* c */ x )`), so
    /// diagnostics point at, and quote, the name itself.
    fn name_span(&self, name: &str, span: Span) -> Span {
        let mut lexer = Lexer::new(span.text(self.source));
        while let Ok(token) = lexer.next_token() {
            match token.kind {
                TokenKind::Identifier(n) if n == name => {
                    return Span::new(span.start + token.span.start, span.start + token.span.end);
                }
                TokenKind::Eof => break,
                _ => {}
            }
        }
        span
    }

    fn throw(&mut self, kind: ThrowKind, span: Span) {
        self.out.op_u16(Opcode::ThrowError, kind as u16, span);
    }

    // ----- statements -----

    /// Reset the script completion value to `undefined` (for `if`/`while`).
    fn reset_completion(&mut self, span: Span) {
        if let Some(slot) = self.completion {
            self.out.op(Opcode::Undefined, span);
            self.out.op_u16(Opcode::SetLocal, slot, span);
        }
    }

    fn statement(&mut self, stmt: &Stmt) -> Result<(), SyntaxError> {
        let span = stmt.span;
        match &stmt.kind {
            StmtKind::Expr(e) => {
                self.expr(e)?;
                match self.completion {
                    Some(slot) => self.out.op_u16(Opcode::SetLocal, slot, span),
                    None => self.out.op(Opcode::Pop, span),
                }
            }
            StmtKind::Lexical(decl) => {
                for d in &decl.declarators {
                    match &d.init {
                        Some(init) => self.expr(init)?,
                        None => self.out.op(Opcode::Undefined, d.span),
                    }
                    let slot = self.initialize(&d.name)?;
                    self.out.op_u16(Opcode::SetLocal, slot, d.span);
                }
            }
            StmtKind::Return(value) => {
                if self.completion.is_some() {
                    // The parser rejects this; kept as a guard.
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        span,
                        "return outside a function",
                    ));
                }
                match value {
                    Some(e) => self.expr(e)?,
                    None => self.out.op(Opcode::Undefined, span),
                }
                self.out.op(Opcode::Return, span);
            }
            StmtKind::Block(body) => self.statement_list(body, &[])?,
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.reset_completion(span);
                self.expr(test)?;
                let otherwise = self.out.label();
                self.out.branch(Opcode::JumpIfFalse, otherwise, test.span);
                self.statement(consequent)?;
                match alternate {
                    Some(alternate) => {
                        let end = self.out.label();
                        self.out.branch(Opcode::Jump, end, span);
                        self.out.bind(otherwise);
                        self.statement(alternate)?;
                        self.out.bind(end);
                    }
                    None => self.out.bind(otherwise),
                }
            }
            StmtKind::While { test, body } => {
                self.reset_completion(span);
                let start = self.out.label();
                let end = self.out.label();
                self.out.bind(start);
                self.expr(test)?;
                self.out.branch(Opcode::JumpIfFalse, end, test.span);
                self.statement(body)?;
                self.out.branch(Opcode::Jump, start, span);
                self.out.bind(end);
            }
            StmtKind::Empty => {}
            StmtKind::FunctionDecl(_) => {
                return Err(self.unsupported(span, "a function declaration"));
            }
        }
        Ok(())
    }

    // ----- expressions -----

    fn constant(&mut self, value: Primitive, span: Span) -> Result<(), SyntaxError> {
        let key = match value {
            Primitive::Null => ConstKey::Null,
            Primitive::Bool(b) => ConstKey::Bool(b),
            Primitive::Number(n) => ConstKey::Number(n.to_bits()),
            Primitive::Undefined => {
                self.out.op(Opcode::Undefined, span);
                return Ok(());
            }
        };
        let index = match self.out.const_index.get(&key) {
            Some(&index) => index,
            None => {
                if self.out.constants.len() >= MAX_CONSTANTS {
                    return Err(self.error(
                        SyntaxErrorKind::TooLarge,
                        span,
                        "more than 65,536 distinct constants",
                    ));
                }
                let index = self.out.constants.len() as u16;
                self.out.constants.push(value);
                self.out.const_index.insert(key, index);
                index
            }
        };
        self.out.op_u16(Opcode::Const, index, span);
        Ok(())
    }

    fn expr(&mut self, e: &Expr) -> Result<(), SyntaxError> {
        let span = e.span;
        match &e.kind {
            ExprKind::Number(n) => self.constant(Primitive::Number(*n), span)?,
            ExprKind::Bool(b) => self.constant(Primitive::Bool(*b), span)?,
            ExprKind::Null => self.constant(Primitive::Null, span)?,
            ExprKind::Undefined => self.out.op(Opcode::Undefined, span),
            ExprKind::String(_) => return Err(self.unsupported(span, "a string value")),
            ExprKind::Identifier(name) => {
                let at = self.name_span(name, span);
                match self.resolve(name) {
                    Resolved::Local {
                        kind: BindingKind::EntryFunction,
                        ..
                    } => return Err(self.unsupported(at, "a reference to a function value")),
                    Resolved::Local {
                        slot,
                        initialized: true,
                        ..
                    } => self.out.op_u16(Opcode::GetLocal, slot, at),
                    Resolved::Local { .. } => self.throw(ThrowKind::Uninitialized, at),
                    Resolved::Unresolvable => self.throw(ThrowKind::Unresolvable, at),
                }
            }
            ExprKind::Unary { op, operand } => {
                self.expr(operand)?;
                match op {
                    UnaryOp::Not => self.out.op(Opcode::Not, span),
                    UnaryOp::Neg | UnaryOp::Plus => {
                        if !is_number(operand) {
                            self.out.op(Opcode::ToNumber, span);
                        }
                        if *op == UnaryOp::Neg {
                            self.out.op(Opcode::Neg, span);
                        }
                    }
                }
            }
            ExprKind::Binary { op, left, right } => {
                self.expr(left)?;
                self.expr(right)?;
                self.binary(*op, left, right, span);
            }
            ExprKind::Logical { op, left, right } => {
                // The left value is the result unless it selects the right.
                self.expr(left)?;
                let end = self.out.label();
                self.out.op(Opcode::Dup, span);
                if *op == LogicalOp::Or {
                    self.out.op(Opcode::Not, span);
                }
                self.out.branch(Opcode::JumpIfFalse, end, span);
                self.out.op(Opcode::Pop, span);
                self.expr(right)?;
                self.out.bind(end);
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expr(test)?;
                let otherwise = self.out.label();
                let end = self.out.label();
                self.out.branch(Opcode::JumpIfFalse, otherwise, test.span);
                self.expr(consequent)?;
                self.out.branch(Opcode::Jump, end, span);
                self.out.bind(otherwise);
                self.expr(alternate)?;
                self.out.bind(end);
            }
            ExprKind::Assign { target, value } => {
                self.expr(value)?;
                let at = self.name_span(&target.name, target.span);
                match self.resolve(&target.name) {
                    Resolved::Local {
                        kind: BindingKind::EntryFunction,
                        ..
                    } => return Err(self.unsupported(at, "assignment to a function binding")),
                    Resolved::Local {
                        initialized: false, ..
                    } => {
                        self.out.op(Opcode::Pop, at);
                        self.throw(ThrowKind::Uninitialized, at);
                    }
                    Resolved::Local {
                        kind: BindingKind::Const,
                        ..
                    } => {
                        self.out.op(Opcode::Pop, at);
                        self.throw(ThrowKind::ConstAssignment, at);
                    }
                    Resolved::Local { slot, .. } => {
                        self.out.op(Opcode::Dup, at);
                        self.out.op_u16(Opcode::SetLocal, slot, at);
                    }
                    Resolved::Unresolvable => {
                        self.out.op(Opcode::Pop, at);
                        self.throw(ThrowKind::Unresolvable, at);
                    }
                }
            }
            ExprKind::Call { .. } => return Err(self.unsupported(span, "a function call")),
            ExprKind::Member { .. } => return Err(self.unsupported(span, "property access")),
            ExprKind::Function(_) => {
                return Err(self.unsupported(span, "a function expression"));
            }
        }
        Ok(())
    }

    /// Emit a binary operator; both operands are on the stack.
    fn binary(&mut self, op: BinaryOp, left: &Expr, right: &Expr, span: Span) {
        let numeric = match op {
            BinaryOp::Mul => Opcode::Mul,
            BinaryOp::Div => Opcode::Div,
            BinaryOp::Rem => Opcode::Rem,
            BinaryOp::Add => Opcode::Add,
            BinaryOp::Sub => Opcode::Sub,
            BinaryOp::Lt => Opcode::Lt,
            BinaryOp::Gt => Opcode::Gt,
            BinaryOp::Le => Opcode::Le,
            BinaryOp::Ge => Opcode::Ge,
            BinaryOp::LooseEq | BinaryOp::LooseNe | BinaryOp::StrictEq | BinaryOp::StrictNe => {
                let (compare, negate) = match op {
                    BinaryOp::LooseEq => (Opcode::LooseEq, false),
                    BinaryOp::LooseNe => (Opcode::LooseEq, true),
                    BinaryOp::StrictEq => (Opcode::StrictEq, false),
                    _ => (Opcode::StrictEq, true),
                };
                self.out.op(compare, span);
                if negate {
                    self.out.op(Opcode::Not, span);
                }
                return;
            }
        };
        // ToNumber(left) must happen after the right operand is evaluated,
        // so a non-number left operand converts both together.
        if !is_number(left) {
            self.out.op(Opcode::ToNumberPair, span);
        } else if !is_number(right) {
            self.out.op(Opcode::ToNumber, span);
        }
        self.out.op(numeric, span);
    }
}

/// True when `e` always evaluates to a Number (if it completes), so no
/// ToNumber is needed.
fn is_number(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Number(_) => true,
        ExprKind::Unary { op, .. } => matches!(op, UnaryOp::Neg | UnaryOp::Plus),
        ExprKind::Binary { op, .. } => matches!(
            op,
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::Rem | BinaryOp::Add | BinaryOp::Sub
        ),
        ExprKind::Conditional {
            consequent,
            alternate,
            ..
        } => is_number(consequent) && is_number(alternate),
        ExprKind::Assign { value, .. } => is_number(value),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::super::{compile_script, line_column, line_columns};

    #[test]
    fn batched_line_columns_match_line_column_at_every_offset() {
        let source = "a\r\nb\rc\n\u{2028}d\u{2029}é\r\r\n\nxyz\u{1F600}w\r";
        let offsets: Vec<usize> = (0..=source.len() + 2).collect();
        let batched = line_columns(source, &offsets);
        for (&offset, &got) in offsets.iter().zip(&batched) {
            assert_eq!(got, line_column(source, offset), "offset {offset}");
        }
    }

    #[test]
    fn every_instruction_carries_a_location_inside_the_source() {
        let source = "let a = 1;\r\nwhile (a < 3) {\n  a = a + 1;\n}\nif (a) { -a } else { b }";
        let exe = compile_script("t", source).unwrap();
        for i in 0..exe.instruction_count() {
            let at = exe.source_location(i).unwrap();
            assert!(at.span.start <= at.span.end && at.span.end <= source.len());
            assert_eq!((at.line, at.column), line_column(source, at.span.start));
        }
        // The `b` reference (an unknown name) is located at line 5, column 22.
        let throw = (0..exe.instruction_count())
            .map(|i| exe.source_location(i).unwrap())
            .find(|at| at.span.text(source) == "b")
            .unwrap();
        assert_eq!((throw.line, throw.column), (5, 22));
    }
}
