//! Binding resolution and lowering of the syntax tree to VM bytecode
//! (`docs/grammar.md`, "Execution"; `docs/architecture.md`, "Source
//! compilation").
//!
//! The tree is lowered twice, by the same code: an **analysis** pass that
//! discards its output and records per-binding [`Facts`] (is the binding
//! assigned? captured by a closure, and was it initialized when that closure
//! was created?), then an **emit** pass that uses those facts to choose each
//! binding's representation. Each pass is one in-order walk; nested
//! functions are lowered inline where they occur, so the work is linear in
//! the program and recursion depth is bounded by the tree height
//! ([`MAX_NESTING`](super::MAX_NESTING)). Both passes resolve every name
//! identically, so the facts of one apply to the other.
//!
//! **Functions.** Each function is its own [`Executable`], loaded bottom-up
//! into its parent's child function table (`ExecutableSpec::functions`); a
//! function expression lowers to `function i`, a call to `call n`. Function
//! declarations are hoisted: entering a statement list creates its
//! declarations' closures, in source order, before any statement runs. An
//! arrow function lowers exactly as an anonymous function expression (its
//! concise body is already a `return` statement in the tree): the subset
//! has no `this`, `arguments`, `new.target` or `super`, the only bindings
//! an arrow would take lexically instead of having its own.
//!
//! **Bindings.** Every `let`/`const`/function binding and every parameter
//! lives in a frame local. Entering a statement list (script, function body,
//! block) first declares all of its bindings, so a reference anywhere in the
//! list resolves to them; a redeclared name is a JavaScript early error,
//! [`SyntaxErrorKind::Invalid`]. Sibling blocks reuse local slots. Names are
//! resolved through the stack of enclosing functions, as QuickJS's
//! `get_closure_var` does: a binding of an enclosing function becomes a
//! capture of every function in between (`ExecutableSpec::captures`), passed
//! down by the parent when it creates the closure (`get_local`,
//! `get_capture` or `current_function`, then `function i`).
//!
//! **Representation of captured bindings.** A binding that no closure
//! captures is a plain local. A captured binding is copied into the closure
//! (by value) when that is unobservable: it is never assigned after its
//! initialization and it is initialized whenever a closure capturing it is
//! created. Otherwise it lives in a binding cell (`new_cell` at scope entry,
//! `cell_init` at its declaration, `cell_get`/`cell_set` at uses), shared by
//! the frame and every closure over it, so closures observe assignments and
//! a closure that runs before the declaration fails with the cell's run-time
//! ReferenceError. A reference to a function's own name from inside it
//! (a named function expression, or a function declaration whose binding is
//! never assigned) is `current_function`, so a self-recursive function
//! captures nothing.
//!
//! **Temporal dead zone.** Within the function that declares a binding,
//! whether it is initialized at a reference is still decided by position:
//! the subset has no `break`, `continue` or labels, so code of one function
//! runs in source order except that an exception or a `return` may leave a
//! `try` block early for its catch or finally block. Those blocks follow the
//! whole `try` block, and every binding declared inside it is out of their
//! scope, so a reference before the declaration (or inside its own
//! initializer) still always runs before initialization, and one after it
//! always runs after. Such references, assignments to `const` bindings and
//! references to undeclared names lower to `throw_error`. Only closures can
//! run a reference at another time, and they use the run-time check above.
//!
//! **Reserved intrinsic binding.** `__rt` resolves to the outermost scope;
//! see [`super::intrinsics`] for the contract and the table.
//!
//! **Numbers.** `+ - * / %` and `< > <= >=` apply JavaScript ToNumber to
//! operands that are not statically numbers (`ToNumber`/`ToNumberPair`)
//! before the number-only arithmetic instruction; with no string values in
//! the subset, that is exactly JavaScript's semantics for primitives. When
//! strings arrive, `+` and the relational operators must lower differently.
//!
//! **Completion value.** A script keeps its completion value in local 0 (see
//! [`super::compile_script`]); a function body discards expression values.
//!
//! **Exceptions.** `throw` lowers to `throw`, and `try` statements to the
//! VM handler instructions (`Lowerer::try_statement`); a `return` needs no
//! special lowering, since the VM runs the frame's finally blocks itself.
//! A catch parameter is a binding of the catch body's scope, initialized
//! from the exception value.

use std::collections::HashMap;
use std::sync::Arc;

use super::ast::{
    BinaryOp, CatchClause, Expr, ExprKind, Function, Ident, LexicalKind, LogicalOp, Script, Stmt,
    StmtKind, UnaryOp,
};
use super::intrinsics::{self, Arity, Lowering, RESERVED_BINDING};
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
/// Child functions are indexed by a u16.
const MAX_FUNCTIONS: usize = 1 << 16;
/// Call arguments and captures are counted by a u16.
const MAX_COUNT: usize = u16::MAX as usize;

pub(crate) fn lower_script(
    name: &str,
    source: &str,
    script: &Script,
) -> Result<Executable, SyntaxError> {
    let lower = |facts| {
        let mut lower = Lowerer::new(source, facts);
        lower.enter_function(None, true);
        lower.statement_list(&script.body, &[], true)?;
        let end = Span::new(script.span.end, script.span.end);
        lower.out().op_u16(Opcode::GetLocal, 0, end);
        lower.out().op(Opcode::Return, end);
        Ok::<_, SyntaxError>(lower)
    };
    let facts = lower(None)?.facts;
    let mut lower = lower(Some(facts))?;
    lower.finish_top(name, 0)
}

pub(crate) fn lower_function(source: &str, function: &Function) -> Result<Executable, SyntaxError> {
    let lower = |facts| {
        let mut lower = Lowerer::new(source, facts);
        if function.params.len() > MAX_LOCALS {
            return Err(lower.error(
                SyntaxErrorKind::TooLarge,
                function.span,
                "too many parameters",
            ));
        }
        let self_name = match &function.name {
            Some(name) => {
                lower.check_not_reserved(name)?;
                Some((name.name.clone(), SelfKind::Entry))
            }
            None => None,
        };
        lower.enter_function(self_name, false);
        lower.statement_list(&function.body, &function.params, true)?;
        let end = Span::new(function.span.end.saturating_sub(1), function.span.end);
        lower.out().op(Opcode::Undefined, end);
        lower.out().op(Opcode::Return, end);
        Ok(lower)
    };
    let facts = lower(None)?.facts;
    let mut lower = lower(Some(facts))?;
    let name = function
        .name
        .as_ref()
        .map_or("anonymous", |n| n.name.as_str());
    lower.finish_top(name, function.params.len() as u16)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingKind {
    Let,
    Const,
    Param,
    /// A function declaration's name.
    Function,
}

/// Where a binding's value lives.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Repr {
    /// In the frame local (or, captured, copied into the closure).
    Value,
    /// In a binding cell held by the frame local and every closure over it.
    Cell,
}

#[derive(Clone, Copy)]
struct Binding {
    kind: BindingKind,
    slot: u16,
    initialized: bool,
    repr: Repr,
    /// Identity: the byte offset of the declaring identifier.
    key: usize,
}

/// The bindings of one statement list, by name. A compiler-private map:
/// lookups never run user code.
type Scope = HashMap<String, Binding>;

/// What the analysis pass learns about one binding (keyed by
/// [`Binding::key`]).
#[derive(Clone, Copy, Default)]
struct Facts {
    /// Some assignment targets it.
    assigned: bool,
    /// A closure capturing it is created while it is initialized.
    captured_initialized: bool,
    /// A closure capturing it may be created before it is initialized.
    captured_uninitialized: bool,
    /// (Function declarations) its own body refers to it.
    self_referenced: bool,
}

impl Facts {
    fn repr(self) -> Repr {
        let captured = self.captured_initialized || self.self_referenced;
        if self.captured_uninitialized || (self.assigned && captured) {
            Repr::Cell
        } else {
            Repr::Value
        }
    }
}

/// The self-binding of a function: its own name, seen from inside it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SelfKind {
    /// A named function expression's name: immutable, `current_function`.
    Expression,
    /// A function declaration's name, while its binding is never assigned.
    Declaration { key: usize },
    /// The entry function of `compile_function`.
    Entry,
}

/// What a name refers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Target {
    Binding(BindingKind),
    SelfFn(SelfKind),
}

/// How the running function reaches a binding.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Place {
    Local(u16),
    Capture(u16),
    /// `current_function`.
    Current,
}

#[derive(Clone, Copy, Debug)]
enum Access {
    Found {
        place: Place,
        target: Target,
        repr: Repr,
        /// False only for a local of the running function referenced before
        /// its declaration ran (decided by position).
        initialized: bool,
        /// Identity of the binding (or self-binding), for capture dedup.
        key: usize,
    },
    /// The reserved intrinsic binding `__rt`.
    Intrinsic,
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

    /// Push the value at `place` (a cell itself for a cell binding).
    fn load(&mut self, place: Place, span: Span) {
        match place {
            Place::Local(slot) => self.op_u16(Opcode::GetLocal, slot, span),
            Place::Capture(i) => self.op_u16(Opcode::GetCapture, i, span),
            Place::Current => self.op(Opcode::CurrentFunction, span),
        }
    }
}

/// One function being lowered.
struct FnCtx {
    out: Emitter,
    /// Innermost last.
    scopes: Vec<Scope>,
    next_slot: usize,
    max_slots: usize,
    /// The completion-value local of a script; `None` in a function.
    completion: Option<u16>,
    /// The function's own name and how it may be referred to from inside.
    self_name: Option<(String, SelfKind)>,
    /// Identity of the self-binding (never a source offset).
    self_key: usize,
    /// Captured values, in capture-index order: how the parent loads each.
    captures: Vec<Place>,
    capture_index: HashMap<usize, u16>,
    /// Child functions, as indices into `Lowerer::sealed` (emit pass only).
    children: Vec<usize>,
    child_count: usize,
}

/// A lowered function waiting to be loaded ([`Lowerer::load_all`]).
struct Sealed {
    name: String,
    params: u16,
    locals: u16,
    captures: u16,
    code: Vec<u8>,
    spans: Vec<Span>,
    constants: Vec<Primitive>,
    children: Vec<usize>,
}

struct Lowerer<'s> {
    source: &'s str,
    shared_source: Arc<str>,
    /// Enclosing functions, innermost (the one being lowered) last.
    fns: Vec<FnCtx>,
    /// Finished functions, children before their parents (emit pass only).
    sealed: Vec<Sealed>,
    /// `None` during the analysis pass, which records into `facts`.
    emit: Option<HashMap<usize, Facts>>,
    facts: HashMap<usize, Facts>,
    next_self_key: usize,
}

impl<'s> Lowerer<'s> {
    fn new(source: &'s str, facts: Option<HashMap<usize, Facts>>) -> Lowerer<'s> {
        Lowerer {
            source,
            shared_source: Arc::from(source),
            fns: Vec::new(),
            sealed: Vec::new(),
            emit: facts,
            facts: HashMap::new(),
            next_self_key: usize::MAX,
        }
    }

    fn analyzing(&self) -> bool {
        self.emit.is_none()
    }

    fn fact(&mut self, key: usize) -> &mut Facts {
        self.facts.entry(key).or_default()
    }

    fn facts_of(&self, key: usize) -> Facts {
        self.emit
            .as_ref()
            .and_then(|f| f.get(&key).copied())
            .unwrap_or_default()
    }

    fn cur(&self) -> &FnCtx {
        self.fns.last().expect("a function is being lowered")
    }

    fn cur_mut(&mut self) -> &mut FnCtx {
        self.fns.last_mut().expect("a function is being lowered")
    }

    fn out(&mut self) -> &mut Emitter {
        &mut self.cur_mut().out
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

    fn internal(&self, span: Span, what: &str) -> SyntaxError {
        self.error(SyntaxErrorKind::Internal, span, what.to_string())
    }

    fn enter_function(&mut self, self_name: Option<(String, SelfKind)>, script: bool) {
        let slots = usize::from(script);
        let self_key = self.next_self_key;
        self.next_self_key -= 1;
        self.fns.push(FnCtx {
            out: Emitter::default(),
            scopes: Vec::new(),
            next_slot: slots,
            max_slots: slots,
            completion: script.then_some(0),
            self_name,
            self_key,
            captures: Vec::new(),
            capture_index: HashMap::new(),
            children: Vec::new(),
            child_count: 0,
        });
    }

    /// Seal the outermost function and load the whole function tree (emit
    /// pass).
    fn finish_top(&mut self, name: &str, params: u16) -> Result<Executable, SyntaxError> {
        let ctx = self.fns.pop().expect("the entry function");
        self.seal(ctx, name, params)?;
        self.load_all()
    }

    /// Resolve `ctx`'s branch targets and keep its code for
    /// [`Self::load_all`]; returns its index in `sealed`.
    fn seal(&mut self, ctx: FnCtx, name: &str, params: u16) -> Result<usize, SyntaxError> {
        let FnCtx {
            mut out,
            max_slots,
            captures,
            children,
            ..
        } = ctx;
        if u32::try_from(out.code.len()).is_err() {
            let span = Span::new(0, self.source.len());
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "compiled code exceeds 4 GiB",
            ));
        }
        for &(at, label) in &out.fixups {
            // Every label is bound before sealing; an unbound one keeps the
            // invalid target, which validation rejects.
            if let Some(target) = out.labels[label] {
                out.code[at..at + 4].copy_from_slice(&(target as u32).to_le_bytes());
            }
        }
        self.sealed.push(Sealed {
            name: name.to_string(),
            params,
            locals: max_slots as u16,
            captures: captures.len() as u16,
            code: out.code,
            spans: out.spans,
            constants: out.constants,
            children,
        });
        Ok(self.sealed.len() - 1)
    }

    /// Load every sealed function, children before parents (the order they
    /// were sealed in), and return the last one, the outermost. The source
    /// positions of all instructions are computed in one pass over the
    /// source, however many functions there are.
    fn load_all(&mut self) -> Result<Executable, SyntaxError> {
        let source = self.source;
        let sealed = std::mem::take(&mut self.sealed);
        let mut starts: Vec<usize> = sealed
            .iter()
            .flat_map(|f| f.spans.iter().map(|s| s.start))
            .collect();
        starts.sort_unstable();
        starts.dedup();
        let positions = line_columns(source, &starts);
        let mut loaded: Vec<Option<Executable>> = Vec::with_capacity(sealed.len());
        for f in sealed {
            let locations = f
                .spans
                .iter()
                .map(|&span| {
                    let i = starts.partition_point(|&s| s < span.start);
                    let (line, column) = positions[i];
                    SourceLocation { span, line, column }
                })
                .collect();
            // Each function has exactly one parent, sealed after it.
            let functions = f
                .children
                .iter()
                .map(|&i| loaded.get_mut(i).and_then(Option::take))
                .collect::<Option<Vec<_>>>()
                .ok_or_else(|| self.internal(Span::new(0, 0), "child function sealed late"))?;
            let spec = ExecutableSpec {
                name: f.name,
                params: f.params,
                locals: f.locals,
                code: f.code,
                constants: f.constants,
                functions,
                captures: f.captures,
            };
            let shared = Arc::clone(&self.shared_source);
            let exe = Executable::load_compiled(spec, shared, locations).map_err(|e| {
                self.error(
                    SyntaxErrorKind::Internal,
                    Span::new(0, source.len()),
                    format!("compiler produced invalid bytecode: {e}"),
                )
            })?;
            loaded.push(Some(exe));
        }
        loaded
            .pop()
            .flatten()
            .ok_or_else(|| self.internal(Span::new(0, 0), "no function was compiled"))
    }

    // ----- bindings -----

    /// Reject a declaration of the reserved intrinsic binding.
    fn check_not_reserved(&self, name: &Ident) -> Result<(), SyntaxError> {
        if name.name == RESERVED_BINDING {
            return Err(self.error(
                SyntaxErrorKind::ReservedWord,
                name.span,
                format!(
                    "`{RESERVED_BINDING}` is the compiler-reserved intrinsic binding and cannot be declared or shadowed"
                ),
            ));
        }
        Ok(())
    }

    /// Enter a statement list: declare `params` (a function body) and every
    /// binding of `body`, create the cells of its cell bindings and the
    /// closures of its function declarations, then lower `body`.
    /// `top_level`: a script or function body, not a block.
    fn statement_list(
        &mut self,
        body: &[Stmt],
        params: &[Ident],
        top_level: bool,
    ) -> Result<(), SyntaxError> {
        let saved_slot = self.cur().next_slot;
        self.cur_mut().scopes.push(Scope::new());
        let mut declared: Vec<&Ident> = Vec::new();
        for p in params {
            self.declare(p, BindingKind::Param, top_level)?;
            declared.push(p);
        }
        for stmt in body {
            match &stmt.kind {
                StmtKind::Lexical(decl) => {
                    let kind = match decl.kind {
                        LexicalKind::Let => BindingKind::Let,
                        LexicalKind::Const => BindingKind::Const,
                    };
                    for d in &decl.declarators {
                        self.declare(&d.name, kind, top_level)?;
                        declared.push(&d.name);
                    }
                }
                StmtKind::FunctionDecl(f) => {
                    let name = self.declaration_name(f)?;
                    self.declare(name, BindingKind::Function, top_level)?;
                    declared.push(name);
                }
                _ => {}
            }
        }
        // Cells first, so hoisted closures can capture any of them.
        for name in declared {
            let b = self.local(name)?;
            if b.repr != Repr::Cell {
                continue;
            }
            let span = name.span;
            self.out().op(Opcode::NewCell, span);
            if b.kind == BindingKind::Param {
                self.out().op(Opcode::Dup, span);
                self.out().op_u16(Opcode::GetLocal, b.slot, span);
                self.out().op(Opcode::CellInit, span);
            }
            self.out().op_u16(Opcode::SetLocal, b.slot, span);
        }
        // Hoisted function declarations, in source order.
        for stmt in body {
            if let StmtKind::FunctionDecl(f) = &stmt.kind {
                let name = self.declaration_name(f)?;
                let b = self.local(name)?;
                let facts = self.facts_of(b.key);
                let self_name = (self.analyzing() || !facts.assigned)
                    .then(|| (name.name.clone(), SelfKind::Declaration { key: b.key }));
                if b.repr == Repr::Cell {
                    self.out().op_u16(Opcode::GetLocal, b.slot, f.span);
                    self.closure(f, self_name)?;
                    self.out().op(Opcode::CellInit, f.span);
                } else {
                    self.closure(f, self_name)?;
                    self.out().op_u16(Opcode::SetLocal, b.slot, f.span);
                }
                self.initialize(name)?;
            }
        }
        for stmt in body {
            self.statement(stmt)?;
        }
        let ctx = self.cur_mut();
        ctx.scopes.pop();
        ctx.next_slot = saved_slot;
        Ok(())
    }

    fn declaration_name<'f>(&self, f: &'f Function) -> Result<&'f Ident, SyntaxError> {
        f.name
            .as_ref()
            .ok_or_else(|| self.internal(f.span, "function declaration without a name"))
    }

    fn declare(
        &mut self,
        name: &Ident,
        kind: BindingKind,
        top_level: bool,
    ) -> Result<(), SyntaxError> {
        self.check_not_reserved(name)?;
        let prior = self.cur().scopes.last().and_then(|s| s.get(&name.name));
        if let Some(prior) = prior {
            // At the top level of a script or function body, JavaScript
            // allows a function declaration to repeat a parameter or another
            // function declaration (they are var-scoped); the subset does not.
            let var_scoped = |k| matches!(k, BindingKind::Param | BindingKind::Function);
            if top_level && kind == BindingKind::Function && var_scoped(prior.kind) {
                return Err(self.error(
                    SyntaxErrorKind::Unsupported,
                    name.span,
                    format!(
                        "redeclaring '{}' with a function declaration is not supported by this JavaScript subset",
                        name.name
                    ),
                ));
            }
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
        let ctx = self.cur();
        if ctx.next_slot >= MAX_LOCALS {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                name.span,
                "more than 65,535 bindings are live at once",
            ));
        }
        let key = name.span.start;
        let binding = Binding {
            kind,
            slot: ctx.next_slot as u16,
            initialized: kind == BindingKind::Param,
            repr: self.facts_of(key).repr(),
            key,
        };
        let ctx = self.cur_mut();
        ctx.next_slot += 1;
        ctx.max_slots = ctx.max_slots.max(ctx.next_slot);
        if let Some(scope) = ctx.scopes.last_mut() {
            scope.insert(name.name.clone(), binding);
        }
        Ok(())
    }

    /// The binding `name` declared in the innermost scope.
    fn local(&self, name: &Ident) -> Result<Binding, SyntaxError> {
        self.cur()
            .scopes
            .last()
            .and_then(|scope| scope.get(&name.name))
            .copied()
            .ok_or_else(|| self.internal(name.span, "declaration was not pre-declared"))
    }

    /// Mark the innermost binding `name` initialized; returns it.
    fn initialize(&mut self, name: &Ident) -> Result<Binding, SyntaxError> {
        let binding = self
            .cur_mut()
            .scopes
            .last_mut()
            .and_then(|scope| scope.get_mut(&name.name));
        match binding {
            Some(b) => {
                b.initialized = true;
                Ok(*b)
            }
            None => Err(self.internal(name.span, "declaration was not pre-declared")),
        }
    }

    fn resolve(&mut self, name: &str) -> Access {
        self.resolve_in(self.fns.len() - 1, name)
    }

    /// Resolve `name` as seen from function `level`, adding captures to it
    /// (and to the functions between it and the declaring one) as needed.
    /// Recursion depth is the function nesting depth, bounded by the tree
    /// height.
    fn resolve_in(&mut self, level: usize, name: &str) -> Access {
        let ctx = &self.fns[level];
        if let Some(b) = ctx.scopes.iter().rev().find_map(|scope| scope.get(name)) {
            return Access::Found {
                place: Place::Local(b.slot),
                target: Target::Binding(b.kind),
                repr: b.repr,
                initialized: b.initialized,
                key: b.key,
            };
        }
        if let Some((self_name, kind)) = &ctx.self_name
            && self_name == name
        {
            let (kind, key) = (*kind, ctx.self_key);
            if let (SelfKind::Declaration { key }, true) = (kind, self.analyzing()) {
                self.fact(key).self_referenced = true;
            }
            return Access::Found {
                place: Place::Current,
                target: Target::SelfFn(kind),
                repr: Repr::Value,
                initialized: true,
                key,
            };
        }
        if level == 0 {
            return if name == RESERVED_BINDING {
                Access::Intrinsic
            } else {
                Access::Unresolvable
            };
        }
        match self.resolve_in(level - 1, name) {
            Access::Found {
                place,
                target,
                repr,
                initialized,
                key,
            } => {
                if self.analyzing() && matches!(place, Place::Local(_)) {
                    // `level` is the direct child of the declaring function;
                    // the declaring function's state is frozen while the
                    // child is lowered, so `initialized` is its state when
                    // the child's closure is created.
                    let facts = self.fact(key);
                    if initialized {
                        facts.captured_initialized = true;
                    } else {
                        facts.captured_uninitialized = true;
                    }
                }
                let ctx = &mut self.fns[level];
                let index = match ctx.capture_index.get(&key) {
                    Some(&index) => index,
                    None => {
                        // Over-long capture lists are reported when the
                        // closure is created.
                        let index = ctx.captures.len().min(MAX_COUNT) as u16;
                        ctx.captures.push(place);
                        ctx.capture_index.insert(key, index);
                        index
                    }
                };
                Access::Found {
                    place: Place::Capture(index),
                    target,
                    repr,
                    initialized: true,
                    key,
                }
            }
            other => other,
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
        self.out().op_u16(Opcode::ThrowError, kind as u16, span);
    }

    fn not_a_value(&self, at: Span) -> SyntaxError {
        self.error(
            SyntaxErrorKind::ReservedWord,
            at,
            format!(
                "the reserved intrinsic binding `{RESERVED_BINDING}` is not a value: it can only be called as `{RESERVED_BINDING}.name(...)`"
            ),
        )
    }

    /// Push the value of a resolved name.
    fn load(&mut self, access: Access, at: Span) -> Result<(), SyntaxError> {
        match access {
            Access::Found {
                initialized: false, ..
            } => self.throw(ThrowKind::Uninitialized, at),
            Access::Found { place, repr, .. } => {
                self.out().load(place, at);
                if repr == Repr::Cell {
                    self.out().op(Opcode::CellGet, at);
                }
            }
            Access::Intrinsic => return Err(self.not_a_value(at)),
            Access::Unresolvable => self.throw(ThrowKind::Unresolvable, at),
        }
        Ok(())
    }

    /// Assign the value on top of the stack to a resolved name, leaving the
    /// value as the expression's result.
    fn store(&mut self, access: Access, at: Span) -> Result<(), SyntaxError> {
        if self.analyzing() {
            match access {
                Access::Found {
                    target: Target::SelfFn(SelfKind::Declaration { key }),
                    ..
                } => self.fact(key).assigned = true,
                Access::Found {
                    target: Target::Binding(_),
                    key,
                    ..
                } => self.fact(key).assigned = true,
                _ => {}
            }
        }
        match access {
            Access::Intrinsic => {
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    at,
                    format!(
                        "the reserved intrinsic binding `{RESERVED_BINDING}` cannot be assigned"
                    ),
                ));
            }
            Access::Unresolvable => {
                self.out().op(Opcode::Pop, at);
                self.throw(ThrowKind::Unresolvable, at);
            }
            Access::Found {
                target: Target::SelfFn(SelfKind::Entry),
                ..
            } => {
                return Err(self.unsupported(at, "assignment to the entry function's own name"));
            }
            Access::Found {
                initialized: false, ..
            } => {
                self.out().op(Opcode::Pop, at);
                self.throw(ThrowKind::Uninitialized, at);
            }
            Access::Found {
                target: Target::Binding(BindingKind::Const) | Target::SelfFn(SelfKind::Expression),
                place,
                repr,
                ..
            } => {
                self.out().op(Opcode::Pop, at);
                if repr == Repr::Cell {
                    // A captured `const` may still be uninitialized, which
                    // is a ReferenceError before the TypeError.
                    self.out().load(place, at);
                    self.out().op(Opcode::CellGet, at);
                    self.out().op(Opcode::Pop, at);
                }
                self.throw(ThrowKind::ConstAssignment, at);
            }
            Access::Found {
                place: Place::Local(slot),
                repr: Repr::Value,
                ..
            } => {
                self.out().op(Opcode::Dup, at);
                self.out().op_u16(Opcode::SetLocal, slot, at);
            }
            Access::Found {
                repr: Repr::Value, ..
            } => {
                // A binding reached through a capture or as the function
                // itself and assigned is always a cell in the emit pass; the
                // analysis pass emits nothing useful here.
                if !self.analyzing() {
                    return Err(self.internal(at, "assigned binding was captured by value"));
                }
            }
            Access::Found {
                place,
                repr: Repr::Cell,
                ..
            } => {
                let tmp = self.temp(at)?;
                self.out().op_u16(Opcode::SetLocal, tmp, at);
                self.out().load(place, at);
                self.out().op_u16(Opcode::GetLocal, tmp, at);
                self.out().op(Opcode::CellSet, at);
                self.out().op_u16(Opcode::GetLocal, tmp, at);
                self.cur_mut().next_slot -= 1;
            }
        }
        Ok(())
    }

    /// A scratch local, live until the caller decrements `next_slot`.
    fn temp(&mut self, at: Span) -> Result<u16, SyntaxError> {
        let ctx = self.cur_mut();
        if ctx.next_slot >= MAX_LOCALS {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                at,
                "more than 65,535 bindings are live at once",
            ));
        }
        let slot = ctx.next_slot as u16;
        ctx.next_slot += 1;
        ctx.max_slots = ctx.max_slots.max(ctx.next_slot);
        Ok(slot)
    }

    // ----- functions -----

    /// Lower `f` as a child function of the running one and emit the code
    /// that creates its closure (pushing one function object).
    fn closure(
        &mut self,
        f: &Function,
        self_name: Option<(String, SelfKind)>,
    ) -> Result<(), SyntaxError> {
        if f.params.len() > MAX_LOCALS {
            return Err(self.error(SyntaxErrorKind::TooLarge, f.span, "too many parameters"));
        }
        self.enter_function(self_name, false);
        self.statement_list(&f.body, &f.params, true)?;
        let end = Span::new(f.span.end.saturating_sub(1), f.span.end);
        self.out().op(Opcode::Undefined, end);
        self.out().op(Opcode::Return, end);
        let ctx = self.fns.pop().expect("the closure's context");
        if ctx.captures.len() > MAX_COUNT {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                f.span,
                "a function captures more than 65,535 bindings",
            ));
        }
        let captures = ctx.captures.clone();
        let child = if self.analyzing() {
            None
        } else {
            let name = f.name.as_ref().map_or("anonymous", |n| n.name.as_str());
            Some(self.seal(ctx, name, f.params.len() as u16)?)
        };
        for place in captures {
            self.out().load(place, f.span);
        }
        let parent = self.cur_mut();
        let index = parent.child_count;
        parent.child_count += 1;
        parent.children.extend(child);
        if index >= MAX_FUNCTIONS {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                f.span,
                "more than 65,536 functions in one function body",
            ));
        }
        self.out().op_u16(Opcode::Function, index as u16, f.span);
        Ok(())
    }

    // ----- statements -----

    /// Reset the script completion value to `undefined` (for `if`/`while`).
    fn reset_completion(&mut self, span: Span) {
        if let Some(slot) = self.cur().completion {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
    }

    fn statement(&mut self, stmt: &Stmt) -> Result<(), SyntaxError> {
        let span = stmt.span;
        match &stmt.kind {
            StmtKind::Expr(e) => {
                self.expr(e)?;
                match self.cur().completion {
                    Some(slot) => self.out().op_u16(Opcode::SetLocal, slot, span),
                    None => self.out().op(Opcode::Pop, span),
                }
            }
            StmtKind::Lexical(decl) => {
                for d in &decl.declarators {
                    let b = self.local(&d.name)?;
                    if b.repr == Repr::Cell {
                        self.out().op_u16(Opcode::GetLocal, b.slot, d.span);
                    }
                    match &d.init {
                        Some(init) => self.expr(init)?,
                        None => self.out().op(Opcode::Undefined, d.span),
                    }
                    let b = self.initialize(&d.name)?;
                    match b.repr {
                        Repr::Cell => self.out().op(Opcode::CellInit, d.span),
                        Repr::Value => self.out().op_u16(Opcode::SetLocal, b.slot, d.span),
                    }
                }
            }
            StmtKind::Return(value) => {
                if self.cur().completion.is_some() {
                    // The parser rejects this; kept as a guard.
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        span,
                        "return outside a function",
                    ));
                }
                match value {
                    Some(e) => self.expr(e)?,
                    None => self.out().op(Opcode::Undefined, span),
                }
                self.out().op(Opcode::Return, span);
            }
            StmtKind::Block(body) => self.statement_list(body, &[], false)?,
            StmtKind::If {
                test,
                consequent,
                alternate,
            } => {
                self.reset_completion(span);
                self.expr(test)?;
                let otherwise = self.out().label();
                self.out().branch(Opcode::JumpIfFalse, otherwise, test.span);
                self.statement(consequent)?;
                match alternate {
                    Some(alternate) => {
                        let end = self.out().label();
                        self.out().branch(Opcode::Jump, end, span);
                        self.out().bind(otherwise);
                        self.statement(alternate)?;
                        self.out().bind(end);
                    }
                    None => self.out().bind(otherwise),
                }
            }
            StmtKind::While { test, body } => {
                self.reset_completion(span);
                let start = self.out().label();
                let end = self.out().label();
                self.out().bind(start);
                self.expr(test)?;
                self.out().branch(Opcode::JumpIfFalse, end, test.span);
                self.statement(body)?;
                self.out().branch(Opcode::Jump, start, span);
                self.out().bind(end);
            }
            StmtKind::Throw(e) => {
                self.expr(e)?;
                self.out().op(Opcode::Throw, span);
            }
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler.as_ref(), finalizer.as_deref(), span)?,
            // Created when its statement list was entered (hoisting).
            StmtKind::FunctionDecl(_) | StmtKind::Empty => {}
        }
        Ok(())
    }

    /// Lower `try`/`catch`/`finally` onto the VM handler stack
    /// (`crate::vm::exception`):
    ///
    /// ```text
    ///     push_finally F        (with a finally block)
    ///     push_catch C          (with a catch clause)
    ///     <block>
    ///     pop_catch
    ///     jump E
    /// C:  set_local param       (the exception; `pop` without a binding)
    ///     <catch body>
    /// E:  pop_finally           (records a normal completion)
    /// F:  <finally body>
    ///     end_finally           (resumes the pending completion)
    /// ```
    ///
    /// A `return` inside reaches the finally block through the VM, which
    /// saves the return value as the pending completion. In a script, the
    /// completion value is that of the block or catch body (`undefined` if
    /// empty), never that of a finally block that completes normally: the
    /// finally block runs with the value saved in a scratch local.
    fn try_statement(
        &mut self,
        block: &[Stmt],
        handler: Option<&CatchClause>,
        finalizer: Option<&[Stmt]>,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.reset_completion(span);
        let finally = finalizer.map(|f| (f, self.out().label()));
        if let Some((_, label)) = finally {
            self.out().branch(Opcode::PushFinally, label, span);
        }
        match handler {
            Some(handler) => {
                let catch = self.out().label();
                let end = self.out().label();
                self.out().branch(Opcode::PushCatch, catch, span);
                self.statement_list(block, &[], false)?;
                self.out().op(Opcode::PopCatch, span);
                self.out().branch(Opcode::Jump, end, span);
                self.out().bind(catch);
                self.reset_completion(handler.span);
                match &handler.param {
                    Some(param) => {
                        // The parameter is the first binding the catch
                        // body's scope declares, so it takes the next slot.
                        let slot = self.cur().next_slot;
                        if slot >= MAX_LOCALS {
                            return Err(self.error(
                                SyntaxErrorKind::TooLarge,
                                param.span,
                                "more than 65,535 bindings are live at once",
                            ));
                        }
                        self.out().op_u16(Opcode::SetLocal, slot as u16, param.span);
                        self.statement_list(&handler.body, std::slice::from_ref(param), false)?;
                    }
                    None => {
                        self.out().op(Opcode::Pop, handler.span);
                        self.statement_list(&handler.body, &[], false)?;
                    }
                }
                self.out().bind(end);
            }
            None => self.statement_list(block, &[], false)?,
        }
        if let Some((body, label)) = finally {
            self.out().op(Opcode::PopFinally, span);
            self.out().bind(label);
            let saved = match self.cur().completion {
                Some(slot) => {
                    let tmp = self.temp(span)?;
                    self.out().op_u16(Opcode::GetLocal, slot, span);
                    self.out().op_u16(Opcode::SetLocal, tmp, span);
                    Some((slot, tmp))
                }
                None => None,
            };
            self.statement_list(body, &[], false)?;
            if let Some((slot, tmp)) = saved {
                self.out().op_u16(Opcode::GetLocal, tmp, span);
                self.out().op_u16(Opcode::SetLocal, slot, span);
                self.cur_mut().next_slot -= 1;
            }
            self.out().op(Opcode::EndFinally, span);
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
                self.out().op(Opcode::Undefined, span);
                return Ok(());
            }
        };
        let index = match self.cur().out.const_index.get(&key) {
            Some(&index) => index,
            None => {
                if self.cur().out.constants.len() >= MAX_CONSTANTS {
                    return Err(self.error(
                        SyntaxErrorKind::TooLarge,
                        span,
                        "more than 65,536 distinct constants",
                    ));
                }
                let out = self.out();
                let index = out.constants.len() as u16;
                out.constants.push(value);
                out.const_index.insert(key, index);
                index
            }
        };
        self.out().op_u16(Opcode::Const, index, span);
        Ok(())
    }

    fn expr(&mut self, e: &Expr) -> Result<(), SyntaxError> {
        let span = e.span;
        match &e.kind {
            ExprKind::Number(n) => self.constant(Primitive::Number(*n), span)?,
            ExprKind::Bool(b) => self.constant(Primitive::Bool(*b), span)?,
            ExprKind::Null => self.constant(Primitive::Null, span)?,
            ExprKind::Undefined => self.out().op(Opcode::Undefined, span),
            ExprKind::String(_) => return Err(self.unsupported(span, "a string value")),
            ExprKind::Identifier(name) => {
                let at = self.name_span(name, span);
                let access = self.resolve(name);
                self.load(access, at)?;
            }
            ExprKind::Unary { op, operand } => {
                self.expr(operand)?;
                match op {
                    UnaryOp::Not => self.out().op(Opcode::Not, span),
                    UnaryOp::Neg | UnaryOp::Plus => {
                        if !is_number(operand) {
                            self.out().op(Opcode::ToNumber, span);
                        }
                        if *op == UnaryOp::Neg {
                            self.out().op(Opcode::Neg, span);
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
                let end = self.out().label();
                self.out().op(Opcode::Dup, span);
                if *op == LogicalOp::Or {
                    self.out().op(Opcode::Not, span);
                }
                self.out().branch(Opcode::JumpIfFalse, end, span);
                self.out().op(Opcode::Pop, span);
                self.expr(right)?;
                self.out().bind(end);
            }
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => {
                self.expr(test)?;
                let otherwise = self.out().label();
                let end = self.out().label();
                self.out().branch(Opcode::JumpIfFalse, otherwise, test.span);
                self.expr(consequent)?;
                self.out().branch(Opcode::Jump, end, span);
                self.out().bind(otherwise);
                self.expr(alternate)?;
                self.out().bind(end);
            }
            ExprKind::Assign { target, value } => {
                self.expr(value)?;
                let at = self.name_span(&target.name, target.span);
                let access = self.resolve(&target.name);
                self.store(access, at)?;
            }
            ExprKind::Call { callee, args } => {
                if let ExprKind::Member { object, property } = &callee.kind {
                    if self.is_intrinsic_binding(object) {
                        return self.intrinsic(property, args, span);
                    }
                    return Err(self.unsupported(callee.span, "property access"));
                }
                if args.len() > MAX_COUNT {
                    return Err(self.error(
                        SyntaxErrorKind::TooLarge,
                        span,
                        "more than 65,535 call arguments",
                    ));
                }
                self.expr(callee)?;
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op_u16(Opcode::Call, args.len() as u16, span);
            }
            ExprKind::Member { object, property } => {
                if self.is_intrinsic_binding(object) {
                    return Err(self.error(
                        SyntaxErrorKind::ReservedWord,
                        span,
                        format!(
                            "the intrinsic `{RESERVED_BINDING}.{}` must be called directly: intrinsics are not values",
                            property.name
                        ),
                    ));
                }
                return Err(self.unsupported(span, "property access"));
            }
            ExprKind::Function(f) => {
                let self_name = match &f.name {
                    Some(name) => {
                        self.check_not_reserved(name)?;
                        Some((name.name.clone(), SelfKind::Expression))
                    }
                    None => None,
                };
                self.closure(f, self_name)?;
            }
        }
        Ok(())
    }

    /// True when `object` is an identifier that resolves to the reserved
    /// intrinsic binding (it cannot be shadowed, so this is resolution, not
    /// spelling).
    fn is_intrinsic_binding(&mut self, object: &Expr) -> bool {
        match &object.kind {
            ExprKind::Identifier(name) if name == RESERVED_BINDING => {
                matches!(self.resolve(name), Access::Intrinsic)
            }
            _ => false,
        }
    }

    /// Lower `__rt.<property>(args)` through the intrinsic table.
    fn intrinsic(
        &mut self,
        property: &Ident,
        args: &[Expr],
        span: Span,
    ) -> Result<(), SyntaxError> {
        let name = &property.name;
        let Some(intrinsic) = intrinsics::lookup(name) else {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                property.span,
                format!("`{RESERVED_BINDING}.{name}` is not an intrinsic"),
            ));
        };
        let n = args.len();
        let bad_arity = match intrinsic.arity {
            Arity::Exactly(k) if n != k => Some(format!(
                "`{RESERVED_BINDING}.{name}` takes {k} argument{}, got {n}",
                if k == 1 { "" } else { "s" }
            )),
            Arity::Pairs if !n.is_multiple_of(2) => Some(format!(
                "`{RESERVED_BINDING}.{name}` takes key/value pairs (an even number of arguments), got {n}"
            )),
            _ => None,
        };
        if let Some(message) = bad_arity {
            return Err(self.error(SyntaxErrorKind::Invalid, span, message));
        }
        if n > MAX_COUNT {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "more than 65,535 intrinsic arguments",
            ));
        }
        match intrinsic.lowering {
            Lowering::Op(op) => {
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op(op, span);
            }
            Lowering::Counted(op) => {
                for arg in args {
                    self.expr(arg)?;
                }
                let count = if intrinsic.arity == Arity::Pairs {
                    n / 2
                } else {
                    n
                };
                self.out().op_u16(op, count as u16, span);
            }
            Lowering::HostEffect => {
                let selector = match args[0].kind {
                    ExprKind::Number(op)
                        if op.fract() == 0.0 && (0.0..=f64::from(u16::MAX)).contains(&op) =>
                    {
                        op as u16
                    }
                    _ => {
                        return Err(self.error(
                            SyntaxErrorKind::Invalid,
                            args[0].span,
                            format!(
                                "the host operation of `{RESERVED_BINDING}.{name}` must be an integer literal from 0 to 65535"
                            ),
                        ));
                    }
                };
                self.expr(&args[1])?;
                self.out().op_u16(Opcode::HostEffect, selector, span);
            }
            Lowering::Need => {
                // The run-time check of `need` (`crate::vm::dispatch`), applied
                // early to literals.
                let limits = [
                    ("definition id", 9_007_199_254_740_991.0),
                    ("result kind", f64::from(u32::MAX)),
                ];
                for (arg, (what, max)) in args.iter().zip(limits) {
                    if let ExprKind::Number(n) = arg.kind
                        && !(n.fract() == 0.0 && (0.0..=max).contains(&n))
                    {
                        return Err(self.error(
                            SyntaxErrorKind::Invalid,
                            arg.span,
                            format!(
                                "the {what} of `{RESERVED_BINDING}.{name}` must be an integer from 0 to {max}"
                            ),
                        ));
                    }
                }
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op(Opcode::Need, span);
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
                self.out().op(compare, span);
                if negate {
                    self.out().op(Opcode::Not, span);
                }
                return;
            }
        };
        // ToNumber(left) must happen after the right operand is evaluated,
        // so a non-number left operand converts both together.
        if !is_number(left) {
            self.out().op(Opcode::ToNumberPair, span);
        } else if !is_number(right) {
            self.out().op(Opcode::ToNumber, span);
        }
        self.out().op(numeric, span);
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
