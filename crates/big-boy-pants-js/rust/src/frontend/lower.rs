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
//! arrow's concise body is already a `return` statement in the tree.
//! Arrows capture `arguments` lexically; ordinary functions install an
//! implicit binding only when referenced, using the analysis pass's facts.
//! Their `this` is captured by value from the nearest ordinary function
//! through intervening arrows (clj-quickjs-98). `new.target` uses the same capture path (clj-quickjs-99);
//! `super` captures its method context through the same path (clj-quickjs-112).
//!
//! **Bindings.** Every `let`/`const`/function binding and every parameter
//! lives in a frame local. Entering a statement list (script, function body,
//! block) first declares all of its bindings, so a reference anywhere in the
//! list resolves to them; a redeclared name is a JavaScript early error,
//! [`SyntaxErrorKind::Invalid`]. Sibling blocks reuse local slots.
//! A script or function body also declares the `var` bindings of its whole
//! body (clj-quickjs-94; [`var_names`]), initialized to `undefined` at
//! entry, so they have no temporal dead zone. There, parameters, `var`s and
//! function declarations are var-scoped: repeating one of them reuses its
//! binding (`Lowerer::declare`), and a `var` statement's initializer is an
//! assignment. A `var` that a lexical binding of an enclosing block would
//! shadow is an early error (`Lowerer::check_var_not_shadowed`). Names are
//! resolved through the stack of enclosing functions, as QuickJS's
//! `get_closure_var` does: a binding of an enclosing function becomes a
//! capture of every function in between (`ExecutableSpec::captures`), passed
//! down by the parent when it creates the closure (`get_local`,
//! `get_capture` or `current_function`, then `function i`).
//!
//! **Default parameter values** (clj-quickjs-100). A function with a
//! default has a scope of its own for its parameters, outside the body's
//! (`Lowerer::parameter_scope`): the parameters are initialized there left
//! to right, each in its temporal dead zone until its turn, so the static
//! decision above applies to them, and a default runs only when its
//! argument is `undefined`. The body's declarations are a second scope
//! that the defaults cannot see; a body `var` repeating a parameter is a
//! new binding that starts with the parameter's value
//! (`Lowerer::init_body_var`), and a body `let`/`const` repeating one is an
//! early error (`Lowerer::check_not_parameter`).
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
//! Syntactic direct eval exposes all visible bindings to runtime source:
//! analysis forces those bindings into cells and emission records their
//! locations at each call. The ordinary capture resolver carries outer cells
//! through intervening functions; eval imports them into its own executable.
//!
//! **Temporal dead zone.** Within the function that declares a binding,
//! whether it is initialized at a reference is still decided by position:
//! code of one function runs in source order except for jumps that never
//! enter a scope past a declaration. An
//! exception or a `return` may leave a `try` block early for its catch or
//! finally block, which follow the whole `try` block, where every binding
//! declared inside it is out of scope. An unlabelled `break` only leaves
//! scopes (to the end of its loop), and `continue` jumps to the loop test,
//! before the loop body, which is entered again from its start: neither
//! skips a declaration of a scope it lands in, and the bindings of the
//! enclosing statement list are in the same state at the test on every
//! iteration (a loop body is a single statement, so it declares nothing
//! there). A `for` head's bindings are all initialized before its first
//! test, and its test, body, update and `continue` target all lie after
//! them in its scope (`Lowerer::for_statement`). A `for`-`in` head's binding
//! (clj-quickjs-85) is in its temporal dead zone in the object expression
//! and initialized at the start of every iteration, before the body;
//! `continue` lands before that initialization
//! (`Lowerer::for_in_statement`). A labelled `break` only
//! leaves scopes, to the end of its labelled statement (never a
//! declaration), and a labelled `continue` lands where an unlabelled one
//! in its loop would (clj-quickjs-82). So a reference before the
//! declaration (or inside its own
//! initializer) still always runs before initialization, and one after it
//! always runs after. Such references, assignments to `const` bindings and
//! references to lexical bindings in their TDZ lower to `throw_error`.
//! Otherwise free names and script `var`/function bindings use run-time
//! global-property lookup (clj-quickjs-104); `typeof` permits a missing global.
//! Only closures can
//! run a reference at another time, and they use the run-time check above.
//! The exception is a `switch` (clj-quickjs-83), which enters its case
//! block at the selected clause, past the declarations of the clauses
//! before it: those `let`/`const` bindings are cells, captured or not, so
//! the references after them check at run time
//! (`Lowerer::switch_statement`).
//!
//! **Reserved intrinsic binding.** `__rt` resolves to the outermost scope;
//! see [`super::intrinsics`] for the contract and the table.
//!
//! **Numbers.** `- * / %`, the bitwise and shift operators
//! `& | ^ << >> >>>`, `**` and unary `-`, `+`, `~` apply JavaScript ToNumber to
//! operands that are not statically numbers (`ToNumber`/`ToNumberPair`)
//! before the number-only arithmetic instruction (which applies ToInt32 /
//! ToUint32 itself for the bitwise ones). `to_number` converts a string
//! operand with StringToNumber (clj-quickjs-67). Binary `+` (clj-quickjs-65)
//! lowers to the number-only `add` only when both operands are statically
//! numbers (`is_number`), and otherwise to `plus`, JavaScript `+` on
//! primitives, which concatenates two strings at run time and applies no
//! ToNumber first. The relational operators (clj-quickjs-68) lower to
//! `compare_lt`/`compare_gt`/`compare_le`/`compare_ge` unless both operands
//! are statically Numbers: those compare BigInt/BigInt and BigInt/Number
//! exactly (clj-quickjs-197), compare strings by UTF-16 code units, and
//! convert other supported primitives with ToNumber. BigInt/String pairs
//! fail explicitly pending StringToBigInt.
//!
//! **Strings.** A string literal (UTF-16 code units, possibly with lone
//! surrogates, decoded by the lexer) is a constant, deduplicated by
//! content like other constants.
//! `++`/`--` on a binding (clj-quickjs-64) is a read, `to_number`, `add` or
//! `sub` of the constant 1 and a store, with the checks of an ordinary read
//! and assignment (so a captured binding goes through its cell and a
//! `const` throws after the read); a postfix update `dup`s the converted
//! old value first and drops the stored one.
//!
//! **Completion value.** A script keeps its completion value in local 0 (see
//! [`super::compile_script`]); a function body discards expression values.
//!
//! **Exceptions.** `throw` lowers to `throw`, and `try` statements to the
//! VM handler instructions (`Lowerer::try_statement`); a `return` needs no
//! special lowering, since the VM runs the frame's finally blocks itself.
//! A catch parameter is a binding of the catch body's scope, initialized
//! from the exception value.
//!
//! **`break` and `continue`** (clj-quickjs-78) jump to the end
//! or the `continue` target (the test, a `for` loop's update, or a
//! `for`-`in` loop's next key) of the
//! innermost loop of the running function (an unlabelled `break` to the end
//! of an inner `switch`, clj-quickjs-83), or with a label (clj-quickjs-82)
//! to the end of the innermost statement with that label or the `continue`
//! target of the innermost loop with that label in its label set
//! (`Lowerer::jump`, `Lowerer::labelled`), first popping the catch handler of every `try` block
//! they leave, running the finally block of every `try` block or catch body
//! they leave and discarding the pending completion of every finally block
//! they leave (clj-quickjs-79). The script completion value needs nothing
//! extra: local 0 already holds the value the loop body produced so far
//! (ECMAScript `UpdateEmpty`), and a finally block that a jump runs restores
//! it on completing normally, as for any other completion.

mod modules;
pub(crate) use modules::{ModuleBinding, lower_module, module_bindings};

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use super::ast::{
    BinaryOp, CatchClause, ChainLink, ChainLinkKind, Class, ClassElement, Declarator, Expr,
    ExprKind, ForInHead, ForInit, Function, FunctionKind, Ident, LexicalDecl, LexicalKind,
    LogicalOp, Pattern, PatternKind, PropertyDef, PropertyName, Script, Stmt, StmtKind, SwitchCase,
    TemplateElement, UnaryOp, UpdateOp,
};
use super::intrinsics::{self, Arity, Lowering, RESERVED_BINDING};
use super::lexer::{Lexer, TokenKind};
use super::{Span, SyntaxError, SyntaxErrorKind, line_columns};
use crate::error::SourceLocation;
use crate::value::string::number_to_string;
use crate::value::{JsString, Primitive};
use crate::vm::executable::{
    ConstructorKind, EvalBinding, EvalBindingKind, EvalSite, EvalSource, Executable, ExecutableSpec,
};
use crate::vm::instr::{Opcode, ThrowKind};

/// Class initialization order, including blocks with independent var scopes.
enum ClassInitializer<'a> {
    Member(&'a PropertyDef, Ident, bool),
    StaticBlock(&'a Function),
}

/// Locals are counted by a u16.
const MAX_LOCALS: usize = u16::MAX as usize;
/// Constants are indexed by a u16.
const MAX_CONSTANTS: usize = 1 << 16;
/// Child functions are indexed by a u16.
const MAX_FUNCTIONS: usize = 1 << 16;
/// Call arguments and captures are counted by a u16.
const MAX_COUNT: usize = u16::MAX as usize;

/// The VM the bytecode is lowered for. The Pants engine's own entry points
/// lower for [`Backend::Pants`] and keep their bytecode; the Goose exporter
/// lowers for [`Backend::Goose`], which also gives a sloppy function with a
/// simple parameter list a mapped arguments object when it can observe
/// one (light-cljs-860; [`Lowerer::wants_mapped_arguments`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Backend {
    #[default]
    Pants,
    Goose,
}

pub(crate) fn lower_script(
    name: &str,
    source: &str,
    script: &Script,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    lower_script_kind(name, source, script, false, None, backend)
}

pub(crate) fn lower_eval(
    name: &str,
    source: &str,
    script: &Script,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    lower_script_kind(name, source, script, true, None, backend)
}

pub(crate) fn lower_direct_eval(
    name: &str,
    source: &str,
    script: &Script,
    site: &EvalSite,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    lower_script_kind(name, source, script, true, Some(site), backend)
}

fn lower_script_kind(
    name: &str,
    source: &str,
    script: &Script,
    eval: bool,
    site: Option<&EvalSite>,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    let lower = |facts| {
        let mut lower = Lowerer::new(source, facts);
        lower.backend = backend;
        if let Some(site) = site {
            lower.import_eval_bindings(site);
            if !script.strict {
                let mut vars = Vec::new();
                var_names(&script.body, &mut vars);
                for stmt in &script.body {
                    if let StmtKind::FunctionDecl(f) = &stmt.kind {
                        vars.push(lower.declaration_name(f)?);
                    }
                }
                for name in vars {
                    if site.lexical_conflicts.contains(&name.name) {
                        return Err(lower.error(
                            SyntaxErrorKind::Invalid,
                            name.span,
                            format!(
                                "eval variable '{}' conflicts with a lexical binding",
                                name.name
                            ),
                        ));
                    }
                }
                lower.eval_variable_bindings = site.variable_bindings.clone();
                lower.eval_variable_environment = site.variable_environment.and_then(|s| {
                    site.environments
                        .iter()
                        .position(|e| *e == s)
                        .map(|i| lower.eval_environments[i].0)
                });
                lower.sloppy_eval = true;
            }
        }
        lower.enter_function(None, true, script.strict);
        if let Some(site) = site {
            lower.cur_mut().captures = vec![
                Place::This;
                site.bindings.len()
                    + 1
                    + usize::from(site.new_target.is_some())
                    + site.environments.len()
                    + usize::from(site.super_home.is_some())
                    + usize::from(site.super_fields.is_some())
            ];
            lower.cur_mut().this_strict = site.receiver_strict;
            lower.cur_mut().class_initializer = site.class_initializer;
            lower.cur_mut().super_property = site.super_home.is_some();
            lower.cur_mut().super_call = site.super_fields.is_some();
            lower.cur_mut().instance_fields = lower.eval_super_fields;
        }
        lower.cur_mut().global_declarations =
            !eval || (!script.strict && lower.eval_variable_environment.is_none());
        lower.cur_mut().script_lexicals = !eval;
        lower.statement_list(&script.body, &[], true)?;
        let end = Span::new(script.span.end, script.span.end);
        lower.out().op_u16(Opcode::GetLocal, 0, end);
        lower.out().op(Opcode::Return, end);
        Ok::<_, SyntaxError>(lower)
    };
    let facts = lower(None)?.facts;
    let mut lower = lower(Some(facts))?;
    lower.finish_top(name, 0, 0, JsString::from(""))
}

pub(crate) fn lower_function(
    source: &str,
    function: &Function,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    lower_function_kind(source, function, true, backend)
}

pub(crate) fn lower_dynamic_function(
    source: &str,
    function: &Function,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    lower_function_kind(source, function, false, backend)
}

fn lower_function_kind(
    source: &str,
    function: &Function,
    bind_name: bool,
    backend: Backend,
) -> Result<Executable, SyntaxError> {
    let lower = |facts| {
        let mut lower = Lowerer::new(source, facts);
        lower.backend = backend;
        if function.params.len() > MAX_LOCALS {
            return Err(lower.error(
                SyntaxErrorKind::TooLarge,
                function.span,
                "too many parameters",
            ));
        }
        let self_name = match function.name.as_ref().filter(|_| bind_name) {
            Some(name) => {
                lower.check_not_reserved(name)?;
                Some((name.name.clone(), SelfKind::Entry))
            }
            None => None,
        };
        lower.enter_function(self_name, false, function.strict);
        lower.cur_mut().own_arguments = true;
        lower.cur_mut().constructor_kind = constructor_kind(function);
        lower.cur_mut().text = Some(function.span);
        lower.cur_mut().simple_parameters = !function.has_non_simple_parameters();
        lower.function_body(function)?;
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
    lower.finish_top(
        name,
        function.params.len() as u16,
        function_length(function),
        JsString::from(function.name.as_ref().map_or("", |n| n.name.as_str())),
    )
}

fn constructor_kind(f: &Function) -> ConstructorKind {
    if f.kind == FunctionKind::DerivedClassConstructor {
        ConstructorKind::DerivedClass
    } else if f.kind == FunctionKind::DefaultDerivedConstructor {
        ConstructorKind::DefaultDerivedClass
    } else if f.kind == FunctionKind::ClassConstructor {
        ConstructorKind::BaseClass
    } else if f.kind == FunctionKind::Ordinary && !f.generator && !f.asynchronous {
        ConstructorKind::Ordinary
    } else {
        ConstructorKind::None
    }
}

fn function_length(f: &Function) -> u16 {
    f.params
        .iter()
        .zip(&f.defaults)
        .take_while(|(p, d)| d.is_none() && !matches!(p.kind, PatternKind::Rest(_)))
        .count() as u16
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BindingKind {
    Import,
    Let,
    Const,
    Param,
    /// A function declaration's name.
    Function,
    /// A `var` binding (clj-quickjs-94) that is not also a parameter or a
    /// function declaration's name.
    Var,
}

/// NamedEvaluation supplies a literal name or an already converted key.
enum ClassName {
    Known(JsString),
    Computed(u16),
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
    /// A script object binding, resolved dynamically rather than captured.
    global: bool,
    /// A top-level script lexical, also retained in the task environment.
    global_lexical: bool,
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
    /// This activation contains a potentially sloppy direct eval.
    has_eval: bool,
    /// A potentially sloppy direct eval runs in this activation's defaults.
    has_parameter_eval: bool,
    /// Direct eval can read, assign and retain this binding dynamically.
    eval_visible: bool,
    /// A reference resolves to this binding (including through an arrow).
    referenced: bool,
    /// Body var initialized from this parameter-scope binding. Its reads
    /// require the source even if defaults never reference it directly.
    copied_to: Option<usize>,
    /// Some assignment targets it.
    assigned: bool,
    /// Some assignment in a nested function (through a capture) targets it.
    assigned_in_closure: bool,
    /// A closure capturing it is created while it is initialized.
    captured_initialized: bool,
    /// A closure capturing it may be created before it is initialized.
    captured_uninitialized: bool,
    /// (Function declarations) its own body refers to it.
    self_referenced: bool,
    /// (A function's self key) a reference resolves to its own `arguments`
    /// binding, the implicit one or a `var` of that name (light-cljs-860).
    arguments_used: bool,
    /// (A function's self key) some such reference is not the base of a
    /// property read (`arguments.length`, `arguments[i]`): the object may
    /// be written, deleted from or reach other code.
    arguments_escape: bool,
}

impl Facts {
    fn repr(self) -> Repr {
        let captured = self.captured_initialized || self.self_referenced;
        if self.eval_visible || self.captured_uninitialized || (self.assigned && captured) {
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
    GlobalObject,
    Local(u16),
    Capture(u16),
    /// `current_function`.
    Current,
    /// The nearest ordinary function's immutable frame receiver.
    This,
    /// Transport an unevaluated receiver or derived receiver cell.
    RawThis,
    NewTarget,
}

fn eval_source(place: Place) -> EvalSource {
    match place {
        Place::GlobalObject => EvalSource::GlobalObject,
        Place::Local(i) => EvalSource::Local(i),
        Place::Capture(i) => EvalSource::Capture(i),
        Place::Current => EvalSource::CurrentFunction,
        Place::This | Place::RawThis => EvalSource::This,
        Place::NewTarget => EvalSource::NewTarget,
    }
}

#[derive(Clone, Copy, Debug)]
enum Access {
    /// Index in Lowerer::dynamic_accesses; retained as a small Copy handle.
    Dynamic(usize),
    /// A dynamic reference selected before evaluating an assignment RHS or
    /// numeric conversion. The slot retains the selected environment, not
    /// its current cell; fallback indexes the name and static fallback.
    Prepared { slot: u16, fallback: usize },
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
    /// A name in `Lowerer::global_names`, looked up in the task's global
    /// object at run time. Also used for script `var`/function bindings.
    Global(usize),
}

/// A property reference on the operand stack
/// (`Lowerer::property_reference`, clj-quickjs-76), with the `.` or `[`
/// token its read and write are reported at.
#[derive(Clone, Copy, Debug)]
enum PropertyRef {
    /// `o.p`: `o` on the stack; the name's constant-pool index.
    Field(u16, Span),
    /// `o[k]`: `o` and the converted key on the stack.
    Elem(Span),
    /// Receiver and a lexically resolved, unobservable private identity.
    Private(Span),
    /// `super[k]`: current receiver, lookup base and raw/converted key.
    Super(Span),
}

/// Constant-pool deduplication key. Numbers compare by bit pattern, so `0`
/// and `-0` (and distinct NaN payloads) stay distinct constants.
#[derive(PartialEq, Eq, Hash)]
enum ConstKey {
    Null,
    Bool(bool),
    Number(u64),
    /// Strings by content (code units).
    String(JsString),
    BigInt(crate::value::JsBigInt),
    Symbol(crate::value::JsSymbol),
}

/// Internal bytecode emitter: byte code, one span per instruction, a
/// deduplicated constant pool, and labels. Nothing outside this module can
/// construct code through it; its output goes through the validator like any
/// other code.
#[derive(Default)]
struct Emitter {
    code: Vec<u8>,
    spans: Vec<Span>,
    /// Source labels for `new` errors, keyed by instruction index.
    construct_callees: HashMap<usize, Span>,
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
            Place::GlobalObject => self.op(Opcode::GetGlobalObject, span),
            Place::Local(slot) => self.op_u16(Opcode::GetLocal, slot, span),
            Place::Capture(i) => self.op_u16(Opcode::GetCapture, i, span),
            Place::Current => self.op(Opcode::CurrentFunction, span),
            Place::This => self.op(Opcode::GetThis, span),
            Place::RawThis => self.op(Opcode::GetRawThis, span),
            Place::NewTarget => self.op(Opcode::GetNewTarget, span),
        }
    }
}

/// One function being lowered.
struct FnCtx {
    async_generator: bool,
    /// Object environments interleaved with lexical statement-list scopes.
    /// Depth is the number of scopes outside the with body. Objects are
    /// captured directly; variable environments remain private binding cells.
    with_envs: Vec<(usize, Place, usize)>,
    dynamic_env: Option<(Place, usize)>,
    environment_key: usize,
    parameter_env: Option<(Place, usize)>,
    parameter_environment_key: usize,
    /// Internal module entry points have no JavaScript new.target binding.
    module: bool,
    eval_sites: Vec<EvalSite>,
    /// The function's source text span ([`Sealed::text`]); set by
    /// [`Lowerer::closure_with_fields`] for a function written in the source.
    text: Option<Span>,
    /// The parameter list is simple (no default, rest or pattern), set with
    /// `text`: sloppy code with one gets a mapped arguments object, whose
    /// `callee` is the function (light-cljs-854; the Goose exporter reads it).
    simple_parameters: bool,
    in_parameters: bool,
    /// Only the original script registers lexicals; eval and functions use
    /// ordinary scope-local bindings for their own lexical declarations.
    script_lexicals: bool,
    /// Script declarations use global properties; strict eval declarations
    /// use locals while still producing a script completion value.
    global_declarations: bool,
    out: Emitter,
    /// Innermost last.
    scopes: Vec<Scope>,
    next_slot: usize,
    max_slots: usize,
    /// The completion-value local of a script; `None` in a function.
    completion: Option<u16>,
    /// The function (or script) is strict-mode code (clj-quickjs-95;
    /// [`Function::strict`], [`Script::strict`]), recorded in its code as
    /// `ExecutableSpec::strict`. Lowering that differs between strict and
    /// sloppy code consults `self.cur().strict`: the mode of the innermost
    /// function, an arrow's own (inherited unless its body has a directive).
    strict: bool,
    /// Strict expressions inside a sloppy function need strict write/delete
    /// bytecodes; their surrounding code and activation remain sloppy.
    strict_expression: bool,
    /// An ordinary function or concise method (not an arrow or a script):
    /// in JavaScript it has its own `arguments` object, which a reference
    /// to `arguments` denotes unless the function binds that name itself
    /// ([`Lowerer::arguments_binding`]).
    own_arguments: bool,
    class_initializer: bool,
    /// Ordinary functions and base classes can be constructed.
    constructor_kind: ConstructorKind,
    /// Resolved when the constructor closure is entered, before any nested
    /// class scopes can shadow the inaccessible initializer name.
    instance_fields: Option<Access>,
    /// Arrows inherit their nearest ordinary function's receiver.
    lexical_this: bool,
    /// Capture index for that receiver, shared by all its uses and any
    /// nested arrows that need it. No cell: a frame's receiver is immutable.
    this_capture: Option<u16>,
    raw_this_capture: Option<u16>,
    new_target_capture: Option<u16>,
    super_capture: Option<u16>,
    super_property: bool,
    super_call: bool,
    /// Strictness of the ordinary function supplying lexical `this`, which
    /// can differ from an arrow/eval's own directive-selected strictness.
    this_strict: bool,

    /// Private identity for the implicit arguments binding, separate from
    /// source offsets and the function's self-binding.
    arguments_key: usize,
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
    /// Constructs `break`/`continue` may leave, innermost last.
    exits: Vec<Exit>,
    /// Declaration start, value slot, method slot, optional async hint slot.
    using_resources: Vec<(usize, u16, u16, Option<u16>)>,
    /// The label set of the loop about to be lowered (`L1: L2: while ...`),
    /// taken by [`Lowerer::push_loop`].
    loop_labels: Vec<String>,
    /// The index in `scopes` of the function body's scope: 1 when the
    /// parameters have their own scope (some parameter has a default,
    /// [`Lowerer::parameter_scope`]), otherwise 0.
    var_scope: usize,
    /// The block function declarations (by [`Function::span`] start) that
    /// also assign a `var` binding of the body when evaluated (ECMAScript
    /// Annex B.3.3; [`Lowerer::enter_scope`], [`Lowerer::annex_b_copy`]).
    annex_b: HashSet<usize>,
    /// The flag local of each new global Annex B binding, true once it was
    /// created ([`Lowerer::annex_b_global`]).
    annex_b_globals: HashMap<String, u16>,
}

/// A construct of the running function that a `break` or `continue` inside
/// it may leave (QuickJS keeps the same chain as `BlockEnv`).
enum Exit {
    /// A loop body: `break` jumps to `brk`, `continue` to `cont`; `labels`
    /// is the loop's label set (clj-quickjs-82), possibly empty.
    Loop {
        brk: usize,
        cont: usize,
        labels: Vec<String>,
    },
    /// A for-in/of loop owns a finally handler. Only its own continue keeps it.
    IteratorLoop {
        brk: usize,
        cont: usize,
        labels: Vec<String>,
    },
    /// A labelled statement that is not a loop (clj-quickjs-82): `break`
    /// to one of its labels jumps to `brk`, past it. An unlabelled `break`
    /// passes it by.
    Labelled { labels: Vec<String>, brk: usize },
    /// The case block of a `switch` (clj-quickjs-83): an unlabelled `break`
    /// jumps to `brk`, past it; `continue` and labelled jumps pass it by.
    Switch { brk: usize },
    /// The block of a `try` statement with a catch clause: its catch handler
    /// is on the handler stack, so leaving pops it.
    Catch,
    /// The block or catch body of a `try` statement with a finally block:
    /// its finally handler is on the handler stack, so leaving runs the
    /// finally block first (`call_finally`).
    Finally,
    /// The finally block of a `try` statement: its pending completion is on
    /// the handler stack, so leaving discards it (`drop_completion`).
    FinallyBody,
}

/// A lowered function waiting to be loaded ([`Lowerer::load_all`]).
struct Sealed {
    eval_sites: Vec<EvalSite>,
    name: String,
    params: u16,
    function_length: u16,
    function_name: JsString,
    locals: u16,
    captures: u16,
    strict: bool,
    /// `ExecutableSpec::constructor_kind`: a `function` declaration or expression.
    constructor_kind: ConstructorKind,
    code: Vec<u8>,
    spans: Vec<Span>,
    construct_callees: HashMap<usize, Span>,
    constants: Vec<Primitive>,
    children: Vec<usize>,
    /// The function's source text (ECMAScript [[SourceText]], QuickJS's
    /// `debug.source`), compiled-function metadata the Goose exporter reads
    /// for `Function.prototype.toString` (light-cljs-853); `None` for a
    /// script, eval code and synthesized functions (class field
    /// initializers).
    text: Option<Span>,
    /// IsSimpleParameterList of the function's parameters (light-cljs-854;
    /// [`FnCtx::simple_parameters`]).
    simple_parameters: bool,
}

struct Lowerer<'s> {
    prepared_accesses: Vec<(String, Access)>,
    dynamic_accesses: Vec<(Place, usize, String, Access)>,
    eval_environments: Vec<(Place, usize)>,
    eval_variable_environment: Option<Place>,
    eval_variable_bindings: Vec<String>,
    eval_lexical_conflicts: Vec<String>,
    sloppy_eval: bool,
    /// The class whose constructor [`Self::closure_with_fields`] lowers
    /// next: a class constructor's source text is the whole class.
    class_text: Option<Span>,
    eval_bindings: HashMap<String, Access>,
    eval_this: Option<u16>,
    eval_new_target: Option<u16>,
    eval_super_home: Option<u16>,
    eval_super_fields: Option<Access>,
    /// Names indexed by `Access::Global`, keeping the recursive resolver's
    /// return type small and Copy. Constant indices belong to each emitter.
    global_names: Vec<String>,
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
    /// The VM the bytecode is for ([`Backend`]).
    backend: Backend,
    /// The identifier being resolved is the base of a property read
    /// ([`Self::get_field`]), which does not let an `arguments` object
    /// escape ([`Facts::arguments_escape`]).
    member_base: bool,
}

impl<'s> Lowerer<'s> {
    fn new(source: &'s str, facts: Option<HashMap<usize, Facts>>) -> Lowerer<'s> {
        Lowerer {
            prepared_accesses: Vec::new(),
            dynamic_accesses: Vec::new(),
            eval_environments: Vec::new(),
            eval_variable_environment: None,
            eval_variable_bindings: Vec::new(),
            eval_lexical_conflicts: Vec::new(),
            sloppy_eval: false,
            class_text: None,
            eval_bindings: HashMap::new(),
            eval_this: None,
            eval_new_target: None,
            eval_super_home: None,
            eval_super_fields: None,
            global_names: Vec::new(),
            source,
            shared_source: Arc::from(source),
            fns: Vec::new(),
            sealed: Vec::new(),
            emit: facts,
            facts: HashMap::new(),
            next_self_key: usize::MAX,
            backend: Backend::Pants,
            member_base: false,
        }
    }

    fn analyzing(&self) -> bool {
        self.emit.is_none()
    }

    /// The instruction that creates the cell of binding `b` at its scope's
    /// entry (light-cljs-646; `crate::vm::function`, "Fixed and sealed
    /// bindings"): a `let`, `const`, parameter or function declaration that
    /// nothing assigns is fixed once initialized; a binding assigned only
    /// by its own function's code may be sealed; any other is mutable.
    fn new_cell_opcode(&self, b: Binding) -> Opcode {
        let facts = self.facts_of(b.key);
        if self.analyzing() || facts.eval_visible || facts.assigned_in_closure || self.sloppy_eval {
            Opcode::NewCell
        } else if !facts.assigned
            && matches!(
                b.kind,
                BindingKind::Let | BindingKind::Const | BindingKind::Param | BindingKind::Function
            )
        {
            Opcode::NewFixedCell
        } else if matches!(b.kind, BindingKind::Import) {
            Opcode::NewCell
        } else {
            Opcode::NewSealableCell
        }
    }

    fn import_eval_bindings(&mut self, site: &EvalSite) {
        self.eval_lexical_conflicts = site.lexical_conflicts.clone();
        let bindings = &site.bindings;
        self.eval_new_target = site.new_target.map(|_| (bindings.len() + 1) as u16);
        self.eval_super_home = site.super_home.map(|_| {
            (bindings.len() + 1 + usize::from(site.new_target.is_some()) + site.environments.len())
                as u16
        });
        if let Some((_, cell)) = site.super_fields {
            let key = self.next_self_key;
            self.next_self_key -= 1;
            self.eval_super_fields = Some(Access::Found {
                place: Place::Capture(
                    (bindings.len()
                        + 1
                        + usize::from(site.new_target.is_some())
                        + site.environments.len()
                        + usize::from(site.super_home.is_some())) as u16,
                ),
                target: Target::Binding(BindingKind::Const),
                repr: if cell { Repr::Cell } else { Repr::Value },
                initialized: true,
                key,
            });
        }
        for i in 0..site.environments.len() {
            let key = self.next_self_key;
            self.next_self_key -= 1;
            self.eval_environments.push((
                Place::Capture(
                    (bindings.len() + 1 + usize::from(site.new_target.is_some()) + i) as u16,
                ),
                key,
            ));
        }
        for (i, b) in bindings.iter().enumerate() {
            let key = self.next_self_key;
            self.next_self_key -= 1;
            let mut access = Access::Found {
                place: Place::Capture(i as u16),
                target: match b.kind {
                    EvalBindingKind::Import => Target::Binding(BindingKind::Import),
                    EvalBindingKind::Mutable => Target::Binding(BindingKind::Let),
                    EvalBindingKind::Const => Target::Binding(BindingKind::Const),
                    EvalBindingKind::FunctionName => Target::SelfFn(SelfKind::Expression),
                },
                repr: if b.cell { Repr::Cell } else { Repr::Value },
                initialized: true,
                key,
            };
            for &(env, key) in self.eval_environments[..usize::from(b.environments)]
                .to_vec()
                .iter()
                .rev()
            {
                access = self.dynamic_access(env, key, &b.name, access);
            }
            self.eval_bindings.insert(b.name.clone(), access);
        }
        self.eval_this = Some(bindings.len() as u16);
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

    fn enter_function(
        &mut self,
        self_name: Option<(String, SelfKind)>,
        script: bool,
        strict: bool,
    ) {
        let slots = usize::from(script);
        let self_key = self.next_self_key;
        self.next_self_key -= 1;
        let arguments_key = self.next_self_key;
        self.next_self_key -= 1;
        let environment_key = self.next_self_key;
        self.next_self_key -= 1;
        let parameter_environment_key = self.next_self_key;
        self.next_self_key -= 1;
        self.fns.push(FnCtx {
            with_envs: Vec::new(),
            dynamic_env: None,
            environment_key,
            parameter_env: None,
            parameter_environment_key,
            module: false,
            eval_sites: Vec::new(),
            text: None,
            simple_parameters: false,
            in_parameters: false,
            script_lexicals: false,
            global_declarations: script,
            out: Emitter::default(),
            scopes: Vec::new(),
            next_slot: slots,
            max_slots: slots,
            completion: script.then_some(0),
            strict,
            strict_expression: false,
            own_arguments: false,
            class_initializer: false,
            constructor_kind: ConstructorKind::None,
            async_generator: false,
            instance_fields: None,
            lexical_this: false,
            this_capture: None,
            raw_this_capture: None,
            new_target_capture: None,
            super_capture: None,
            super_property: false,
            super_call: false,
            this_strict: strict,

            arguments_key,
            self_name,
            self_key,
            captures: Vec::new(),
            capture_index: HashMap::new(),
            children: Vec::new(),
            child_count: 0,
            exits: Vec::new(),
            using_resources: Vec::new(),
            loop_labels: Vec::new(),
            var_scope: 0,
            annex_b: HashSet::new(),
            annex_b_globals: HashMap::new(),
        });
    }

    /// Seal the outermost function and load the whole function tree (emit
    /// pass).
    fn finish_top(
        &mut self,
        name: &str,
        params: u16,
        length: u16,
        function_name: JsString,
    ) -> Result<Executable, SyntaxError> {
        let ctx = self.fns.pop().expect("the entry function");
        self.seal(ctx, name, params, length, function_name)?;
        self.load_all(name)
    }

    /// Resolve `ctx`'s branch targets and keep its code for
    /// [`Self::load_all`]; returns its index in `sealed`.
    fn seal(
        &mut self,
        ctx: FnCtx,
        name: &str,
        params: u16,
        length: u16,
        function_name: JsString,
    ) -> Result<usize, SyntaxError> {
        let FnCtx {
            mut out,
            max_slots,
            captures,
            strict,
            constructor_kind,
            children,
            eval_sites,
            text,
            simple_parameters,
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
            eval_sites,
            name: name.to_string(),
            params,
            function_length: length,
            function_name,
            locals: max_slots as u16,
            captures: captures.len() as u16,
            strict,
            constructor_kind,
            code: out.code,
            spans: out.spans,
            construct_callees: out.construct_callees,
            constants: out.constants,
            children,
            text,
            simple_parameters,
        });
        Ok(self.sealed.len() - 1)
    }

    /// Load every sealed function, children before parents (the order they
    /// were sealed in), and return the last one, the outermost. The source
    /// positions of all instructions are computed in one pass over the
    /// source, however many functions there are.
    fn load_all(&mut self, filename: &str) -> Result<Executable, SyntaxError> {
        let source = self.source;
        let filename: Arc<str> = Arc::from(filename);
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
                function_length: Some(f.function_length),
                function_name: Some(f.function_name),
                locals: f.locals,
                code: f.code,
                constants: f.constants,
                functions,
                captures: f.captures,
                strict: f.strict,
                constructor_kind: f.constructor_kind,
                eval_sites: f.eval_sites,
            };
            let shared = Arc::clone(&self.shared_source);
            let exe = Executable::load_compiled(
                spec,
                Arc::clone(&filename),
                shared,
                locations,
                f.construct_callees,
                f.text,
                f.simple_parameters,
            )
            .map_err(|e| {
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
        let saved_slot = self.enter_scope(&[body], params, top_level, 0)?;
        if body.iter().any(|stmt| {
            matches!(&stmt.kind,
            StmtKind::Lexical(decl) if matches!(decl.kind, LexicalKind::Using | LexicalKind::AwaitUsing))
        }) {
            self.disposable_body(body)?;
        } else {
            for stmt in body {
                self.statement(stmt)?;
            }
        }
        self.exit_scope(saved_slot);
        Ok(())
    }

    /// Protect a statement list with the ordinary finally completion protocol.
    /// The hidden method is undefined until validation succeeds; resources
    /// declared after an abrupt initializer are therefore never disposed.
    #[inline(never)]
    fn disposable_body(&mut self, body: &[Stmt]) -> Result<(), SyntaxError> {
        self.disposable_body_with(body, |this| {
            for stmt in body {
                this.statement(stmt)?;
            }
            Ok(())
        })
    }

    /// Modules mix ordinary statements with default-export initializers, but
    /// use the same resource lifetime and finally protocol as statement lists.
    fn disposable_body_with(
        &mut self,
        body: &[Stmt],
        emit: impl FnOnce(&mut Self) -> Result<(), SyntaxError>,
    ) -> Result<(), SyntaxError> {
        let Some(first) = body.first() else {
            return emit(self);
        };
        let resources: Vec<_> = body
            .iter()
            .filter_map(|stmt| match &stmt.kind {
                StmtKind::Lexical(decl)
                    if matches!(decl.kind, LexicalKind::Using | LexicalKind::AwaitUsing) =>
                {
                    Some(decl)
                }
                _ => None,
            })
            .flat_map(|decl| {
                decl.declarators
                    .iter()
                    .map(|d| (d.span, decl.kind == LexicalKind::AwaitUsing))
            })
            .collect();
        self.disposable_scope(&resources, first.span, emit)
    }

    /// Shared disposal scope for statement lists and for-using-of iterations.
    /// `body` initializes resources through `register_disposable` before use.
    fn disposable_scope(
        &mut self,
        resources: &[(Span, bool)],
        span: Span,
        body: impl FnOnce(&mut Self) -> Result<(), SyntaxError>,
    ) -> Result<(), SyntaxError> {
        let saved_resources = self.cur().using_resources.len();
        let saved_slot = self.cur().next_slot;
        let asynchronous = resources.iter().any(|(_, asynchronous)| *asynchronous);
        for &(resource, async_resource) in resources {
            let value = self.temp(resource)?;
            let method = self.temp(resource)?;
            let hint = if async_resource {
                Some(self.temp(resource)?)
            } else {
                None
            };
            self.out().op(Opcode::Undefined, resource);
            self.out().op_u16(Opcode::SetLocal, value, resource);
            self.out().op(Opcode::Undefined, resource);
            self.out().op_u16(Opcode::SetLocal, method, resource);
            if let Some(hint) = hint {
                self.constant(Primitive::Number(0.0), resource)?;
                self.out().op_u16(Opcode::SetLocal, hint, resource);
            }
            self.cur_mut()
                .using_resources
                .push((resource.start, value, method, hint));
        }
        if self.cur().using_resources.len() == saved_resources {
            return body(self);
        }
        let cleanup = self.out().label();
        self.out().branch(Opcode::PushFinally, cleanup, span);
        self.cur_mut().exits.push(Exit::Finally);
        body(self)?;
        self.out().op(Opcode::PopFinally, span);
        self.out().bind(cleanup);
        *self.cur_mut().exits.last_mut().unwrap() = Exit::FinallyBody;
        let resources = self.cur().using_resources[saved_resources..].to_vec();
        for (_, value, method, hint) in &resources {
            self.out().op_u16(Opcode::GetLocal, *value, span);
            self.out().op_u16(Opcode::GetLocal, *method, span);
            if asynchronous {
                if let Some(hint) = hint {
                    self.out().op_u16(Opcode::GetLocal, *hint, span);
                } else {
                    self.constant(Primitive::Number(0.0), span)?;
                }
            }
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, *value, span);
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, *method, span);
        }
        self.out().op(Opcode::PendingThrow, span);
        self.out().op(Opcode::PendingException, span);
        self.out().op(Opcode::PendingCancel, span);
        let count = u16::try_from(resources.len())
            .map_err(|_| self.internal(span, "too many using resources"))?;
        self.out().op_u16(
            if asynchronous {
                Opcode::UsingDisposeAsync
            } else {
                Opcode::UsingDispose
            },
            count,
            span,
        );
        self.out().op(Opcode::Pop, span);
        self.out().op(Opcode::EndFinally, span);
        self.cur_mut().exits.pop();
        self.cur_mut().using_resources.truncate(saved_resources);
        self.cur_mut().next_slot = saved_slot;
        Ok(())
    }

    /// Enter the scope of a statement list made of `clauses` (one slice for
    /// a script, function body or block; the clause bodies of a `switch`
    /// case block): declare `params` (a function body) and every binding of
    /// the list, create the cells of its cell bindings and the closures of
    /// its function declarations. The `let`/`const` bindings declared
    /// directly in `clauses[..checked]` are kept in cells whatever the facts
    /// say, so every reference after their declaration checks at run time
    /// that they are initialized: control can enter a later clause past
    /// them (clj-quickjs-83). Returns the slot count to restore with
    /// [`Self::exit_scope`].
    fn enter_scope(
        &mut self,
        clauses: &[&[Stmt]],
        params: &[Ident],
        top_level: bool,
        checked: usize,
    ) -> Result<usize, SyntaxError> {
        let saved_slot = self.cur().next_slot;
        self.cur_mut().scopes.push(Scope::new());
        let mut declared: Vec<&Ident> = Vec::new();
        let mut annex_b_globals: Vec<&Ident> = Vec::new();
        // Facts are complete in the emit pass, so whether this function's
        // arguments object is mapped is known before its parameters are
        // declared (light-cljs-860).
        let mapped = top_level && !self.analyzing() && self.wants_mapped_arguments(params);
        for p in params {
            self.declare(p, BindingKind::Param, top_level, false)?;
        }
        // A mapped arguments object aliases each parameter: every parameter
        // lives in the binding cell NewMappedArguments makes for it.
        if mapped {
            let scope = self.cur_mut().scopes.last_mut().expect("function scope");
            for p in params {
                if let Some(b) = scope.get_mut(&p.name) {
                    b.repr = Repr::Cell;
                }
            }
        }
        // A sloppy simple parameter list may repeat a name (the parser
        // rejects it elsewhere); its last occurrence is the binding.
        let mut seen = HashSet::new();
        let mut last: Vec<&Ident> = params
            .iter()
            .rev()
            .filter(|p| seen.insert(p.name.as_str()))
            .collect();
        last.reverse();
        declared.extend(last);
        // Analysis resolves the static fallbacks and marks direct eval sites;
        // emission inserts an environment only in the affected activations.
        // Allocate after argument slots and after parameter expressions, so
        // closures created by defaults never capture the body's environment.
        if top_level
            && self.cur().completion.is_none()
            && !self.cur().strict
            && !self.analyzing()
            && self.facts_of(self.cur().self_key).has_eval
        {
            let slot = self.temp(Span::new(0, 0))?;
            let key = self.cur().environment_key;
            self.cur_mut().dynamic_env = Some((Place::Local(slot), key));
            let at = Span::new(0, 0);
            self.out().op(Opcode::NewCell, at);
            self.out().op(Opcode::Dup, at);
            self.out().op(Opcode::NewObject, at);
            self.out().op(Opcode::CellInit, at);
            self.out().op_u16(Opcode::SetLocal, slot, at);
        }
        for (i, clause) in clauses.iter().enumerate() {
            for stmt in clause.iter() {
                match &stmt.kind {
                    StmtKind::Lexical(decl) => {
                        let kind = match decl.kind {
                            LexicalKind::Let => BindingKind::Let,
                            LexicalKind::Const | LexicalKind::Using | LexicalKind::AwaitUsing => {
                                BindingKind::Const
                            }
                        };
                        for d in &decl.declarators {
                            for name in d.name.names() {
                                if self.declare(name, kind, top_level, i < checked)? {
                                    declared.push(name);
                                }
                            }
                        }
                    }
                    StmtKind::FunctionDecl(f) => {
                        let name = self.declaration_name(f)?;
                        if self.declare(name, BindingKind::Function, top_level, false)? {
                            declared.push(name);
                        }
                    }
                    _ => {}
                }
            }
        }
        if top_level {
            // The `var` bindings of the whole body, nested blocks included
            // (not nested functions), after its parameters and lexical and
            // function declarations, which they may share a binding with.
            let mut vars = Vec::new();
            for clause in clauses {
                var_names(clause, &mut vars);
            }
            for name in vars {
                if self.declare(name, BindingKind::Var, true, false)? {
                    declared.push(name);
                }
            }
            if !self.cur().strict {
                self.annex_b_bindings(clauses, params, &mut declared, &mut annex_b_globals)?;
            }
        }
        let arguments = if top_level && self.cur().var_scope == 0 {
            self.arguments_binding(
                clauses
                    .first()
                    .and_then(|c| c.first())
                    .map_or(Span::new(0, 0), |s| s.span),
                mapped,
            )?
        } else {
            None
        };
        // All lexical conflicts precede every property-admissibility check,
        // so a later conflict is a SyntaxError even when an earlier name
        // would otherwise fail to declare its property with TypeError.
        for name in &declared {
            if self.local(name)?.global {
                let index = self.string_index(JsString::from(name.name.as_str()), name.span)?;
                self.out()
                    .op_u16(Opcode::CheckGlobalLexicalConflict, index, name.span);
            }
        }
        // Validate every object declaration before creating any properties
        // or initializing hoisted functions. Eval may collide with the
        // script's declarative environment or a restricted object property.
        for name in &declared {
            let b = self.local(name)?;
            if b.global {
                let index = self.string_index(JsString::from(name.name.as_str()), name.span)?;
                self.out().op_u16(
                    if b.kind == BindingKind::Function {
                        Opcode::CheckGlobalFunction
                    } else {
                        Opcode::CheckGlobalVar
                    },
                    index,
                    name.span,
                );
            }
        }
        // Annex B's global bindings come after the checks of the ordinary
        // declarations and before their creation (B.3.2.2 step 12).
        for name in annex_b_globals {
            self.annex_b_global(name)?;
        }
        // Cells first, so hoisted closures can capture any of them. A
        // parameter's cell holds its argument and a `var`'s `undefined`;
        // the others start uninitialized.
        for name in declared {
            let b = self.local(name)?;
            if arguments.is_some_and(|a| a.key == b.key) {
                continue;
            }
            // NewMappedArguments made the parameters' cells.
            if mapped && arguments.is_some() && b.kind == BindingKind::Param {
                continue;
            }
            if b.global {
                let index = self.string_index(JsString::from(name.name.as_str()), name.span)?;
                let op = if b.kind == BindingKind::Function {
                    Opcode::DeclareGlobalFunction
                } else {
                    Opcode::DeclareGlobalVar
                };
                self.out().op_u16(op, index, name.span);
                continue;
            }
            if b.global_lexical {
                let index = self.string_index(JsString::from(name.name.as_str()), name.span)?;
                if b.repr == Repr::Cell {
                    self.out().op(Opcode::NewCell, name.span);
                    self.out().op_u16(
                        if b.kind == BindingKind::Const {
                            Opcode::DeclareGlobalConst
                        } else {
                            Opcode::DeclareGlobalLet
                        },
                        index,
                        name.span,
                    );
                    self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                } else {
                    self.out()
                        .op_u16(Opcode::DeclareGlobalConstValue, index, name.span);
                }
                continue;
            }
            if self.sloppy_eval
                && self.fns.len() == 1
                && top_level
                && matches!(b.kind, BindingKind::Var | BindingKind::Function)
            {
                if self.eval_variable_bindings.contains(&name.name) {
                    let mut access = self.eval_bindings[&name.name];
                    while let Access::Dynamic(i) = access {
                        access = self.dynamic_accesses[i].3;
                    }
                    let Access::Found { place, .. } = access else {
                        return Err(self.internal(name.span, "eval variable binding is missing"));
                    };
                    self.out().load(place, name.span);
                } else {
                    self.eval_var_cell(&name.name, name.span)?;
                }
                self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                continue;
            }
            if top_level && b.kind == BindingKind::Var && self.cur().var_scope > 0 {
                self.init_body_var(name, b);
                continue;
            }
            if b.repr != Repr::Cell {
                continue;
            }
            let span = name.span;
            let new_cell = self.new_cell_opcode(b);
            self.out().op(new_cell, span);
            match b.kind {
                BindingKind::Param => {
                    self.out().op(Opcode::Dup, span);
                    self.out().op_u16(Opcode::GetLocal, b.slot, span);
                    self.out().op(Opcode::CellInit, span);
                }
                BindingKind::Var => {
                    self.out().op(Opcode::Dup, span);
                    self.out().op(Opcode::Undefined, span);
                    self.out().op(Opcode::CellInit, span);
                }
                BindingKind::Import
                | BindingKind::Let
                | BindingKind::Const
                | BindingKind::Function => {}
            }
            self.out().op_u16(Opcode::SetLocal, b.slot, span);
        }
        // Hoisted function declarations, in source order. At the top level,
        // several may share one binding (with each other or a parameter);
        // the last one wins.
        for stmt in clauses.iter().flat_map(|c| c.iter()) {
            if let StmtKind::FunctionDecl(f) = &stmt.kind {
                let name = self.declaration_name(f)?;
                let b = self.local(name)?;
                let facts = self.facts_of(b.key);
                let mutable = b.global
                    || (self.sloppy_eval && self.fns.len() == 1 && top_level)
                    || (!self.analyzing() && facts.assigned);
                let self_name =
                    (!mutable).then(|| (name.name.clone(), SelfKind::Declaration { key: b.key }));
                if b.global {
                    self.closure(f, None, None)?;
                    // Declaration instantiation initializes the variable
                    // binding, whereas references in eval code must still
                    // consult any intervening object environments.
                    let access = self.global_access(&name.name);
                    self.store(access, name.span)?;
                    self.out().op(Opcode::Pop, name.span);
                } else if b.repr == Repr::Cell {
                    self.out().op_u16(Opcode::GetLocal, b.slot, f.span);
                    self.closure(f, self_name, None)?;
                    // `cell_init` also overwrites an initialized cell.
                    self.out().op(Opcode::CellInit, f.span);
                } else {
                    self.closure(f, self_name, None)?;
                    self.out().op_u16(Opcode::SetLocal, b.slot, f.span);
                }
                self.initialize(name)?;
            }
        }
        Ok(saved_slot)
    }

    /// Declare the `var` bindings that ECMAScript Annex B.3.3 gives a sloppy
    /// script, function or eval body for its block function declarations
    /// (FunctionDeclarationInstantiation, GlobalDeclarationInstantiation and
    /// EvalDeclarationInstantiation as changed by B.3.2.1 to B.3.2.3): one
    /// per name of an eligible declaration ([`annex_b_functions`]) that is
    /// not a parameter, not `arguments` in a function (as in QuickJS) and,
    /// in direct eval code, not a lexical binding of the calling code
    /// between the eval and its variable environment. Such a binding is
    /// `undefined` until a declaration of the name is evaluated
    /// ([`Self::annex_b_copy`]), unless the name already is a `var` or
    /// function binding of the body, which it then is. A new global one
    /// (script code, or eval code whose variable environment is the global
    /// one) goes to `globals` instead of `declared`, for
    /// [`Self::annex_b_global`]. Each eligible declaration is recorded in
    /// [`FnCtx::annex_b`].
    #[inline(never)]
    fn annex_b_bindings<'a>(
        &mut self,
        clauses: &[&'a [Stmt]],
        params: &[Ident],
        declared: &mut Vec<&'a Ident>,
        globals: &mut Vec<&'a Ident>,
    ) -> Result<(), SyntaxError> {
        let mut functions = Vec::new();
        annex_b_functions(clauses, &mut functions);
        let function_code = self.cur().completion.is_none();
        let direct_eval = self.sloppy_eval && self.fns.len() == 1;
        for f in functions {
            let name = self.declaration_name(f)?;
            let ctx = self.cur();
            let parameter = params.iter().any(|p| p.name == name.name)
                || (ctx.var_scope > 0
                    && ctx.scopes[0]
                        .get(&name.name)
                        .is_some_and(|b| b.kind == BindingKind::Param));
            if parameter
                || (function_code && name.name == "arguments")
                || (direct_eval && self.eval_lexical_conflicts.contains(&name.name))
            {
                continue;
            }
            self.cur_mut().annex_b.insert(f.span.start);
            // The block binding has the name's offset as its key; this
            // binding takes the declaration's, where no name starts.
            if self.declare_keyed(name, BindingKind::Var, true, false, f.span.start)? {
                if self.local(name)?.global {
                    globals.push(name);
                } else {
                    declared.push(name);
                }
            }
        }
        Ok(())
    }

    /// Create the global `var` binding of the Annex B block function `name`
    /// only if no global lexical binding has the name and the global object
    /// can define it (GlobalDeclarationInstantiation and
    /// EvalDeclarationInstantiation as changed by B.3.2.2 and B.3.2.3):
    /// the ordinary checks, whose errors are caught here, decide it, and a
    /// local flag records it for [`Self::annex_b_copy`], which copies
    /// nothing when the binding was not created.
    #[inline(never)]
    fn annex_b_global(&mut self, name: &Ident) -> Result<(), SyntaxError> {
        let at = name.span;
        let flag = self.temp(at)?;
        let index = self.string_index(JsString::from(name.name.as_str()), at)?;
        let skipped = self.out().label();
        let end = self.out().label();
        self.constant(Primitive::Bool(false), at)?;
        self.out().op_u16(Opcode::SetLocal, flag, at);
        self.out().branch(Opcode::PushCatch, skipped, at);
        self.out()
            .op_u16(Opcode::CheckGlobalLexicalConflict, index, at);
        self.out().op_u16(Opcode::CheckGlobalVar, index, at);
        self.out().op_u16(Opcode::DeclareGlobalVar, index, at);
        self.out().op(Opcode::PopCatch, at);
        self.constant(Primitive::Bool(true), at)?;
        self.out().op_u16(Opcode::SetLocal, flag, at);
        self.out().branch(Opcode::Jump, end, at);
        self.out().bind(skipped);
        self.out().op(Opcode::Pop, at);
        self.out().bind(end);
        self.cur_mut()
            .annex_b_globals
            .insert(name.name.clone(), flag);
        Ok(())
    }

    /// Evaluate the function declaration `f`, which was created when its
    /// statement list was entered. If Annex B.3.3 also binds it with `var`
    /// ([`Self::annex_b_bindings`]), copy the block binding's value, the
    /// closure made when the block was entered, to the body's `var` binding
    /// of the name in the body's variable environment itself, never through
    /// a `with` object or block in between (also not the calling code's, for
    /// eval code). A global binding that [`Self::annex_b_global`] did not
    /// create gets no copy. Out of line: the statement frame stays small.
    #[inline(never)]
    fn annex_b_copy(&mut self, f: &Function) -> Result<(), SyntaxError> {
        if !self.cur().annex_b.contains(&f.span.start) {
            return Ok(());
        }
        let name = self.declaration_name(f)?;
        let at = name.span;
        let saved = self.cur().next_slot;
        let level = self.fns.len() - 1;
        let ctx = self.cur();
        let var = ctx.scopes[ctx.var_scope]
            .get(&name.name)
            .copied()
            .ok_or_else(|| self.internal(at, "Annex B binding was not pre-declared"))?;
        let skip = self.out().label();
        if var.global {
            if let Some(&flag) = self.cur().annex_b_globals.get(&name.name) {
                self.out().op_u16(Opcode::GetLocal, flag, at);
                self.out().branch(Opcode::JumpIfFalse, skip, at);
            }
            let block = self.resolve(&name.name);
            self.load(block, at)?;
            let target = self.global_access(&name.name);
            self.store(target, at)?;
            self.out().op(Opcode::Pop, at);
        } else if level == 0 && self.sloppy_eval {
            // The eval body's binding holds the cell of the variable in the
            // calling code's variable environment (`enter_scope`). A deleted
            // eval-created variable is created again, as a sloppy
            // SetMutableBinding does.
            if self.eval_variable_bindings.contains(&name.name) {
                self.out().op_u16(Opcode::GetLocal, var.slot, at);
            } else {
                self.eval_var_cell(&name.name, at)?;
            }
            let block = self.resolve(&name.name);
            self.load(block, at)?;
            self.out().op(Opcode::CellSet, at);
        } else {
            let block = self.resolve(&name.name);
            self.load(block, at)?;
            let ctx = self.cur();
            let end = ctx.var_scope + 1;
            let with_count = ctx
                .with_envs
                .iter()
                .take_while(|(depth, _, _)| *depth < end)
                .count();
            let target = self.resolve_scopes(level, &name.name, end, with_count);
            self.store(target, at)?;
            self.out().op(Opcode::Pop, at);
        }
        self.out().bind(skip);
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// Leave the scope entered by [`Self::enter_scope`], which returned
    /// `saved_slot`.
    fn exit_scope(&mut self, saved_slot: usize) {
        let ctx = self.cur_mut();
        ctx.scopes.pop();
        ctx.next_slot = saved_slot;
    }

    fn declaration_name<'f>(&self, f: &'f Function) -> Result<&'f Ident, SyntaxError> {
        f.name
            .as_ref()
            .ok_or_else(|| self.internal(f.span, "function declaration without a name"))
    }

    /// Declare `name` in the innermost scope; returns whether that made a
    /// new binding. `checked`: keep it in a cell even if no closure needs
    /// one, for the cell's run-time temporal-dead-zone check
    /// ([`Self::enter_scope`]).
    ///
    /// At the top level of a script or function body (`top_level`),
    /// parameters, `var`s and function declarations are var-scoped
    /// (clj-quickjs-94): a `var` or function declaration that repeats one of
    /// them is that binding, not a new one. A function declaration then
    /// assigns it at scope entry, after the argument or an earlier function
    /// declaration, so the binding counts as assigned. Any other repeated
    /// name is a JavaScript early error, except a repeated parameter name,
    /// which the parser admits only in a sloppy simple parameter list: each
    /// occurrence keeps its argument slot and the last is the binding.
    fn declare(
        &mut self,
        name: &Ident,
        kind: BindingKind,
        top_level: bool,
        checked: bool,
    ) -> Result<bool, SyntaxError> {
        self.declare_keyed(name, kind, top_level, checked, name.span.start)
    }

    /// [`Self::declare`] with the binding's fact `key`, which is the name's
    /// source offset except for the `var` binding of an Annex B block
    /// function ([`Self::enter_scope`]), whose name is the block binding's.
    fn declare_keyed(
        &mut self,
        name: &Ident,
        kind: BindingKind,
        top_level: bool,
        checked: bool,
        key: usize,
    ) -> Result<bool, SyntaxError> {
        self.check_not_reserved(name)?;
        let prior = self
            .cur()
            .scopes
            .last()
            .and_then(|s| s.get(&name.name))
            .copied();
        if let Some(prior) = prior
            .filter(|p| !(top_level && kind == BindingKind::Param && p.kind == BindingKind::Param))
        {
            let var_scoped = |k| {
                matches!(
                    k,
                    BindingKind::Param | BindingKind::Function | BindingKind::Var
                )
            };
            if top_level && var_scoped(kind) && var_scoped(prior.kind) && kind != BindingKind::Param
            {
                if kind == BindingKind::Function && self.analyzing() {
                    self.fact(prior.key).assigned = true;
                }
                return Ok(false);
            }
            let what = match prior.kind {
                BindingKind::Param => "a parameter",
                BindingKind::Var => "a `var` binding",
                _ => "a binding in this scope",
            };
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                name.span,
                format!("'{}' is already declared as {what}", name.name),
            ));
        }
        if top_level && matches!(kind, BindingKind::Let | BindingKind::Const) {
            self.check_not_parameter(name)?;
        }
        let ctx = self.cur();
        if ctx.next_slot >= MAX_LOCALS {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                name.span,
                "more than 65,535 bindings are live at once",
            ));
        }
        let global_lexical = ctx.script_lexicals
            && top_level
            && matches!(kind, BindingKind::Let | BindingKind::Const);
        let binding = Binding {
            global_lexical,
            global: ctx.global_declarations
                && top_level
                && matches!(kind, BindingKind::Var | BindingKind::Function),
            kind,
            slot: ctx.next_slot as u16,
            initialized: matches!(kind, BindingKind::Param | BindingKind::Var),
            repr: if checked
                || (global_lexical && kind == BindingKind::Let)
                || (self.sloppy_eval
                    && self.fns.len() == 1
                    && top_level
                    && matches!(kind, BindingKind::Var | BindingKind::Function))
            {
                Repr::Cell
            } else {
                self.facts_of(key).repr()
            },
            key,
        };
        let ctx = self.cur_mut();
        ctx.next_slot += 1;
        ctx.max_slots = ctx.max_slots.max(ctx.next_slot);
        if let Some(scope) = ctx.scopes.last_mut() {
            scope.insert(name.name.clone(), binding);
        }
        Ok(true)
    }

    /// Reject a `var` declaration of `name` that a `let`, `const`, block
    /// function declaration or `for` head binding of an enclosing block of
    /// the running function would shadow (an early error: `var` names the
    /// function's binding, which such a declaration hides). A catch
    /// parameter does not count (ECMAScript Annex B, "VariableStatements in
    /// Catch Blocks", as in QuickJS's `define_var`): an initializer assigns
    /// the catch parameter, the innermost binding of that name.
    fn check_var_not_shadowed(&self, name: &Ident) -> Result<(), SyntaxError> {
        let blocks = self.cur().scopes.iter().skip(self.cur().var_scope + 1);
        let lexical = blocks
            .filter_map(|scope| scope.get(&name.name))
            .any(|b| b.kind != BindingKind::Param);
        if lexical {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                name.span,
                format!(
                    "'{}' is already declared by `let`, `const` or a function declaration in an enclosing block",
                    name.name
                ),
            ));
        }
        Ok(())
    }

    /// The declarators of a `var` statement or `for` head: the bindings
    /// exist from function entry (`undefined`); each initializer is an
    /// assignment, and `var x;` does nothing.
    fn var_declarators(&mut self, declarators: &[Declarator]) -> Result<(), SyntaxError> {
        for d in declarators {
            for name in d.name.names() {
                self.check_var_not_shadowed(name)?;
            }
            if let Some(init) = &d.init {
                if let Some(name) = d.name.identifier() {
                    self.assign(name, init)?;
                    self.out().op(Opcode::Pop, name.span);
                } else {
                    self.pattern_value(init, &d.name)?;
                    self.pattern_store(&d.name, false, false)?;
                }
            }
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
        if name == RESERVED_BINDING {
            return Access::Intrinsic;
        }
        self.resolve_in(self.fns.len() - 1, name)
    }

    #[inline(never)]
    fn global_access(&mut self, name: &str) -> Access {
        let index = self.global_names.len();
        self.global_names.push(name.to_string());
        Access::Global(index)
    }

    #[inline(never)]
    fn global_op(&mut self, op: Opcode, name: usize, at: Span) -> Result<(), SyntaxError> {
        let name = JsString::from(self.global_names[name].as_str());
        let index = self.string_index(name, at)?;
        self.out().op_u16(op, index, at);
        Ok(())
    }

    /// Resolve a source reference through lexical bindings and the global object.
    #[inline(never)]
    fn resolve_reference(&mut self, name: &str) -> Access {
        self.resolve(name)
    }

    /// Install an ordinary function's implicit arguments binding in its
    /// parameter scope (or body scope without defaults). Parameters and
    /// lexical/function declarations shadow it; a plain `var` keeps it.
    /// Arrows and scripts introduce no implicit binding.
    ///
    /// `mapped`: the object is a mapped one ([`Self::wants_mapped_arguments`]),
    /// made by NewMappedArguments, which also turns every parameter local
    /// into the binding cell it aliases.
    #[inline(never)]
    fn arguments_binding(
        &mut self,
        span: Span,
        mapped: bool,
    ) -> Result<Option<Binding>, SyntaxError> {
        if !self.cur().own_arguments {
            return Ok(None);
        }
        let prior = self
            .cur()
            .scopes
            .last()
            .and_then(|s| s.get("arguments"))
            .copied();
        if prior.is_some_and(|b| b.kind != BindingKind::Var) {
            return Ok(None);
        }
        let key = prior.map_or(self.cur().arguments_key, |b| b.key);
        // The analysis pass installs the candidate binding so nested arrows
        // resolve normally. The emit pass allocates it only if referenced.
        let facts = self.facts_of(key);
        let referenced = facts.referenced
            || facts
                .copied_to
                .is_some_and(|body| self.facts_of(body).referenced);
        if !self.analyzing() && !referenced {
            return Ok(None);
        }
        let b = match prior {
            Some(b) => b,
            None => {
                let b = Binding {
                    global_lexical: false,
                    global: false,
                    kind: BindingKind::Var,
                    // Analysis bytecode is discarded. A symbolic slot keeps
                    // an unused candidate from reducing the local budget,
                    // including space needed by subsequent body declarations.
                    slot: if self.analyzing() {
                        u16::MAX
                    } else {
                        self.temp(span)?
                    },
                    initialized: true,
                    repr: self.facts_of(key).repr(),
                    key,
                };
                self.cur_mut()
                    .scopes
                    .last_mut()
                    .expect("function scope")
                    .insert("arguments".into(), b);
                b
            }
        };
        if b.repr == Repr::Cell {
            self.out().op(Opcode::NewCell, span);
            self.out().op(Opcode::Dup, span);
        }
        self.out().op(
            if mapped {
                Opcode::NewMappedArguments
            } else {
                Opcode::NewArguments
            },
            span,
        );
        if b.repr == Repr::Cell {
            self.out().op(Opcode::CellInit, span);
        }
        self.out().op_u16(Opcode::SetLocal, b.slot, span);
        Ok(Some(b))
    }

    /// Whether the function being lowered gets a mapped arguments object
    /// (light-cljs-860; ECMAScript 10.4.4.7 CreateMappedArgumentsObject,
    /// QuickJS `js_build_mapped_arguments`): only when lowering for Goose,
    /// for a sloppy ordinary function or method with a simple, nonempty
    /// parameter list that no parameter names `arguments`, and only when
    /// the aliasing could be observed. A function that refers to its
    /// `arguments` only as the base of property reads, assigns no
    /// parameter and has no direct eval cannot tell a mapped object from
    /// an unmapped one with the same `callee`, so it keeps NewArguments and
    /// plain parameter locals. (A getter on Object.prototype reached by
    /// such a read past the arguments' length receives the object as its
    /// receiver; docs/javascript-coverage.md, decision 2, records that
    /// limit.) The parameters' facts are complete in the emit pass.
    fn wants_mapped_arguments(&self, params: &[Ident]) -> bool {
        let ctx = self.cur();
        if self.backend != Backend::Goose
            || !ctx.own_arguments
            || ctx.strict
            || !ctx.simple_parameters
            || ctx.var_scope != 0
            || params.is_empty()
            || params.iter().any(|p| p.name == "arguments")
        {
            return false;
        }
        let own = self.facts_of(ctx.self_key);
        own.has_eval
            || own.arguments_used
                && (own.arguments_escape
                    || params.iter().any(|p| self.facts_of(p.span.start).assigned))
    }

    /// Resolve `name` as seen from function `level`, adding captures to it
    /// (and to the functions between it and the declaring one) as needed.
    /// Recursion depth is the function nesting depth, bounded by the tree
    /// height.
    fn resolve_in(&mut self, level: usize, name: &str) -> Access {
        self.resolve_scopes(
            level,
            name,
            self.fns[level].scopes.len(),
            if name.starts_with('#') {
                0
            } else {
                self.fns[level].with_envs.len()
            },
        )
    }

    fn resolve_scopes(
        &mut self,
        level: usize,
        name: &str,
        end: usize,
        with_count: usize,
    ) -> Access {
        let ctx = &self.fns[level];
        let with_env = with_count.checked_sub(1).map(|i| ctx.with_envs[i]);
        let start = with_env.map_or_else(
            || {
                if ctx.dynamic_env.is_some() && !name.starts_with('#') {
                    ctx.var_scope
                } else {
                    0
                }
            },
            |e| e.0,
        );
        if let Some(b) = ctx.scopes[..end]
            .iter()
            .skip(start)
            .rev()
            .find_map(|scope| scope.get(name))
            .copied()
        {
            if level == 0
                && self.sloppy_eval
                && matches!(b.kind, BindingKind::Var | BindingKind::Function)
                && self.fns[level].scopes[self.fns[level].var_scope]
                    .get(name)
                    .is_some_and(|body| body.key == b.key)
            {
                return self.resolve_outside_body(level, name);
            }
            if self.analyzing() {
                self.fact(b.key).referenced = true;
                if name == "arguments" && b.kind == BindingKind::Var {
                    let escape = !self.member_base;
                    let own = self.fact(self.fns[level].self_key);
                    own.arguments_used = true;
                    own.arguments_escape |= escape;
                }
            }
            if b.global {
                return self.global_access(name);
            }
            return Access::Found {
                place: Place::Local(b.slot),
                target: Target::Binding(b.kind),
                repr: b.repr,
                initialized: b.initialized,
                key: b.key,
            };
        }
        if let Some((depth, env, key)) = with_env {
            let fallback = self.resolve_scopes(level, name, depth, with_count - 1);
            return self.dynamic_access(env, key, name, fallback);
        }
        let env = self.fns[level]
            .dynamic_env
            .filter(|_| !name.starts_with('#'));
        let fallback = self.resolve_outside_body(level, name);
        if let Some((env, key)) = env {
            self.dynamic_access(env, key, name, fallback)
        } else {
            fallback
        }
    }

    fn resolve_outside_body(&mut self, level: usize, name: &str) -> Access {
        let ctx = &self.fns[level];
        if ctx.dynamic_env.is_some()
            && ctx.var_scope > 0
            && let Some(b) = ctx.scopes[0].get(name).copied()
        {
            if self.analyzing() {
                self.fact(b.key).referenced = true;
            }
            return Access::Found {
                place: Place::Local(b.slot),
                target: Target::Binding(b.kind),
                repr: b.repr,
                initialized: b.initialized,
                key: b.key,
            };
        }
        let fallback = self.resolve_outside_parameters(level, name);
        if let Some((env, key)) = self.fns[level]
            .parameter_env
            .filter(|_| !name.starts_with('#'))
        {
            self.dynamic_access(env, key, name, fallback)
        } else {
            fallback
        }
    }

    fn resolve_outside_parameters(&mut self, level: usize, name: &str) -> Access {
        let ctx = &self.fns[level];
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
            if let Some(access) = self.eval_bindings.get(name) {
                return *access;
            }
            let mut access = if name == RESERVED_BINDING {
                Access::Intrinsic
            } else {
                self.global_access(name)
            };
            for &(env, key) in self
                .eval_environments
                .clone()
                .iter()
                .rev()
                .filter(|_| !name.starts_with('#'))
            {
                access = self.dynamic_access(env, key, name, access);
            }
            return access;
        }
        let access = self.resolve_in(level - 1, name);
        self.capture_access(level, access)
    }

    fn capture_access(&mut self, level: usize, access: Access) -> Access {
        match access {
            Access::Dynamic(i) => {
                let (env, key, name, fallback) = self.dynamic_accesses[i].clone();
                let env = self.capture_place(level, env, key);
                let fallback = self.capture_access(level, fallback);
                self.dynamic_access(env, key, &name, fallback)
            }
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
                let place = self.capture_place(level, place, key);
                Access::Found {
                    place,
                    target,
                    repr,
                    initialized: true,
                    key,
                }
            }
            other => other,
        }
    }

    fn capture_place(&mut self, level: usize, place: Place, key: usize) -> Place {
        let ctx = &mut self.fns[level];
        let index = *ctx.capture_index.entry(key).or_insert_with(|| {
            let index = ctx.captures.len().min(MAX_COUNT) as u16;
            ctx.captures.push(place);
            index
        });
        Place::Capture(index)
    }

    fn dynamic_access(&mut self, env: Place, key: usize, name: &str, fallback: Access) -> Access {
        let index = self.dynamic_accesses.len();
        self.dynamic_accesses
            .push((env, key, name.into(), fallback));
        Access::Dynamic(index)
    }

    fn environment_lookup(&mut self, env: Place, name: &str, at: Span) -> Result<(), SyntaxError> {
        let object = self.out().label();
        let done = self.out().label();
        self.out().load(env, at);
        self.out().op(Opcode::IsCell, at);
        self.out().branch(Opcode::JumpIfFalse, object, at);
        self.variable_lookup(env, name, at)?;
        self.out().branch(Opcode::Jump, done, at);
        self.out().bind(object);
        // HasBinding checks HasProperty before reading @@unscopables. All
        // observable reads use ordinary bytecode, whose getters can suspend.
        let absent = self.out().label();
        let primitive = self.out().label();
        let index = self.name_index(name, at)?;
        self.out().op_u16(Opcode::Const, index, at);
        self.out().load(env, at);
        self.out().op(Opcode::In, at);
        self.out().branch(Opcode::JumpIfFalse, absent, at);
        self.out().load(env, at);
        self.constant(
            Primitive::Symbol(crate::value::symbol::well_known("unscopables")),
            at,
        )?;
        self.out().op(Opcode::GetElem, at);
        self.out().op(Opcode::Dup, at);
        self.out().op(Opcode::IsObject, at);
        self.out().branch(Opcode::JumpIfFalse, primitive, at);
        self.out().op_u16(Opcode::GetField, index, at);
        self.out().op(Opcode::Not, at);
        self.out().branch(Opcode::Jump, done, at);
        self.out().bind(primitive);
        self.out().op(Opcode::Pop, at);
        self.constant(Primitive::Bool(true), at)?;
        self.out().branch(Opcode::Jump, done, at);
        self.out().bind(absent);
        self.constant(Primitive::Bool(false), at)?;
        self.out().bind(done);
        Ok(())
    }

    fn variable_lookup(&mut self, env: Place, name: &str, at: Span) -> Result<(), SyntaxError> {
        let index = self.name_index(name, at)?;
        self.out().load(env, at);
        self.out().op(Opcode::CellGet, at);
        self.out().op_u16(Opcode::GetField, index, at);
        Ok(())
    }

    /// GetValue on an already selected reference, without HasBinding again.
    fn environment_get(&mut self, env: Place, name: &str, at: Span) -> Result<(), SyntaxError> {
        let object = self.out().label();
        let done = self.out().label();
        self.out().load(env, at);
        self.out().op(Opcode::IsCell, at);
        self.out().branch(Opcode::JumpIfFalse, object, at);
        self.variable_lookup(env, name, at)?;
        self.out().op(Opcode::CellGet, at);
        self.out().branch(Opcode::Jump, done, at);
        self.out().bind(object);
        self.check_object_binding(env, name, at)?;
        self.out().load(env, at);
        let index = self.name_index(name, at)?;
        self.out().op_u16(Opcode::GetField, index, at);
        self.out().bind(done);
        Ok(())
    }

    /// Object GetBindingValue/SetMutableBinding recheck existence after
    /// unscopables getters or the RHS may have deleted the selected property.
    fn check_object_binding(
        &mut self,
        env: Place,
        name: &str,
        at: Span,
    ) -> Result<(), SyntaxError> {
        if self.cur().strict || self.cur().strict_expression {
            let missing = self.out().label();
            let done = self.out().label();
            let index = self.name_index(name, at)?;
            self.out().op_u16(Opcode::Const, index, at);
            self.out().load(env, at);
            self.out().op(Opcode::In, at);
            self.out().branch(Opcode::JumpIfFalse, missing, at);
            self.out().branch(Opcode::Jump, done, at);
            self.out().bind(missing);
            self.throw(ThrowKind::Unresolvable, at);
            // ThrowError has an expression result in the bytecode verifier.
            self.out().op(Opcode::Pop, at);
            self.out().bind(done);
        }
        Ok(())
    }

    fn lookup_reference(&mut self, access: Access, at: Span) -> Result<Access, SyntaxError> {
        match access {
            Access::Dynamic(i) => {
                let (env, _, name, fallback) = self.dynamic_accesses[i].clone();
                self.environment_lookup(env, &name, at)?;
                Ok(fallback)
            }
            _ => Err(self.internal(at, "expected a dynamic reference")),
        }
    }

    /// Select the first dynamic environment without reading its binding. Resolving
    /// a reference precedes the RHS; TDZ/const checks still happen at GetValue
    /// or PutValue, and deletion during the RHS cannot retarget the store.
    fn select_reference(&mut self, access: Access, at: Span) -> Result<Access, SyntaxError> {
        if !matches!(access, Access::Dynamic(_)) {
            self.out().op(Opcode::Undefined, at);
            return Ok(access);
        }
        let fallback = self.lookup_reference(access, at)?;
        let Access::Dynamic(i) = access else {
            unreachable!()
        };
        let env = self.dynamic_accesses[i].0;
        let missing = self.out().label();
        let done = self.out().label();
        self.out().op(Opcode::Dup, at);
        self.out().branch(Opcode::JumpIfFalse, missing, at);
        self.out().op(Opcode::Pop, at);
        self.out().load(env, at);
        self.out().branch(Opcode::Jump, done, at);
        self.out().bind(missing);
        self.out().op(Opcode::Pop, at);
        let fallback = self.select_reference(fallback, at)?;
        self.out().bind(done);
        Ok(fallback)
    }

    fn prepare_reference(&mut self, access: Access, at: Span) -> Result<Access, SyntaxError> {
        if !matches!(access, Access::Dynamic(_)) {
            return Ok(access);
        }
        let slot = self.temp(at)?;
        let Access::Dynamic(i) = access else {
            unreachable!()
        };
        let name = self.dynamic_accesses[i].2.clone();
        let fallback = self.select_reference(access, at)?;
        self.out().op_u16(Opcode::SetLocal, slot, at);
        let index = self.prepared_accesses.len();
        self.prepared_accesses.push((name, fallback));
        Ok(Access::Prepared {
            slot,
            fallback: index,
        })
    }

    /// Create a deletable binding if absent; repeated evals reuse its cell.
    fn eval_var_cell(&mut self, name: &str, at: Span) -> Result<(), SyntaxError> {
        let env = self
            .eval_variable_environment
            .ok_or_else(|| self.internal(at, "missing eval environment"))?;
        self.environment_cell(env, name, at, false)
    }

    /// SetMutableBinding rechecks the selected environment by name. A sloppy
    /// store recreates a deleted binding; a strict store reports ReferenceError.
    fn environment_cell(
        &mut self,
        env: Place,
        name: &str,
        at: Span,
        strict: bool,
    ) -> Result<(), SyntaxError> {
        let done = self.out().label();
        self.variable_lookup(env, name, at)?;
        self.out().op(Opcode::Dup, at);
        self.out().op(Opcode::Not, at);
        self.out().branch(Opcode::JumpIfFalse, done, at);
        self.out().op(Opcode::Pop, at);
        if strict {
            self.throw(ThrowKind::Unresolvable, at);
            self.out().bind(done);
            return Ok(());
        }
        self.out().op(Opcode::NewCell, at);
        self.out().op(Opcode::Dup, at);
        self.out().op(Opcode::Undefined, at);
        self.out().op(Opcode::CellInit, at);
        let tmp = self.temp(at)?;
        self.out().op(Opcode::Dup, at);
        self.out().op_u16(Opcode::SetLocal, tmp, at);
        self.out().load(env, at);
        self.out().op(Opcode::CellGet, at);
        self.out().op_u16(Opcode::GetLocal, tmp, at);
        let index = self.name_index(name, at)?;
        self.out().op_u16(Opcode::DefineField, index, at);
        self.out().op(Opcode::Pop, at);
        self.cur_mut().next_slot -= 1;
        self.out().bind(done);
        Ok(())
    }

    /// Capture `this` through each intervening arrow, stopping at an
    /// ordinary function or the script global object. Capture counts are
    /// checked by `closure`, as for named bindings.
    #[inline(never)]
    fn resolve_this(&mut self, level: usize, raw: bool) -> Place {
        if self.fns[level].completion.is_some() {
            return self.eval_this.map_or(Place::GlobalObject, Place::Capture);
        }
        if !self.fns[level].lexical_this {
            return if raw || self.fns[level].constructor_kind.is_derived() {
                Place::RawThis
            } else {
                Place::This
            };
        }
        let captured = if raw {
            self.fns[level].raw_this_capture
        } else {
            self.fns[level].this_capture
        };
        if let Some(index) = captured {
            return Place::Capture(index);
        }
        let parent = self.resolve_this(level - 1, raw);
        let ctx = &mut self.fns[level];
        let index = ctx.captures.len().min(MAX_COUNT) as u16;
        ctx.captures.push(parent);
        if raw {
            ctx.raw_this_capture = Some(index);
        } else {
            ctx.this_capture = Some(index);
        }
        Place::Capture(index)
    }

    /// Arrows capture the immutable construct target by value through each
    /// enclosing arrow. Eval imports it alongside its receiver, and ordinary
    /// functions always start a fresh binding (even when created by eval).
    fn resolve_new_target(&mut self, level: usize) -> Option<Place> {
        if self.fns[level].module {
            return None;
        }
        if self.fns[level].completion.is_some() {
            return self.eval_new_target.map(Place::Capture);
        }
        if !self.fns[level].lexical_this {
            return Some(Place::NewTarget);
        }
        if let Some(index) = self.fns[level].new_target_capture {
            return Some(Place::Capture(index));
        }
        let parent = self.resolve_new_target(level - 1)?;
        let ctx = &mut self.fns[level];
        let index = ctx.captures.len().min(MAX_COUNT) as u16;
        ctx.captures.push(parent);
        ctx.new_target_capture = Some(index);
        Some(Place::Capture(index))
    }

    fn resolve_super_function(&mut self, level: usize) -> Place {
        if self.fns[level].completion.is_some()
            && let Some(index) = self.eval_super_home
        {
            return Place::Capture(index);
        }
        if !self.fns[level].lexical_this {
            return Place::Current;
        }
        if let Some(index) = self.fns[level].super_capture {
            return Place::Capture(index);
        }
        let parent = self.resolve_super_function(level - 1);
        let ctx = &mut self.fns[level];
        let index = ctx.captures.len().min(MAX_COUNT) as u16;
        ctx.captures.push(parent);
        ctx.super_capture = Some(index);
        Place::Capture(index)
    }

    fn resolve_instance_fields(&mut self, level: usize, span: Span) -> Result<Access, SyntaxError> {
        if let Some(access) = self.fns[level].instance_fields {
            return Ok(access);
        }
        if level == 0 || !self.fns[level].lexical_this {
            return Err(self.internal(span, "missing constructor field initializer"));
        }
        let parent = self.resolve_instance_fields(level - 1, span)?;
        let access = self.capture_access(level, parent);
        self.fns[level].instance_fields = Some(access);
        Ok(access)
    }

    /// Keep the receiver separate from the home object's current prototype.
    /// A method call retains a second receiver below the read's operands.
    fn super_base(&mut self, span: Span, method: bool) -> Result<(), SyntaxError> {
        self.literal(&Expr::new(ExprKind::This, span))?;
        if method {
            self.out().op(Opcode::Dup, span);
        }
        let home = self.resolve_super_function(self.fns.len() - 1);
        self.out().load(home, span);
        self.out().op(Opcode::GetSuperBase, span);
        Ok(())
    }

    #[inline(never)]
    fn super_call(&mut self, args: &[Expr], span: Span, forward: bool) -> Result<(), SyntaxError> {
        if args.len() > MAX_COUNT {
            return Err(self.error(SyntaxErrorKind::TooLarge, span, "too many super arguments"));
        }
        let level = self.fns.len() - 1;
        let receiver = self.resolve_this(level, true);
        self.out().load(receiver, span);
        let constructor = self.resolve_super_function(level);
        self.out().load(constructor, span);
        self.out().op(Opcode::GetSuperConstructor, span);
        let target = self
            .resolve_new_target(level)
            .ok_or_else(|| self.internal(span, "super outside a constructor"))?;
        self.out().load(target, span);
        if forward {
            self.out().op(Opcode::ForwardArguments, span);
            self.out().op(Opcode::ConstructSuperSpread, span);
        } else if args.iter().any(|a| matches!(a.kind, ExprKind::Spread(_))) {
            self.spread_arguments(args, span)?;
            self.out().op(Opcode::ConstructSuperSpread, span);
        } else {
            for arg in args {
                self.expr(arg)?;
            }
            self.out()
                .op_u16(Opcode::ConstructSuper, args.len() as u16, span);
        }
        self.out().op(Opcode::BindThis, span);
        // Keep the super() expression's value while the method-like field
        // initializer runs as an ordinary, resumable VM activation.
        self.out().op(Opcode::Dup, span);
        let fields = self.resolve_instance_fields(level, span)?;
        self.load(fields, span)?;
        self.out().op_u16(Opcode::CallMethod, 0, span);
        self.out().op(Opcode::Pop, span);
        Ok(())
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
        self.load_reference(access, at, false)
    }

    fn load_reference(
        &mut self,
        access: Access,
        at: Span,
        typeof_name: bool,
    ) -> Result<(), SyntaxError> {
        match access {
            Access::Prepared { slot, fallback } => {
                let (name, fallback) = self.prepared_accesses[fallback].clone();
                let missing = self.out().label();
                let done = self.out().label();
                self.out().op_u16(Opcode::GetLocal, slot, at);
                self.out().branch(Opcode::JumpIfFalse, missing, at);
                self.environment_get(Place::Local(slot), &name, at)?;
                self.out().branch(Opcode::Jump, done, at);
                self.out().bind(missing);
                self.load_reference(fallback, at, typeof_name)?;
                self.out().bind(done);
            }
            Access::Dynamic(_) => {
                let saved = self.cur().next_slot;
                let prepared = self.prepare_reference(access, at)?;
                self.load_reference(prepared, at, typeof_name)?;
                self.cur_mut().next_slot = saved;
            }
            Access::Found {
                initialized: false, ..
            } => self.throw(ThrowKind::Uninitialized, at),
            Access::Found { place, repr, .. } => {
                self.out().load(place, at);
                if repr == Repr::Cell {
                    self.out().op(Opcode::CellGet, at);
                }
            }
            Access::Global(global) => self.global_op(
                if typeof_name {
                    Opcode::GetGlobalOrUndefined
                } else {
                    Opcode::GetGlobal
                },
                global,
                at,
            )?,
            Access::Intrinsic => return Err(self.not_a_value(at)),
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
                } => {
                    // From inside the function itself: a nested function
                    // of the declaring scope.
                    let facts = self.fact(key);
                    facts.assigned = true;
                    facts.assigned_in_closure = true;
                }
                Access::Found {
                    target: Target::Binding(_),
                    key,
                    place,
                    ..
                } => {
                    let facts = self.fact(key);
                    facts.assigned = true;
                    if !matches!(place, Place::Local(_)) {
                        facts.assigned_in_closure = true;
                    }
                }
                _ => {}
            }
        }
        match access {
            Access::Prepared { slot, fallback } => {
                let (name, fallback) = self.prepared_accesses[fallback].clone();
                let tmp = self.temp(at)?;
                self.out().op_u16(Opcode::SetLocal, tmp, at);
                let missing = self.out().label();
                let done = self.out().label();
                let object = self.out().label();
                self.out().op_u16(Opcode::GetLocal, slot, at);
                self.out().branch(Opcode::JumpIfFalse, missing, at);
                self.out().op_u16(Opcode::GetLocal, slot, at);
                self.out().op(Opcode::IsCell, at);
                self.out().branch(Opcode::JumpIfFalse, object, at);
                self.environment_cell(
                    Place::Local(slot),
                    &name,
                    at,
                    self.cur().strict || self.cur().strict_expression,
                )?;
                self.out().op_u16(Opcode::GetLocal, tmp, at);
                self.out().op(Opcode::CellSet, at);
                self.out().op_u16(Opcode::GetLocal, tmp, at);
                self.out().branch(Opcode::Jump, done, at);
                self.out().bind(object);
                self.check_object_binding(Place::Local(slot), &name, at)?;
                self.out().op_u16(Opcode::GetLocal, slot, at);
                self.out().op_u16(Opcode::GetLocal, tmp, at);
                let index = self.name_index(&name, at)?;
                let op = self.strict_op(Opcode::PutField);
                self.out().op_u16(op, index, at);
                self.out().branch(Opcode::Jump, done, at);
                self.out().bind(missing);
                self.out().op_u16(Opcode::GetLocal, tmp, at);
                self.store(fallback, at)?;
                self.out().bind(done);
                self.cur_mut().next_slot -= 1;
            }
            Access::Dynamic(_) => {
                let saved = self.cur().next_slot;
                let prepared = self.prepare_reference(access, at)?;
                self.store(prepared, at)?;
                self.cur_mut().next_slot = saved;
            }
            Access::Intrinsic => {
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    at,
                    format!(
                        "the reserved intrinsic binding `{RESERVED_BINDING}` cannot be assigned"
                    ),
                ));
            }
            Access::Global(global) => {
                self.global_op(self.strict_op(Opcode::PutGlobal), global, at)?
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
                target: Target::SelfFn(SelfKind::Expression),
                ..
            } if !self.cur().strict && !self.cur().strict_expression => {}
            Access::Found {
                target: Target::Binding(BindingKind::Import),
                ..
            } => {
                self.out().op(Opcode::Pop, at);
                self.throw(ThrowKind::ConstAssignment, at);
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

    /// Materialize every visible name through the ordinary capture resolver.
    /// Analysis forces their binding cells even when source code never names
    /// them; emission records how this call reaches each cell. Shadowed names
    /// are resolved once, from the innermost scope, and loop cells are read at
    /// the call so a descriptor always sees the current iteration.
    fn eval_site(&mut self, span: Span) -> Result<EvalSite, SyntaxError> {
        if self.analyzing() && !self.cur().strict {
            let key = self.cur().self_key;
            if self.cur().in_parameters {
                self.fact(key).has_parameter_eval = true;
            } else {
                self.fact(key).has_eval = true;
            }
        }
        let level = self.fns.len() - 1;
        let mut environments = Vec::new();
        for owner in (0..=level).rev() {
            let with_envs = self.fns[owner]
                .with_envs
                .iter()
                .rev()
                .map(|&(_, p, k)| Some((p, k)));
            for (mut place, key) in with_envs
                .chain([self.fns[owner].dynamic_env, self.fns[owner].parameter_env])
                .collect::<Vec<_>>()
                .into_iter()
                .flatten()
            {
                for child in owner + 1..=level {
                    place = self.capture_place(child, place, key);
                }
                environments.push(eval_source(place));
            }
        }
        for (mut place, key) in self.eval_environments.clone() {
            for child in 1..=level {
                place = self.capture_place(child, place, key);
            }
            environments.push(eval_source(place));
        }
        let variable_environment = self
            .cur()
            .dynamic_env
            .or(self.cur().parameter_env)
            .map(|(p, _)| eval_source(p))
            .or_else(|| {
                (level == 0)
                    .then_some(self.eval_variable_environment)
                    .flatten()
                    .map(eval_source)
            });
        let mut lexical_conflicts = if self.cur().completion.is_some() {
            self.eval_lexical_conflicts.clone()
        } else {
            Vec::new()
        };
        let mut variable_bindings = if self.cur().completion.is_some() {
            self.eval_variable_bindings.clone()
        } else {
            Vec::new()
        };
        for (i, scope) in self
            .cur()
            .scopes
            .iter()
            .enumerate()
            .skip(self.cur().var_scope)
        {
            for (name, b) in scope {
                if self.cur().in_parameters {
                    // Parameters and the ordinary function's implicit
                    // arguments binding cannot be redeclared by sloppy eval.
                    lexical_conflicts.push(name.clone());
                } else if i == self.cur().var_scope
                    && matches!(
                        b.kind,
                        BindingKind::Var | BindingKind::Function | BindingKind::Param
                    )
                {
                    if !(b.global || self.sloppy_eval && level == 0) {
                        variable_bindings.push(name.clone());
                    }
                } else if b.kind != BindingKind::Param {
                    lexical_conflicts.push(name.clone());
                }
            }
        }
        variable_bindings.sort();
        variable_bindings.dedup();
        lexical_conflicts.sort();
        lexical_conflicts.dedup();
        variable_bindings.retain(|name| !lexical_conflicts.contains(name));
        let mut names: Vec<String> = self.eval_bindings.keys().cloned().collect();
        for ctx in &self.fns {
            for scope in &ctx.scopes {
                names.extend(scope.keys().cloned());
            }
            if let Some((name, _)) = &ctx.self_name {
                names.push(name.clone());
            }
        }
        names.sort();
        names.dedup();
        let mut bindings = Vec::new();
        for name in names {
            let mut access = self.resolve(&name);
            let mut count = 0;
            while let Access::Dynamic(i) = access {
                count += 1;
                access = self.dynamic_accesses[i].3;
            }
            if let Access::Found {
                place,
                target,
                repr,
                key,
                ..
            } = access
            {
                if self.analyzing() {
                    let facts = self.fact(key);
                    facts.eval_visible = true;
                    facts.assigned = true;
                    if let Target::SelfFn(SelfKind::Declaration { key }) = target {
                        let facts = self.fact(key);
                        facts.eval_visible = true;
                        facts.assigned = true;
                    }
                }
                bindings.push(EvalBinding {
                    name,
                    source: eval_source(place),
                    kind: match target {
                        Target::Binding(BindingKind::Import) => EvalBindingKind::Import,
                        Target::Binding(BindingKind::Const) => EvalBindingKind::Const,
                        Target::SelfFn(_) => EvalBindingKind::FunctionName,
                        _ => EvalBindingKind::Mutable,
                    },
                    cell: repr == Repr::Cell,
                    environments: count,
                });
            }
        }
        let new_target = self.resolve_new_target(self.fns.len() - 1).map(eval_source);
        let super_home = self
            .cur()
            .super_property
            .then(|| eval_source(self.resolve_super_function(self.fns.len() - 1)));
        let super_fields = if self.cur().super_call {
            let fields = self.resolve_instance_fields(self.fns.len() - 1, span)?;
            let Access::Found { place, repr, .. } = fields else {
                return Err(self.internal(span, "invalid constructor field binding"));
            };
            Some((eval_source(place), repr == Repr::Cell))
        } else {
            None
        };
        if bindings.len() + environments.len()
            > MAX_COUNT
                - 1
                - usize::from(new_target.is_some())
                - usize::from(super_home.is_some())
                - usize::from(super_fields.is_some())
        {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "too many direct eval bindings",
            ));
        }
        let receiver = eval_source(self.resolve_this(self.fns.len() - 1, true));
        Ok(EvalSite {
            offset: self.cur().out.code.len() as u32,
            strict: self.cur().strict,
            bindings,
            receiver,
            receiver_strict: self.cur().this_strict,
            class_initializer: self.cur().class_initializer,
            super_home,
            super_fields,
            new_target,
            environments,
            variable_environment,
            variable_bindings,
            lexical_conflicts,
        })
    }

    /// Lower the parameters and body of `f`, the function just entered.
    /// With a simple parameter list, the parameters are bindings of the
    /// body's scope. With a default value (clj-quickjs-100) they have their
    /// own scope around the body's ([`Self::parameter_scope`]), as in
    /// ECMAScript FunctionDeclarationInstantiation and QuickJS
    /// `js_parse_function_decl2` (`has_parameter_expressions`).
    fn function_body(&mut self, f: &Function) -> Result<(), SyntaxError> {
        self.cur_mut().async_generator = f.asynchronous && f.generator;
        if self.cur().async_generator {
            self.out().op(Opcode::AsyncGeneratorStart, f.span);
        } else if f.asynchronous {
            self.out().op(Opcode::AsyncStart, f.span);
        }
        if f.generator && !f.asynchronous {
            self.out().op(Opcode::GeneratorStart, f.span);
        }
        // Collect before NewArguments can release the original actuals, and
        // before defaults can mutate incoming parameter slots. The resulting
        // array remains hidden until its parameter's initialization turn.
        if let Some((index, p)) = f
            .params
            .iter()
            .enumerate()
            .find(|(_, p)| matches!(p.kind, PatternKind::Rest(_)))
        {
            self.out().op_u16(Opcode::Rest, index as u16, p.span);
            self.out().op_u16(Opcode::SetLocal, index as u16, p.span);
        }
        if !f.has_non_simple_parameters() {
            if f.generator {
                self.out().op(Opcode::GeneratorInitialYield, f.span);
            }
            let names: Vec<Ident> = f.params.iter().flat_map(|p| p.names()).cloned().collect();
            return self.statement_list(&f.body, &names, true);
        }
        self.cur_mut().in_parameters = true;
        self.parameter_scope(f)?;
        self.cur_mut().in_parameters = false;
        if f.generator {
            self.out().op(Opcode::GeneratorInitialYield, f.span);
        }
        self.cur_mut().var_scope = 1;
        self.statement_list(&f.body, &[], true)?;
        self.cur_mut().var_scope = 0;
        self.exit_scope(0);
        Ok(())
    }

    /// Enter the parameter scope of `f`, which has a default value, and
    /// initialize its parameters left to right. Each parameter is in its
    /// temporal dead zone until its turn, so a default sees the parameters
    /// before it (and the enclosing scopes and the function's own name), but
    /// not itself, the ones after it or the body's declarations. A default
    /// runs only when its argument is `undefined`:
    ///
    /// ```text
    /// get_local src; undefined; strict_eq; jump_if_false skip
    /// <default>; set_local src
    /// skip:
    /// ```
    ///
    /// `src` is the parameter's own local (the argument), except for a
    /// parameter kept in a cell: its cell is created uninitialized on entry,
    /// before any default runs (a closure in an earlier default may capture
    /// it), after its argument is moved to a scratch local, and initialized
    /// from that local in its turn (`cell_init`), which is then cleared.
    #[inline(never)]
    fn identifier_parameter_scope(&mut self, f: &Function) -> Result<(), SyntaxError> {
        self.cur_mut().scopes.push(Scope::new());
        for (i, p) in f.params.iter().enumerate() {
            let p = p.identifier().unwrap();
            self.declare(p, BindingKind::Param, false, false)?;
            let binding = self
                .cur_mut()
                .scopes
                .last_mut()
                .and_then(|scope| scope.get_mut(&p.name));
            match binding {
                Some(b) if usize::from(b.slot) == i => b.initialized = false,
                _ => return Err(self.internal(p.span, "parameter is not in its argument slot")),
            }
        }
        self.arguments_binding(f.span, false)?;
        self.parameter_eval_environment(f.span)?;
        let parameter_slots = self.cur().next_slot;
        let mut sources = Vec::with_capacity(f.params.len());
        for p in &f.params {
            let p = p.identifier().unwrap();
            let b = self.local(p)?;
            if b.repr == Repr::Cell {
                let stash = self.temp(p.span)?;
                self.out().op_u16(Opcode::GetLocal, b.slot, p.span);
                self.out().op_u16(Opcode::SetLocal, stash, p.span);
                self.out().op(Opcode::NewCell, p.span);
                self.out().op_u16(Opcode::SetLocal, b.slot, p.span);
                sources.push(stash);
            } else {
                sources.push(b.slot);
            }
        }
        for ((p, default), src) in f.params.iter().zip(&f.defaults).zip(sources) {
            let p = p.identifier().unwrap();
            if let Some(default) = default {
                let skip = self.out().label();
                self.out().op_u16(Opcode::GetLocal, src, p.span);
                self.out().op(Opcode::Undefined, p.span);
                self.out().op(Opcode::StrictEq, p.span);
                self.out().branch(Opcode::JumpIfFalse, skip, p.span);
                self.named_expr(default, JsString::from(p.name.as_str()))?;
                self.out().op_u16(Opcode::SetLocal, src, default.span);
                self.out().bind(skip);
            }
            let b = self.initialize(p)?;
            if b.repr == Repr::Cell {
                self.out().op_u16(Opcode::GetLocal, b.slot, p.span);
                self.out().op_u16(Opcode::GetLocal, src, p.span);
                self.out().op(Opcode::CellInit, p.span);
                // The scratch local would otherwise keep the value alive
                // (frames are traced whole) after the parameter changes.
                self.out().op(Opcode::Undefined, p.span);
                self.out().op_u16(Opcode::SetLocal, src, p.span);
            }
        }
        // The scratch locals are free again: the body's bindings reuse them
        // (`init_body_var`).
        self.cur_mut().next_slot = parameter_slots;
        Ok(())
    }

    #[inline(never)]
    fn parameter_scope(&mut self, f: &Function) -> Result<(), SyntaxError> {
        if f.params.iter().all(|p| p.identifier().is_some()) {
            return self.identifier_parameter_scope(f);
        }
        self.cur_mut().scopes.push(Scope::new());
        // Keep incoming argument slots separate from pattern bindings: one
        // argument may initialize many names, and earlier defaults see TDZ.
        self.cur_mut().next_slot = f.params.len();
        for p in &f.params {
            for name in p.names() {
                self.declare(name, BindingKind::Param, false, false)?;
                self.cur_mut()
                    .scopes
                    .last_mut()
                    .unwrap()
                    .get_mut(&name.name)
                    .unwrap()
                    .initialized = false;
                let b = self.local(name)?;
                if b.repr == Repr::Cell {
                    self.out().op(Opcode::NewCell, name.span);
                    self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                }
            }
        }
        self.arguments_binding(f.span, false)?;
        self.parameter_eval_environment(f.span)?;
        let parameter_slots = self.cur().next_slot;
        for (i, (p, default)) in f.params.iter().zip(&f.defaults).enumerate() {
            let src = i as u16;
            if let Some(default) = default {
                let skip = self.out().label();
                self.out().op_u16(Opcode::GetLocal, src, p.span);
                self.out().op(Opcode::Undefined, p.span);
                self.out().op(Opcode::StrictEq, p.span);
                self.out().branch(Opcode::JumpIfFalse, skip, p.span);
                self.pattern_value(default, p)?;
                self.out().op_u16(Opcode::SetLocal, src, default.span);
                self.out().bind(skip);
            }
            self.out().op_u16(Opcode::GetLocal, src, p.span);
            self.pattern_store(p, true, false)?;
            self.out().op(Opcode::Undefined, p.span);
            self.out().op_u16(Opcode::SetLocal, src, p.span);
        }
        // The scratch locals are free again: the body's bindings reuse them
        // (`init_body_var`).
        self.cur_mut().next_slot = parameter_slots;
        Ok(())
    }

    /// Defaults share a variable environment outside the body's var scope.
    /// Allocate it only for direct eval in these defaults; eval in a nested
    /// function or in the body has its own variable environment instead.
    fn parameter_eval_environment(&mut self, span: Span) -> Result<(), SyntaxError> {
        if !self.cur().strict
            && !self.analyzing()
            && self.facts_of(self.cur().self_key).has_parameter_eval
        {
            let slot = self.temp(span)?;
            let key = self.cur().parameter_environment_key;
            self.cur_mut().parameter_env = Some((Place::Local(slot), key));
            self.out().op(Opcode::NewCell, span);
            self.out().op(Opcode::Dup, span);
            self.out().op(Opcode::NewObject, span);
            self.out().op(Opcode::CellInit, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
        Ok(())
    }

    /// Initialize the body `var` binding `b` of a function with a parameter
    /// scope: from the parameter of the same name, if there is one, else to
    /// `undefined` (ECMAScript FunctionDeclarationInstantiation, step 28).
    /// Unlike in other functions, its local may hold a leftover scratch
    /// value of the parameter scope, so even a plain local is set.
    #[inline(never)]
    fn init_body_var(&mut self, name: &Ident, b: Binding) {
        let param = self.cur().scopes[0].get(&name.name).copied();
        let span = name.span;
        if b.repr == Repr::Cell {
            self.out().op(Opcode::NewCell, span);
            self.out().op(Opcode::Dup, span);
        }
        match param {
            Some(p) => {
                if self.analyzing() {
                    self.fact(p.key).copied_to = Some(b.key);
                }
                self.out().op_u16(Opcode::GetLocal, p.slot, span);
                if p.repr == Repr::Cell {
                    self.out().op(Opcode::CellGet, span);
                }
            }
            None => self.out().op(Opcode::Undefined, span),
        }
        if b.repr == Repr::Cell {
            self.out().op(Opcode::CellInit, span);
        }
        self.out().op_u16(Opcode::SetLocal, b.slot, span);
    }

    /// Reject a `let`/`const` at the top of a function body that repeats a
    /// parameter in the parameter scope (an early error; without a parameter
    /// scope the body's scope reports it as a redeclaration).
    fn check_not_parameter(&self, name: &Ident) -> Result<(), SyntaxError> {
        let ctx = self.cur();
        if ctx.var_scope > 0
            && ctx.scopes[0]
                .get(&name.name)
                .is_some_and(|b| b.kind == BindingKind::Param)
        {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                name.span,
                format!("'{}' is already declared as a parameter", name.name),
            ));
        }
        Ok(())
    }

    /// Lower `f` as a child function of the running one and emit the code
    /// that creates its closure (pushing one function object).
    fn closure(
        &mut self,
        f: &Function,
        self_name: Option<(String, SelfKind)>,
        inferred_name: Option<JsString>,
    ) -> Result<(), SyntaxError> {
        self.closure_with_fields(f, self_name, inferred_name, None)
    }

    #[inline(never)]
    fn closure_with_fields(
        &mut self,
        f: &Function,
        self_name: Option<(String, SelfKind)>,
        inferred_name: Option<JsString>,
        fields: Option<&[ClassInitializer<'_>]>,
    ) -> Result<(), SyntaxError> {
        if f.params.len() > MAX_LOCALS {
            return Err(self.error(SyntaxErrorKind::TooLarge, f.span, "too many parameters"));
        }
        let parent_this_strict = self.cur().this_strict;
        let parent_initializer = self.cur().class_initializer;
        let parent_super_property = self.cur().super_property;
        let parent_super_call = self.cur().super_call;
        self.enter_function(self_name, false, f.strict);
        self.cur_mut().constructor_kind = constructor_kind(f);
        // [[SourceText]]: the function's own span, the whole class for a
        // class constructor, none for a synthesized field initializer.
        let class_text = self.class_text.take();
        self.cur_mut().text = if fields.is_some() {
            None
        } else if self.cur().constructor_kind.is_class() {
            class_text
        } else {
            Some(f.span)
        };
        self.cur_mut().simple_parameters = !f.has_non_simple_parameters();
        if self.cur().constructor_kind.is_class() {
            let fields = self.resolve("*instance-fields*");
            self.cur_mut().instance_fields = Some(fields);
        }
        self.cur_mut().own_arguments = matches!(
            f.kind,
            FunctionKind::Ordinary
                | FunctionKind::ClassConstructor
                | FunctionKind::DerivedClassConstructor
                | FunctionKind::DefaultDerivedConstructor
                | FunctionKind::Method
                | FunctionKind::Getter
                | FunctionKind::Setter
        );
        self.cur_mut().lexical_this =
            matches!(f.kind, FunctionKind::Arrow | FunctionKind::ConciseArrow);
        self.cur_mut().class_initializer = fields.is_some()
            || f.kind == FunctionKind::ClassStaticBlock
            || self.cur().lexical_this && parent_initializer;
        self.cur_mut().super_property = if self.cur().lexical_this {
            parent_super_property
        } else {
            !matches!(f.kind, FunctionKind::Ordinary)
        };
        self.cur_mut().super_call = self.cur().constructor_kind.is_derived()
            || self.cur().lexical_this && parent_super_call;
        if fields.is_some() {
            self.cur_mut().own_arguments = false;
        }
        if self.cur().lexical_this {
            self.cur_mut().this_strict = parent_this_strict;
        }
        if f.kind == FunctionKind::ClassConstructor {
            self.out().op(Opcode::GetThis, f.span);
            let fields = self.resolve_instance_fields(self.fns.len() - 1, f.span)?;
            self.load(fields, f.span)?;
            self.out().op_u16(Opcode::CallMethod, 0, f.span);
            self.out().op(Opcode::Pop, f.span);
        }
        if let Some(fields) = fields {
            for initializer in fields {
                let (property, key, is_field) = match initializer {
                    ClassInitializer::Member(property, key, is_field) => (property, key, is_field),
                    ClassInitializer::StaticBlock(block) => {
                        // Method creation leaves the home object as the call's receiver.
                        self.out().op(Opcode::GetThis, block.span);
                        self.closure(block, None, None)?;
                        self.out().op_u16(Opcode::CallMethod, 0, block.span);
                        self.out().op(Opcode::Pop, block.span);
                        continue;
                    }
                };
                let at = property.name_span;
                self.out().op(Opcode::GetThis, at);
                if let PropertyName::Identifier(name) = &property.name
                    && name.starts_with('#')
                {
                    self.private_name(name, at)?;
                    let op = if *is_field {
                        self.named_expr(&property.value, JsString::from(name.as_str()))?;
                        Opcode::DefinePrivateField
                    } else {
                        self.identifier(&key.name, key.span)?;
                        match &property.value.kind {
                            ExprKind::Function(f) if f.kind == FunctionKind::Getter => {
                                Opcode::DefinePrivateGetter
                            }
                            ExprKind::Function(f) if f.kind == FunctionKind::Setter => {
                                Opcode::DefinePrivateSetter
                            }
                            _ => Opcode::DefinePrivateMethod,
                        }
                    };
                    self.out().op(op, at);
                } else {
                    self.computed_property(
                        property,
                        &Expr::new(ExprKind::Identifier(key.name.clone()), at),
                    )?;
                }
                self.out().op(Opcode::Pop, at);
            }
        } else {
            self.function_body(f)?;
        }
        if f.kind == FunctionKind::DefaultDerivedConstructor {
            self.super_call(&[], f.span, true)?;
            self.out().op(Opcode::Return, f.span);
        }
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
            let function_name = f
                .name
                .as_ref()
                .map(|n| JsString::from(n.name.as_str()))
                .or(inferred_name)
                .unwrap_or_else(|| JsString::from(""));
            Some(self.seal(
                ctx,
                name,
                f.params.len() as u16,
                function_length(f),
                function_name,
            )?)
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
        let opcode = if matches!(
            f.kind,
            FunctionKind::Method
                | FunctionKind::Getter
                | FunctionKind::Setter
                | FunctionKind::ClassStaticBlock
        ) {
            Opcode::Method
        } else {
            Opcode::Function
        };
        self.out().op_u16(opcode, index as u16, f.span);
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
            StmtKind::Lexical(decl) => self.declarators(decl)?,
            StmtKind::Var(declarators) => self.var_declarators(declarators)?,
            StmtKind::Return(value) => self.return_statement(value.as_ref(), span)?,
            StmtKind::Block(body) => self.statement_list(body, &[], false)?,
            StmtKind::With { object, body } => self.with_statement(object, body, span)?,
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
                self.push_loop(end, start);
                self.statement(body)?;
                self.cur_mut().exits.pop();
                self.out().branch(Opcode::Jump, start, span);
                self.out().bind(end);
            }
            // start: <body>
            // cont:  <test>; jump_if_false end; jump start
            // end:
            StmtKind::DoWhile { body, test } => {
                self.reset_completion(span);
                let start = self.out().label();
                let cont = self.out().label();
                let end = self.out().label();
                self.out().bind(start);
                self.push_loop(end, cont);
                self.statement(body)?;
                self.cur_mut().exits.pop();
                self.out().bind(cont);
                self.expr(test)?;
                self.out().branch(Opcode::JumpIfFalse, end, test.span);
                self.out().branch(Opcode::Jump, start, span);
                self.out().bind(end);
            }
            StmtKind::For {
                init,
                test,
                update,
                body,
            } => self.for_statement(init.as_ref(), test.as_ref(), update.as_ref(), body, span)?,
            StmtKind::ForIn { head, object, body } => {
                self.for_in_statement(head, object, body, span)?
            }
            StmtKind::ForOf {
                asynchronous,
                head,
                object,
                body,
            } => self.for_of_statement(head, object, body, span, *asynchronous)?,
            StmtKind::Throw(e) => {
                self.expr(e)?;
                self.out().op(Opcode::Throw, span);
            }
            StmtKind::Try {
                block,
                handler,
                finalizer,
            } => self.try_statement(block, handler.as_ref(), finalizer.as_deref(), span)?,
            StmtKind::Break(label) => self.jump(true, label.as_ref(), span)?,
            StmtKind::Continue(label) => self.jump(false, label.as_ref(), span)?,
            StmtKind::Labelled { .. } => self.labelled(stmt)?,
            StmtKind::Switch {
                discriminant,
                cases,
            } => self.switch_statement(discriminant, cases, span)?,
            // Created when its statement list was entered (hoisting).
            StmtKind::FunctionDecl(f) => self.annex_b_copy(f)?,
            StmtKind::Empty => {}
        }
        Ok(())
    }

    // Keep return/with scratch values off the shared statement frame. That
    // frame remains live once per nested loop, including for-of's iterator
    // and disposal handlers, which must fit the parser's 2 MiB budget.
    #[inline(never)]
    fn return_statement(&mut self, value: Option<&Expr>, span: Span) -> Result<(), SyntaxError> {
        if self.cur().completion.is_some() {
            // The parser rejects this; kept as a guard.
            return Err(self.error(SyntaxErrorKind::Invalid, span, "return outside a function"));
        }
        match value {
            Some(e) => self.expr(e)?,
            None => self.out().op(Opcode::Undefined, span),
        }
        if self.cur().async_generator && value.is_some() {
            self.out().op(Opcode::Await, span);
        }
        self.out().op(Opcode::Return, span);
        Ok(())
    }

    #[inline(never)]
    fn with_statement(
        &mut self,
        object: &Expr,
        body: &Stmt,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.reset_completion(span);
        let saved = self.cur().next_slot;
        let slot = self.temp(span)?;
        let key = self.next_self_key;
        self.next_self_key -= 1;
        self.expr(object)?;
        self.out().op(Opcode::ToObject, span);
        self.out().op_u16(Opcode::SetLocal, slot, span);
        let depth = self.cur().scopes.len();
        self.cur_mut()
            .with_envs
            .push((depth, Place::Local(slot), key));
        self.statement(body)?;
        self.cur_mut().with_envs.pop();
        self.out().op(Opcode::Undefined, span);
        self.out().op_u16(Opcode::SetLocal, slot, span);
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    fn pattern_value(&mut self, value: &Expr, pattern: &Pattern) -> Result<(), SyntaxError> {
        if let Some(name) = pattern.identifier() {
            self.named_expr(value, JsString::from(name.name.as_str()))
        } else {
            self.expr(value)
        }
    }

    /// Prepare assignment references before stepping the iterator or evaluating
    /// a default. The reference stays on the traced operand stack across calls.
    fn pattern_reference(&mut self, pattern: &Pattern) -> Result<bool, SyntaxError> {
        match &pattern.kind {
            PatternKind::Identifier(name) => {
                let access = self.resolve_reference(&name.name);
                if matches!(access, Access::Dynamic(_)) {
                    self.select_reference(access, name.span)?;
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            PatternKind::Target(target) => {
                // Assignment references retain the unconverted computed key.
                // PutElem performs ToPropertyKey after stepping/defaults, as
                // for a simple property assignment (rather than a read/write).
                if let ExprKind::Index { object, index, .. } = &target.kind
                    && !matches!(object.kind, ExprKind::Super)
                {
                    self.expr(object)?;
                    self.expr(index)?;
                } else {
                    self.property_reference(target, false)?;
                }
                Ok(true)
            }
            PatternKind::Default { target, .. } | PatternKind::Rest(target) => {
                self.pattern_reference(target)
            }
            _ => Ok(false),
        }
    }

    #[inline(never)]
    fn pattern_store(
        &mut self,
        pattern: &Pattern,
        initialize: bool,
        prepared: bool,
    ) -> Result<(), SyntaxError> {
        let span = pattern.span;
        match &pattern.kind {
            PatternKind::Identifier(name) => {
                if initialize {
                    let b = self.initialize(name)?;
                    if b.global_lexical {
                        // No scratch local is needed to initialize the
                        // registered cell, even at the u16 local limit.
                        if b.repr == Repr::Value {
                            self.out().op(Opcode::Dup, span);
                        }
                        let index = self.string_index(JsString::from(name.name.as_str()), span)?;
                        self.out().op_u16(Opcode::InitGlobalLexical, index, span);
                        if b.repr == Repr::Value {
                            self.out().op_u16(Opcode::SetLocal, b.slot, span);
                        }
                    } else if b.repr == Repr::Cell {
                        let saved = self.cur().next_slot;
                        let tmp = self.temp(span)?;
                        self.out().op_u16(Opcode::SetLocal, tmp, span);
                        self.out().op_u16(Opcode::GetLocal, b.slot, span);
                        self.out().op_u16(Opcode::GetLocal, tmp, span);
                        self.out().op(Opcode::CellInit, span);
                        self.out().op(Opcode::Undefined, span);
                        self.out().op_u16(Opcode::SetLocal, tmp, span);
                        self.cur_mut().next_slot = saved;
                    } else {
                        self.out().op_u16(Opcode::SetLocal, b.slot, span);
                    }
                } else {
                    let at = self.name_span(&name.name, name.span);
                    let mut access = self.resolve_reference(&name.name);
                    let saved = self.cur().next_slot;
                    if prepared {
                        let value = self.temp(at)?;
                        let slot = self.temp(at)?;
                        self.out().op_u16(Opcode::SetLocal, value, at);
                        self.out().op_u16(Opcode::SetLocal, slot, at);
                        self.out().op_u16(Opcode::GetLocal, value, at);
                        while let Access::Dynamic(i) = access {
                            access = self.dynamic_accesses[i].3;
                        }
                        let fallback = self.prepared_accesses.len();
                        self.prepared_accesses.push((name.name.clone(), access));
                        access = Access::Prepared { slot, fallback };
                    }
                    self.store(access, at)?;
                    self.out().op(Opcode::Pop, span);
                    self.cur_mut().next_slot = saved;
                }
            }
            PatternKind::Default { target, value } => {
                let skip = self.out().label();
                self.out().op(Opcode::Dup, span);
                self.out().op(Opcode::Undefined, span);
                self.out().op(Opcode::StrictEq, span);
                self.out().branch(Opcode::JumpIfFalse, skip, span);
                self.out().op(Opcode::Pop, span);
                self.pattern_value(value, target)?;
                self.out().bind(skip);
                self.pattern_store(target, initialize, prepared)?;
            }
            PatternKind::Target(target) => {
                if !prepared {
                    return Err(self.internal(span, "unprepared pattern reference"));
                }
                let reference = match &target.kind {
                    ExprKind::Member {
                        object, op_span, ..
                    }
                    | ExprKind::Index {
                        object, op_span, ..
                    } if matches!(object.kind, ExprKind::Super) => PropertyRef::Super(*op_span),
                    ExprKind::Member {
                        property, op_span, ..
                    } if property.name.starts_with('#') => PropertyRef::Private(*op_span),
                    ExprKind::Member {
                        property, op_span, ..
                    } => PropertyRef::Field(
                        self.name_index(&property.name, property.span)?,
                        *op_span,
                    ),
                    ExprKind::Index { op_span, .. } => PropertyRef::Elem(*op_span),
                    _ => return Err(self.internal(span, "invalid pattern reference")),
                };
                self.reference_put(reference);
                self.out().op(Opcode::Pop, span);
            }
            PatternKind::Rest(target) => self.pattern_store(target, initialize, prepared)?,
            PatternKind::Array { elements, rest } => {
                self.array_pattern(elements, rest.as_deref(), initialize, span)?
            }
            PatternKind::Object { properties, rest } => {
                self.object_pattern(properties, rest.as_deref(), initialize, span)?
            }
        }
        Ok(())
    }

    /// Keep source and converted exclusion keys in traced locals. Reads and
    /// writes are ordinary VM operations, so getters and setters can suspend.
    #[inline(never)]
    fn object_pattern(
        &mut self,
        properties: &[(PropertyName, Pattern)],
        rest: Option<&Pattern>,
        initialize: bool,
        span: Span,
    ) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let source = self.temp(span)?;
        self.out().op(Opcode::RequireCoercible, span);
        self.out().op_u16(Opcode::SetLocal, source, span);
        let mut excluded = Vec::new();
        for (name, target) in properties {
            let key_span = match name {
                PropertyName::Computed(_, op_span) => *op_span,
                _ => span,
            };
            let key = self.temp(span)?;
            self.out().op_u16(Opcode::GetLocal, source, span);
            match name {
                PropertyName::Computed(e, _) => self.expr(e)?,
                PropertyName::Identifier(n) => {
                    self.constant(Primitive::String(JsString::from(n.as_str())), span)?
                }
                PropertyName::String(s) => self.constant(Primitive::String(s.clone()), span)?,
                PropertyName::Number(n) => self.constant(
                    Primitive::String(JsString::from(number_to_string(*n).as_str())),
                    span,
                )?,
            }
            self.out().op(Opcode::ToPropertyKey, key_span);
            self.out().op_u16(Opcode::SetLocal, key, span);
            self.out().op(Opcode::Pop, span);
            let prepared = self.pattern_reference(target)?;
            self.out().op_u16(Opcode::GetLocal, source, span);
            self.out().op_u16(Opcode::GetLocal, key, span);
            self.out().op(Opcode::GetElem, key_span);
            self.pattern_store(target, initialize, prepared)?;
            excluded.push(key);
        }
        if let Some(rest) = rest {
            let prepared = self.pattern_reference(rest)?;
            let dest = self.temp(span)?;
            let keys = self.temp(span)?;
            let index = self.temp(span)?;
            let key = self.temp(span)?;
            self.out().op(Opcode::NewObjectLiteral, span);
            self.out().op_u16(Opcode::SetLocal, dest, span);
            self.out().op_u16(Opcode::GetLocal, source, span);
            self.out().op(Opcode::OwnKeys, span);
            self.out().op_u16(Opcode::SetLocal, keys, span);
            self.constant(Primitive::Number(0.0), span)?;
            self.out().op_u16(Opcode::SetLocal, index, span);
            let start = self.out().label();
            let skip = self.out().label();
            let end = self.out().label();
            self.out().bind(start);
            self.out().op_u16(Opcode::GetLocal, index, span);
            self.out().op_u16(Opcode::GetLocal, keys, span);
            self.iterator_field("length", span)?;
            self.out().op(Opcode::Lt, span);
            self.out().branch(Opcode::JumpIfFalse, end, span);
            self.out().op_u16(Opcode::GetLocal, keys, span);
            self.out().op_u16(Opcode::GetLocal, index, span);
            self.out().op(Opcode::GetElem, span);
            self.out().op_u16(Opcode::SetLocal, key, span);
            for excluded in excluded {
                self.out().op_u16(Opcode::GetLocal, key, span);
                self.out().op_u16(Opcode::GetLocal, excluded, span);
                self.out().op(Opcode::StrictEq, span);
                self.out().op(Opcode::Not, span);
                self.out().branch(Opcode::JumpIfFalse, skip, span);
            }
            self.out().op_u16(Opcode::GetLocal, source, span);
            self.out().op_u16(Opcode::GetLocal, key, span);
            self.out().op(Opcode::IsEnumerable, span);
            self.out().branch(Opcode::JumpIfFalse, skip, span);
            self.out().op_u16(Opcode::GetLocal, dest, span);
            self.out().op_u16(Opcode::GetLocal, key, span);
            self.out().op_u16(Opcode::GetLocal, source, span);
            self.out().op_u16(Opcode::GetLocal, key, span);
            self.out().op(Opcode::GetElem, span);
            self.out().op(Opcode::DefineElem, span);
            self.out().op(Opcode::Pop, span);
            self.out().bind(skip);
            self.out().op_u16(Opcode::GetLocal, index, span);
            self.out().op(Opcode::Inc, span);
            self.out().op_u16(Opcode::SetLocal, index, span);
            self.out().branch(Opcode::Jump, start, span);
            self.out().bind(end);
            self.out().op_u16(Opcode::GetLocal, dest, span);
            self.pattern_store(rest, initialize, prepared)?;
        }
        for slot in saved..self.cur().next_slot {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot as u16, span);
        }
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    #[inline(never)]
    fn assign_pattern(
        &mut self,
        target: &Pattern,
        value: &Expr,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.expr(value)?;
        self.out().op(Opcode::Dup, span);
        self.pattern_store(target, false, false)
    }

    /// Consume a value through the same GetIterator/advance/close lowering as
    /// for-of. `done` is set before protocol operations, so failures in next,
    /// done or value do not close. Consumer failures do close, preserving throws.
    #[inline(never)]
    fn array_pattern(
        &mut self,
        elements: &[Option<Pattern>],
        rest: Option<&Pattern>,
        initialize: bool,
        span: Span,
    ) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let it = self.temp(span)?;
        let next = self.temp(span)?;
        let result = self.temp(span)?;
        let done = self.temp(span)?;
        let suppress = self.temp(span)?;
        self.iterator_acquire_value(it, next, span)?;
        self.constant(Primitive::Bool(false), span)?;
        self.out().op_u16(Opcode::SetLocal, done, span);
        let close = self.out().label();
        self.out().branch(Opcode::PushFinally, close, span);
        for element in elements {
            let prepared = match element {
                Some(p) => self.pattern_reference(p)?,
                None => false,
            };
            let step = self.out().label();
            let empty = self.out().label();
            let value = self.out().label();
            self.out().op_u16(Opcode::GetLocal, done, span);
            self.out().branch(Opcode::JumpIfFalse, step, span);
            self.out().branch(Opcode::Jump, empty, span);
            self.out().bind(step);
            self.constant(Primitive::Bool(true), span)?;
            self.out().op_u16(Opcode::SetLocal, done, span);
            self.iterator_advance(it, next, result, empty, span)?;
            if element.is_some() {
                self.out().op_u16(Opcode::GetLocal, result, span);
                self.iterator_field("value", span)?;
            }
            self.constant(Primitive::Bool(false), span)?;
            self.out().op_u16(Opcode::SetLocal, done, span);
            self.out().branch(Opcode::Jump, value, span);
            self.out().bind(empty);
            if element.is_some() {
                self.out().op(Opcode::Undefined, span);
            }
            self.out().bind(value);
            if let Some(p) = element {
                self.pattern_store(p, initialize, prepared)?;
            }
        }
        if let Some(rest) = rest {
            let prepared = self.pattern_reference(rest)?;
            self.out().op(Opcode::NewArray, span);
            let start = self.out().label();
            let collect = self.out().label();
            let end = self.out().label();
            self.out().op_u16(Opcode::GetLocal, done, span);
            self.out().branch(Opcode::JumpIfFalse, collect, span);
            self.out().branch(Opcode::Jump, end, span);
            self.out().bind(collect);
            self.constant(Primitive::Bool(true), span)?;
            self.out().op_u16(Opcode::SetLocal, done, span);
            self.out().bind(start);
            self.iterator_step(it, next, result, end, span)?;
            self.out().op(Opcode::AppendElement, span);
            self.out().branch(Opcode::Jump, start, span);
            self.out().bind(end);
            self.pattern_store(rest, initialize, prepared)?;
        }
        self.out().op(Opcode::PopFinally, span);
        self.out().bind(close);
        let finished = self.out().label();
        let run_close = self.out().label();
        let caught = self.out().label();
        self.out().op_u16(Opcode::GetLocal, done, span);
        self.out().op(Opcode::Not, span);
        self.out().branch(Opcode::JumpIfFalse, finished, span);
        self.out().op(Opcode::PendingThrow, span);
        self.out().op_u16(Opcode::SetLocal, suppress, span);
        self.out().op_u16(Opcode::GetLocal, suppress, span);
        self.out().branch(Opcode::JumpIfFalse, run_close, span);
        self.out().branch(Opcode::PushCatch, caught, span);
        self.out().bind(run_close);
        self.iterator_close(it, result, span)?;
        self.out().op_u16(Opcode::GetLocal, suppress, span);
        self.out().branch(Opcode::JumpIfFalse, finished, span);
        self.out().op(Opcode::PopCatch, span);
        self.out().branch(Opcode::Jump, finished, span);
        self.out().bind(caught);
        self.out().op(Opcode::Pop, span);
        self.out().bind(finished);
        self.out().op(Opcode::EndFinally, span);
        for slot in saved..self.cur().next_slot {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot as u16, span);
        }
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// Initialize the declared bindings of `decl` in order, each from its
    /// initializer (`undefined` without one).
    fn declarators(&mut self, decl: &LexicalDecl) -> Result<(), SyntaxError> {
        for d in &decl.declarators {
            match &d.init {
                Some(init) => self.pattern_value(init, &d.name)?,
                None => self.out().op(Opcode::Undefined, d.span),
            }
            if matches!(decl.kind, LexicalKind::Using | LexicalKind::AwaitUsing) {
                self.register_disposable(d.span)?;
            }
            self.pattern_store(&d.name, true, false)?;
        }
        Ok(())
    }

    /// Snapshot the disposer while retaining the value for binding initialization.
    fn register_disposable(&mut self, span: Span) -> Result<(), SyntaxError> {
        let (_, value, method, hint) = *self
            .cur()
            .using_resources
            .iter()
            .rev()
            .find(|(start, _, _, _)| *start == span.start)
            .ok_or_else(|| self.internal(span, "using resource without a scope"))?;
        self.out().op(Opcode::Dup, span);
        self.out().op_u16(Opcode::SetLocal, value, span);
        self.out().op(Opcode::Dup, span);
        if let Some(hint) = hint {
            self.out().op(Opcode::UsingCheckAsync, span);
            self.out().op_u16(Opcode::SetLocal, hint, span);
        } else {
            self.out().op(Opcode::UsingCheck, span);
        }
        self.out().op_u16(Opcode::SetLocal, method, span);
        Ok(())
    }

    /// Lower `for (init; test; update) body` (clj-quickjs-81):
    ///
    /// ```text
    ///        <init>                (a lexical head: new_cell for its cell
    ///                               bindings, then its declarators)
    ///        <copy>
    /// start: <test>; jump_if_false end
    ///        <body>
    /// cont:  <copy>
    ///        <update>; pop
    ///        jump start
    /// end:
    /// ```
    ///
    /// A lexical head's bindings form a scope around the whole loop, in TDZ
    /// during their own initializers. Each iteration has its own copy of its
    /// `let` bindings (ECMAScript CreatePerIterationEnvironment, before the
    /// first test and before each update; QuickJS closes the loop variables
    /// with `OP_close_loc` before the update): `<copy>` replaces the cell of
    /// every `let` binding kept in a cell with a new cell holding its current
    /// value, so closures created during an iteration keep that iteration's
    /// binding, and the next iteration starts from the values it ended with.
    /// A binding not kept in a cell is either never captured or never
    /// assigned after initialization, so it needs no copy; nor does a
    /// `const`, which cannot change. Every jump stays inside the head's
    /// scope after its declarations: the static TDZ decision holds.
    fn for_statement(
        &mut self,
        init: Option<&ForInit>,
        test: Option<&Expr>,
        update: Option<&Expr>,
        body: &Stmt,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.reset_completion(span);
        let saved_slot = self.cur().next_slot;
        self.cur_mut().scopes.push(Scope::new());
        // (slot, span) of each per-iteration `let` cell.
        let mut copies = Vec::new();
        match init {
            Some(ForInit::Expr(e)) => {
                self.expr(e)?;
                self.out().op(Opcode::Pop, e.span);
            }
            Some(ForInit::Lexical(decl)) => {
                let kind = match decl.kind {
                    LexicalKind::Let => BindingKind::Let,
                    LexicalKind::Const | LexicalKind::Using | LexicalKind::AwaitUsing => {
                        BindingKind::Const
                    }
                };
                for d in &decl.declarators {
                    for name in d.name.names() {
                        self.declare(name, kind, false, false)?;
                    }
                }
                for d in &decl.declarators {
                    for name in d.name.names() {
                        let b = self.local(name)?;
                        if b.repr == Repr::Cell {
                            self.out().op(Opcode::NewCell, name.span);
                            self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                            if kind == BindingKind::Let {
                                copies.push((b.slot, name.span));
                            }
                        }
                    }
                }
                self.declarators(decl)?;
            }
            // `var` bindings are the function's, declared at its entry: one
            // binding for the whole loop, never copied per iteration.
            Some(ForInit::Var(declarators)) => self.var_declarators(declarators)?,
            None => {}
        }
        let start = self.out().label();
        let cont = self.out().label();
        let end = self.out().label();
        self.copy_cells(&copies);
        self.out().bind(start);
        if let Some(test) = test {
            self.expr(test)?;
            self.out().branch(Opcode::JumpIfFalse, end, test.span);
        }
        self.push_loop(end, cont);
        self.statement(body)?;
        self.cur_mut().exits.pop();
        self.out().bind(cont);
        self.copy_cells(&copies);
        if let Some(update) = update {
            self.expr(update)?;
            self.out().op(Opcode::Pop, update.span);
        }
        self.out().branch(Opcode::Jump, start, span);
        self.out().bind(end);
        let ctx = self.cur_mut();
        ctx.scopes.pop();
        ctx.next_slot = saved_slot;
        Ok(())
    }

    #[inline(never)]
    fn check_iteration_pattern(&self, head: &ForInHead) -> Result<(), SyntaxError> {
        if let ForInHead::Pattern(None, target) = head {
            for name in target.names() {
                self.check_var_not_shadowed(name)?;
            }
        }
        Ok(())
    }

    fn declare_iteration_pattern(&mut self, head: &ForInHead) -> Result<Vec<u16>, SyntaxError> {
        let mut cells = Vec::new();
        if let ForInHead::Pattern(Some(kind), target) = head {
            for name in target.names() {
                self.declare(
                    name,
                    if *kind == LexicalKind::Let {
                        BindingKind::Let
                    } else {
                        BindingKind::Const
                    },
                    false,
                    false,
                )?;
                let b = self.local(name)?;
                if b.repr == Repr::Cell {
                    cells.push(b.slot);
                }
            }
            self.renew_iteration_cells(&cells, target.span);
        }
        Ok(cells)
    }

    fn renew_iteration_cells(&mut self, cells: &[u16], span: Span) {
        for &slot in cells {
            self.out().op(Opcode::NewCell, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
    }

    /// Lower `for (head in object) body` (clj-quickjs-85):
    ///
    /// ```text
    ///        <var initializer>; <assign k>; pop  (non-strict Annex B head)
    ///        <object>; for_in_start; set_local it
    ///        push_finally cleanup
    /// start: new_cell; dup; set_local k      (a lexical binding kept in a cell)
    ///        get_local it; for_in_next; jump_if_false done
    ///        <assign the key to the head>
    ///        <body>
    ///        jump start
    /// done:  pop                             (the key; and the fresh cell)
    ///        pop_finally
    /// cleanup: undefined; set_local it; end_finally
    /// end:
    /// ```
    ///
    /// The enumerator (`crate::vm::keys::ForInIterator`) lives in the
    /// scratch local `it` of the running frame, so it is traced with the
    /// frame and survives any yield, wait or migration inside the loop, and
    /// the operand stack is empty at `start` and in the body: `continue`
    /// jumps to `start` with nothing to pop ([`Self::jump`]). The generated
    /// finally clears `it` on exhaustion, exceptions, returns and jumps out
    /// of the loop; only a continue targeting this loop keeps its handler
    /// and enumerator. A break runs cleanup before jumping to `end`.
    ///
    /// A lexical head's binding is declared in a scope around the whole
    /// loop. `object` is evaluated in that scope with the binding in its
    /// temporal dead zone (ECMAScript ForIn/OfHeadEvaluation's TDZ
    /// environment), so `for (let k in k)` throws statically, and a closure
    /// created there captures a cell that stays uninitialized. Each
    /// iteration initializes a fresh binding (ForIn/OfBodyEvaluation): a
    /// binding kept in a cell gets a new cell each time, so closures created
    /// in an iteration keep that iteration's key, and a binding kept in a
    /// local is re-initialized (closures copy it by value only when it is
    /// never assigned). The body follows the initialization, and `continue`
    /// re-enters before it, so the static TDZ decision holds. An identifier
    /// head, or a `var` head (the function's binding, clj-quickjs-89), is
    /// assigned each key exactly as `k = key` would be.
    #[inline(never)]
    fn for_in_statement(
        &mut self,
        head: &ForInHead,
        object: &Expr,
        body: &Stmt,
        span: Span,
    ) -> Result<(), SyntaxError> {
        if let ForInHead::Var(name, _) = head {
            self.check_var_not_shadowed(name)?;
        }
        self.check_iteration_pattern(head)?;
        self.reset_completion(span);
        let saved_slot = self.cur().next_slot;
        self.cur_mut().scopes.push(Scope::new());
        let pattern_cells = self.declare_iteration_pattern(head)?;
        let mut cell = None;
        if let ForInHead::Var(name, Some(init)) = head {
            self.assign(name, init)?;
            self.out().op(Opcode::Pop, name.span);
        }
        if let ForInHead::Lexical(kind, name) = head {
            let kind = match kind {
                LexicalKind::Let => BindingKind::Let,
                LexicalKind::Const | LexicalKind::Using | LexicalKind::AwaitUsing => {
                    BindingKind::Const
                }
            };
            self.declare(name, kind, false, false)?;
            let b = self.local(name)?;
            if b.repr == Repr::Cell {
                self.out().op(Opcode::NewCell, name.span);
                self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                cell = Some(b.slot);
            }
        }
        self.expr(object)?;
        let it = self.temp(object.span)?;
        self.out().op(Opcode::ForInStart, object.span);
        self.out().op_u16(Opcode::SetLocal, it, object.span);
        let start = self.out().label();
        let done = self.out().label();
        let cleanup = self.out().label();
        let end = self.out().label();
        self.out().branch(Opcode::PushFinally, cleanup, span);
        self.out().bind(start);
        self.renew_iteration_cells(&pattern_cells, span);
        if let Some(slot) = cell {
            let out = self.out();
            out.op(Opcode::NewCell, span);
            out.op(Opcode::Dup, span);
            out.op_u16(Opcode::SetLocal, slot, span);
        }
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.out().op(Opcode::ForInNext, span);
        self.out().branch(Opcode::JumpIfFalse, done, span);
        match head {
            ForInHead::Pattern(kind, target) => {
                self.pattern_store(target, kind.is_some(), false)?
            }
            ForInHead::Assignment(target) => self.pattern_store(target, false, false)?,
            ForInHead::Lexical(_, name) => {
                let b = self.initialize(name)?;
                match b.repr {
                    Repr::Cell => self.out().op(Opcode::CellInit, name.span),
                    Repr::Value => self.out().op_u16(Opcode::SetLocal, b.slot, name.span),
                }
            }
            ForInHead::Binding(name) => {
                let at = self.name_span(&name.name, name.span);
                let access = self.resolve_reference(&name.name);
                self.store(access, at)?;
                self.out().op(Opcode::Pop, name.span);
            }
            // The function's `var` binding, declared at its entry: one
            // binding for the whole loop, assigned each key.
            ForInHead::Var(name, _) => {
                let access = self.resolve(&name.name);
                self.store(access, name.span)?;
                self.out().op(Opcode::Pop, name.span);
            }
            ForInHead::Member(target) => self.for_member_store(target)?,
        }
        let labels = std::mem::take(&mut self.cur_mut().loop_labels);
        self.cur_mut().exits.push(Exit::IteratorLoop {
            brk: end,
            cont: start,
            labels,
        });
        self.statement(body)?;
        self.cur_mut().exits.pop();
        self.out().branch(Opcode::Jump, start, span);
        self.out().bind(done);
        self.out().op(Opcode::Pop, span);
        if cell.is_some() {
            self.out().op(Opcode::Pop, span);
        }
        self.out().op(Opcode::PopFinally, span);
        self.out().bind(cleanup);
        self.out().op(Opcode::Undefined, span);
        self.out().op_u16(Opcode::SetLocal, it, span);
        self.out().op(Opcode::EndFinally, span);
        self.out().bind(end);
        let ctx = self.cur_mut();
        ctx.scopes.pop();
        ctx.next_slot = saved_slot;
        Ok(())
    }

    /// Consume the iteration key after evaluating the reference anew. The
    /// saved key and the reference remain traced VM state across any calls,
    /// yields or waits in the object, computed key, conversion or setter.
    #[inline(never)]
    fn for_member_store(&mut self, target: &Expr) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let key = self.temp(target.span)?;
        self.out().op_u16(Opcode::SetLocal, key, target.span);
        let reference = self.property_reference(target, false)?;
        self.out().op_u16(Opcode::GetLocal, key, target.span);
        self.reference_put(reference);
        self.out().op(Opcode::Pop, target.span);
        self.out().op(Opcode::Undefined, target.span);
        self.out().op_u16(Opcode::SetLocal, key, target.span);
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// GetIterator and IteratorClose use ordinary resumable calls. The
    /// finally handler protects the whole loop, while `in_body` distinguishes
    /// consumer failures from failures in next/done/value (which do not close).
    /// All protocol values are hidden frame locals, hence traced task state.
    #[inline(never)]
    fn for_of_statement(
        &mut self,
        head: &ForInHead,
        object: &Expr,
        body: &Stmt,
        span: Span,
        asynchronous: bool,
    ) -> Result<(), SyntaxError> {
        if let ForInHead::Var(name, _) = head {
            self.check_var_not_shadowed(name)?;
        }
        self.check_iteration_pattern(head)?;
        self.reset_completion(span);
        let saved_slot = self.cur().next_slot;
        self.cur_mut().scopes.push(Scope::new());
        let pattern_cells = self.declare_iteration_pattern(head)?;
        let mut cell = None;
        if let ForInHead::Lexical(kind, name) = head {
            self.declare(
                name,
                if *kind == LexicalKind::Let {
                    BindingKind::Let
                } else {
                    BindingKind::Const
                },
                false,
                false,
            )?;
            let b = self.local(name)?;
            if b.repr == Repr::Cell {
                self.out().op(Opcode::NewCell, name.span);
                self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                cell = Some(b.slot);
            }
        }
        let it = self.temp(span)?;
        let next = self.temp(span)?;
        let result = self.temp(span)?;
        let in_body = self.temp(span)?;
        let suppress = self.temp(span)?;
        // A separate outer handler releases protocol roots even when acquisition,
        // next/done/value, or the close handler throws or returns from the frame.
        let cleanup = self.out().label();
        self.out().branch(Opcode::PushFinally, cleanup, span);
        self.cur_mut().exits.push(Exit::Finally);
        self.expr(object)?;
        self.iterator_acquire_value_mode(it, next, span, asynchronous)?;
        let start = self.out().label();
        let exhausted = self.out().label();
        let close = self.out().label();
        let end = self.out().label();
        self.out().branch(Opcode::PushFinally, close, span);
        self.out().bind(start);
        self.constant(Primitive::Bool(false), span)?;
        self.out().op_u16(Opcode::SetLocal, in_body, span);
        self.iterator_advance_mode(it, next, result, exhausted, span, asynchronous)?;
        self.out().op_u16(Opcode::GetLocal, result, span);
        self.iterator_field("value", span)?;
        self.out().op_u16(Opcode::SetLocal, result, span);
        self.constant(Primitive::Bool(true), span)?;
        self.out().op_u16(Opcode::SetLocal, in_body, span);
        let labels = std::mem::take(&mut self.cur_mut().loop_labels);
        self.cur_mut().exits.push(Exit::IteratorLoop {
            brk: end,
            cont: start,
            labels,
        });
        let resources = match head {
            ForInHead::Lexical(LexicalKind::Using, name) => vec![(name.span, false)],
            ForInHead::Lexical(LexicalKind::AwaitUsing, name) => vec![(name.span, true)],
            _ => Vec::new(),
        };
        // The per-iteration disposal handler is inside IteratorClose, and
        // outside the body's scopes. A continue unwinds only disposal; other
        // exits dispose before closing the iterator. Validation failures are
        // already consumer failures (`in_body` is true).
        self.disposable_scope(&resources, span, |this| {
            this.for_of_binding(head, &pattern_cells, cell, result, span)?;
            this.statement(body)
        })?;
        self.cur_mut().exits.pop();
        self.out().branch(Opcode::Jump, start, span);
        self.out().bind(exhausted);
        self.out().op(Opcode::PopFinally, span);
        self.out().op(Opcode::EndFinally, span);
        self.out().branch(Opcode::Jump, end, span);
        self.out().bind(close);
        self.out().op(Opcode::PendingThrow, span);
        self.out().op_u16(Opcode::SetLocal, suppress, span);
        let run_close = self.out().label();
        let finished = self.out().label();
        self.out().op_u16(Opcode::GetLocal, suppress, span);
        self.out().branch(Opcode::JumpIfFalse, run_close, span);
        self.out().op_u16(Opcode::GetLocal, in_body, span);
        self.out().branch(Opcode::JumpIfFalse, finished, span);
        let caught = self.out().label();
        self.out().branch(Opcode::PushCatch, caught, span);
        self.out().bind(run_close);
        self.iterator_close_mode(it, result, span, asynchronous)?;
        self.out().op_u16(Opcode::GetLocal, suppress, span);
        self.out().branch(Opcode::JumpIfFalse, finished, span);
        self.out().op(Opcode::PopCatch, span);
        self.out().branch(Opcode::Jump, finished, span);
        self.out().bind(caught);
        self.out().op(Opcode::Pop, span);
        self.out().bind(finished);
        self.out().op(Opcode::EndFinally, span);
        self.out().bind(end);
        self.out().op(Opcode::PopFinally, span);
        self.out().bind(cleanup);
        for slot in [it, next, result, in_body, suppress] {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
        self.out().op(Opcode::EndFinally, span);
        self.cur_mut().exits.pop();
        self.cur_mut().scopes.pop();
        self.cur_mut().next_slot = saved_slot;
        Ok(())
    }

    /// Keep binding scratch state off the recursive loop-body compiler stack.
    #[inline(never)]
    fn for_of_binding(
        &mut self,
        head: &ForInHead,
        pattern_cells: &[u16],
        cell: Option<u16>,
        result: u16,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.renew_iteration_cells(pattern_cells, span);
        if let Some(slot) = cell {
            self.out().op(Opcode::NewCell, span);
            self.out().op(Opcode::Dup, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
        self.out().op_u16(Opcode::GetLocal, result, span);
        match head {
            ForInHead::Pattern(kind, target) => {
                self.pattern_store(target, kind.is_some(), false)?
            }
            ForInHead::Assignment(target) => self.pattern_store(target, false, false)?,
            ForInHead::Lexical(kind, name) => {
                if matches!(kind, LexicalKind::Using | LexicalKind::AwaitUsing) {
                    self.register_disposable(name.span)?;
                }
                let b = self.initialize(name)?;
                match b.repr {
                    Repr::Cell => self.out().op(Opcode::CellInit, name.span),
                    Repr::Value => self.out().op_u16(Opcode::SetLocal, b.slot, name.span),
                }
            }
            ForInHead::Binding(name) | ForInHead::Var(name, _) => {
                let access = self.resolve_reference(&name.name);
                self.store(access, name.span)?;
                self.out().op(Opcode::Pop, name.span);
            }
            ForInHead::Member(target) => self.for_member_store(target)?,
        }
        Ok(())
    }

    #[inline(never)]
    fn iterator_field(&mut self, name: &str, span: Span) -> Result<(), SyntaxError> {
        let key = self.string_index(JsString::from(name), span)?;
        self.out().op_u16(Opcode::GetField, key, span);
        Ok(())
    }

    /// Shared protocol lowering for for-of, spread and yield*. Acquisition itself
    /// runs the existing resumable native iterator state machine.
    #[inline(never)]
    fn iterator_acquire(
        &mut self,
        source: &Expr,
        it: u16,
        next: u16,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.expr(source)?;
        self.iterator_acquire_value(it, next, span)
    }

    fn iterator_acquire_value(
        &mut self,
        it: u16,
        next: u16,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.iterator_acquire_value_mode(it, next, span, false)
    }

    fn iterator_acquire_value_mode(
        &mut self,
        it: u16,
        next: u16,
        span: Span,
        asynchronous: bool,
    ) -> Result<(), SyntaxError> {
        self.out().op(
            if asynchronous {
                Opcode::GetAsyncIterator
            } else {
                Opcode::GetIterator
            },
            span,
        );
        self.out().op(Opcode::Dup, span);
        self.iterator_field("next", span)?;
        self.out().op_u16(Opcode::SetLocal, next, span);
        self.iterator_field("iterator", span)?;
        self.out().op_u16(Opcode::SetLocal, it, span);
        Ok(())
    }

    /// Test a saved completion kind, leaving only the boolean on the stack.
    fn resume_kind_is(&mut self, slot: u16, kind: u8, span: Span) -> Result<(), SyntaxError> {
        self.out().op_u16(Opcode::GetLocal, slot, span);
        self.constant(Primitive::Number(kind as f64), span)?;
        self.out().op(Opcode::StrictEq, span);
        Ok(())
    }

    /// GetMethod's null/undefined case. Calls validate all other values.
    fn branch_nullish(&mut self, slot: u16, target: usize, span: Span) -> Result<(), SyntaxError> {
        for nullish in [Primitive::Undefined, Primitive::Null] {
            self.out().op_u16(Opcode::GetLocal, slot, span);
            self.constant(nullish, span)?;
            self.out().op(Opcode::StrictEq, span);
            self.out().op(Opcode::Not, span);
            self.out().branch(Opcode::JumpIfFalse, target, span);
        }
        Ok(())
    }

    /// Delegate with cached protocol values in traced frame locals. All
    /// observable protocol operations run as resumable bytecode calls.
    #[inline(never)]
    fn yield_star(&mut self, operand: &Expr, span: Span) -> Result<(), SyntaxError> {
        // Compile nested operands without retaining the protocol emitter's
        // large scratch frame on the compiler's bounded Rust stack.
        self.expr(operand)?;
        self.yield_star_protocol(span)
    }

    #[inline(never)]
    fn yield_star_protocol(&mut self, span: Span) -> Result<(), SyntaxError> {
        let asynchronous = self.cur().async_generator;
        let saved_slot = self.cur().next_slot;
        let it = self.temp(span)?;
        let next = self.temp(span)?;
        let result = self.temp(span)?;
        let received = self.temp(span)?;
        let kind = self.temp(span)?;
        let method = self.temp(span)?;
        if asynchronous {
            self.out().op(Opcode::GetAsyncIterator, span);
        } else {
            self.out().op(Opcode::GetIterator, span);
        }
        self.out().op(Opcode::Dup, span);
        self.iterator_field("next", span)?;
        self.out().op_u16(Opcode::SetLocal, next, span);
        self.iterator_field("iterator", span)?;
        self.out().op_u16(Opcode::SetLocal, it, span);
        self.out().op(Opcode::Undefined, span);
        self.out().op_u16(Opcode::SetLocal, received, span);
        self.constant(Primitive::Number(0.0), span)?;
        self.out().op_u16(Opcode::SetLocal, kind, span);
        let start = self.out().label();
        let done = self.out().label();
        let abrupt = self.out().label();
        let throwing = self.out().label();
        let missing_return = self.out().label();
        let missing_throw = self.out().label();
        let call = self.out().label();
        let end = self.out().label();
        self.out().bind(start);
        self.resume_kind_is(kind, 0, span)?;
        self.out().branch(Opcode::JumpIfFalse, abrupt, span);
        self.out().op_u16(Opcode::GetLocal, next, span);
        self.out().op_u16(Opcode::SetLocal, method, span);
        self.out().branch(Opcode::Jump, call, span);
        self.out().bind(abrupt);
        self.resume_kind_is(kind, 1, span)?;
        self.out().branch(Opcode::JumpIfFalse, throwing, span);
        if asynchronous {
            // AsyncGeneratorUnwrapYieldResumption turns a rejected return
            // argument into a throw completion consumed by yield* itself.
            let rejected = self.out().label();
            let unwrapped = self.out().label();
            self.out().branch(Opcode::PushCatch, rejected, span);
            self.out().op_u16(Opcode::GetLocal, received, span);
            self.out().op(Opcode::Await, span);
            self.out().op(Opcode::PopCatch, span);
            self.out().op_u16(Opcode::SetLocal, received, span);
            self.out().branch(Opcode::Jump, unwrapped, span);
            self.out().bind(rejected);
            self.out().op_u16(Opcode::SetLocal, received, span);
            self.constant(Primitive::Number(2.0), span)?;
            self.out().op_u16(Opcode::SetLocal, kind, span);
            self.out().branch(Opcode::Jump, throwing, span);
            self.out().bind(unwrapped);
        }
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.iterator_field("return", span)?;
        self.out().op_u16(Opcode::SetLocal, method, span);
        self.branch_nullish(method, missing_return, span)?;
        self.out().branch(Opcode::Jump, call, span);
        self.out().bind(missing_return);
        self.out().op_u16(Opcode::GetLocal, received, span);
        if asynchronous {
            self.out().op(Opcode::Await, span);
        }
        self.out().op(Opcode::Return, span);
        self.out().bind(throwing);
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.iterator_field("throw", span)?;
        self.out().op_u16(Opcode::SetLocal, method, span);
        self.branch_nullish(method, missing_throw, span)?;
        self.out().branch(Opcode::Jump, call, span);
        self.out().bind(missing_throw);
        // IteratorClose uses no argument and validates its result, but does
        // not inspect done/value. A close failure replaces the TypeError.
        self.iterator_close_mode(it, method, span, asynchronous)?;
        self.throw(ThrowKind::YieldStarMissingThrow, span);
        // ThrowError has an expression result in the verifier.
        self.out().branch(Opcode::Jump, end, span);
        self.out().bind(call);
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.out().op_u16(Opcode::GetLocal, method, span);
        self.out().op_u16(Opcode::GetLocal, received, span);
        self.out().op_u16(Opcode::CallMethod, 1, span);
        if asynchronous {
            self.out().op(Opcode::Await, span);
        }
        self.out().op(Opcode::RequireObject, span);
        self.out().op_u16(Opcode::SetLocal, result, span);
        self.out().op_u16(Opcode::GetLocal, result, span);
        self.iterator_field("done", span)?;
        self.out().op(Opcode::Not, span);
        self.out().branch(Opcode::JumpIfFalse, done, span);
        self.out().op_u16(Opcode::GetLocal, result, span);
        if asynchronous {
            self.iterator_field("value", span)?;
            self.out().op(Opcode::GeneratorYield, span);
        } else {
            self.out().op(Opcode::GeneratorYieldStar, span);
        }
        self.out().op_u16(Opcode::SetLocal, kind, span);
        self.out().op_u16(Opcode::SetLocal, received, span);
        self.out().branch(Opcode::Jump, start, span);
        self.out().bind(done);
        self.out().op_u16(Opcode::GetLocal, result, span);
        self.iterator_field("value", span)?;
        self.resume_kind_is(kind, 1, span)?;
        self.out().branch(Opcode::JumpIfFalse, end, span);
        // A delegate's completed return is already a return completion;
        // unlike an explicit return expression, its value is kept intact.
        self.out().op(Opcode::Return, span);
        self.out().bind(end);
        for slot in [it, next, result, received, kind, method] {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
        self.cur_mut().next_slot = saved_slot;
        Ok(())
    }

    /// Branch on exhaustion, otherwise leave the next value on the stack.
    /// Every callback and getter remains an ordinary resumable VM operation.
    #[inline(never)]
    fn iterator_step(
        &mut self,
        it: u16,
        next: u16,
        result: u16,
        done: usize,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.iterator_advance(it, next, result, done, span)?;
        self.out().op_u16(Opcode::GetLocal, result, span);
        self.iterator_field("value", span)
    }

    fn iterator_advance(
        &mut self,
        it: u16,
        next: u16,
        result: u16,
        done: usize,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.iterator_advance_mode(it, next, result, done, span, false)
    }

    fn iterator_advance_mode(
        &mut self,
        it: u16,
        next: u16,
        result: u16,
        done: usize,
        span: Span,
        asynchronous: bool,
    ) -> Result<(), SyntaxError> {
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.out().op_u16(Opcode::GetLocal, next, span);
        self.out().op_u16(Opcode::CallMethod, 0, span);
        if asynchronous {
            self.out().op(Opcode::Await, span);
        }
        self.out().op(Opcode::RequireObject, span);
        self.out().op_u16(Opcode::SetLocal, result, span);
        self.out().op_u16(Opcode::GetLocal, result, span);
        self.iterator_field("done", span)?;
        self.out().op(Opcode::Not, span);
        self.out().branch(Opcode::JumpIfFalse, done, span);
        Ok(())
    }

    #[inline(never)]
    fn iterator_close(&mut self, it: u16, method: u16, span: Span) -> Result<(), SyntaxError> {
        self.iterator_close_mode(it, method, span, false)
    }

    fn iterator_close_mode(
        &mut self,
        it: u16,
        method: u16,
        span: Span,
        asynchronous: bool,
    ) -> Result<(), SyntaxError> {
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.iterator_field("return", span)?;
        self.out().op_u16(Opcode::SetLocal, method, span);
        let done = self.out().label();
        for nullish in [Primitive::Undefined, Primitive::Null] {
            self.out().op_u16(Opcode::GetLocal, method, span);
            self.constant(nullish, span)?;
            self.out().op(Opcode::StrictEq, span);
            self.out().op(Opcode::Not, span);
            self.out().branch(Opcode::JumpIfFalse, done, span);
        }
        self.out().op_u16(Opcode::GetLocal, it, span);
        self.out().op_u16(Opcode::GetLocal, method, span);
        self.out().op_u16(Opcode::CallMethod, 0, span);
        if asynchronous {
            self.out().op(Opcode::Await, span);
        }
        self.out().op(Opcode::RequireObject, span);
        self.out().op(Opcode::Pop, span);
        self.out().bind(done);
        Ok(())
    }

    /// Replace each cell local in `copies` with a new cell initialized to
    /// the old cell's value (always initialized here).
    fn copy_cells(&mut self, copies: &[(u16, Span)]) {
        for &(slot, span) in copies {
            let out = self.out();
            out.op(Opcode::NewCell, span);
            out.op(Opcode::Dup, span);
            out.op_u16(Opcode::GetLocal, slot, span);
            out.op(Opcode::CellGet, span);
            out.op(Opcode::CellInit, span);
            out.op_u16(Opcode::SetLocal, slot, span);
        }
    }

    /// Push the exit of a loop whose `break` target is `brk` and `continue`
    /// target `cont`, with the label set a [`Self::labelled`] around it left
    /// in `loop_labels`.
    fn push_loop(&mut self, brk: usize, cont: usize) {
        let ctx = self.cur_mut();
        let labels = std::mem::take(&mut ctx.loop_labels);
        ctx.exits.push(Exit::Loop { brk, cont, labels });
    }

    /// Lower `switch (discriminant) { cases }` (clj-quickjs-83):
    ///
    /// ```text
    ///        <discriminant>; set_local d
    ///        <case block scope entry: new_cell, hoisted functions>
    ///        get_local d; <test 1>; strict_eq; not; jump_if_false C1
    ///        ...                                    (each `case`, in order)
    ///        jump D                                 (`default`, else end)
    /// C1:    <clause 1>
    /// ...    (fall through from clause to clause)
    /// end:
    /// ```
    ///
    /// The discriminant is evaluated once and kept in a scratch local, not
    /// on the operand stack, so a clause body starts at an empty stack like
    /// any statement and jumps out of it drop nothing. ECMAScript tests the
    /// clauses before `default`, then those after it, then takes `default`
    /// (CaseBlockEvaluation): testing them all in source order first is the
    /// same, since `default` has no test, and a test after the first match
    /// is never evaluated. The tests run inside the case block's scope
    /// (its function declarations are created first, its `let`/`const`
    /// bindings are all uninitialized there).
    ///
    /// Control enters the case block at a clause, possibly past a
    /// declaration of an earlier clause, so the `let`/`const` bindings of
    /// every clause before the last non-empty one live in cells, whose
    /// reads and writes check initialization at run time
    /// ([`Self::enter_scope`]); a reference to one in an earlier clause or
    /// a test always runs before its declaration and still throws
    /// statically. The bindings of the last non-empty clause are only
    /// reachable after their declaration in that clause.
    ///
    /// An unlabelled `break` jumps to `end` ([`Exit::Switch`]). The
    /// completion value is reset to `undefined` at entry, then set by the
    /// clause bodies (ECMAScript `UpdateEmpty(R, undefined)`).
    fn switch_statement(
        &mut self,
        discriminant: &Expr,
        cases: &[SwitchCase],
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.reset_completion(span);
        self.expr(discriminant)?;
        let saved_slot = self.cur().next_slot;
        let value = self.temp(discriminant.span)?;
        self.out()
            .op_u16(Opcode::SetLocal, value, discriminant.span);
        let clauses: Vec<&[Stmt]> = cases.iter().map(|c| c.body.as_slice()).collect();
        let checked = clauses.iter().rposition(|b| !b.is_empty()).unwrap_or(0);
        let scope_slot = self.enter_scope(&clauses, &[], false, checked)?;
        let starts: Vec<usize> = cases.iter().map(|_| self.out().label()).collect();
        let end = self.out().label();
        let mut default = end;
        for (case, &start) in cases.iter().zip(&starts) {
            match &case.test {
                Some(test) => {
                    self.out().op_u16(Opcode::GetLocal, value, test.span);
                    self.expr(test)?;
                    self.out().op(Opcode::StrictEq, test.span);
                    self.out().op(Opcode::Not, test.span);
                    self.out().branch(Opcode::JumpIfFalse, start, test.span);
                }
                None => default = start,
            }
        }
        self.out().branch(Opcode::Jump, default, span);
        self.cur_mut().exits.push(Exit::Switch { brk: end });
        for (case, &start) in cases.iter().zip(&starts) {
            self.out().bind(start);
            for stmt in &case.body {
                self.statement(stmt)?;
            }
        }
        self.cur_mut().exits.pop();
        self.out().bind(end);
        self.exit_scope(scope_slot);
        self.cur_mut().next_slot = saved_slot;
        Ok(())
    }

    /// Lower the labelled statement `stmt` with its whole label set
    /// `L1: L2: ... body` (clj-quickjs-82). A loop body takes the label set
    /// as its own (so `continue L1` continues it); any other body is wrapped
    /// in an [`Exit::Labelled`] whose `break` target follows it. A labelled
    /// `break` only leaves scopes, like an unlabelled one, and the body's
    /// completion value is the statement's (ECMAScript `LabelledEvaluation`
    /// turns `break L` into a normal completion; the value the body produced
    /// so far is already stored, as for `UpdateEmpty`).
    fn labelled(&mut self, stmt: &Stmt) -> Result<(), SyntaxError> {
        let mut labels = Vec::new();
        let mut body = stmt;
        while let StmtKind::Labelled { label, body: inner } = &body.kind {
            labels.push(label.name.clone());
            body = inner;
        }
        if matches!(
            body.kind,
            StmtKind::While { .. }
                | StmtKind::DoWhile { .. }
                | StmtKind::For { .. }
                | StmtKind::ForIn { .. }
                | StmtKind::ForOf { .. }
        ) {
            self.cur_mut().loop_labels = labels;
            return self.statement(body);
        }
        let brk = self.out().label();
        self.cur_mut().exits.push(Exit::Labelled { labels, brk });
        self.statement(body)?;
        self.cur_mut().exits.pop();
        self.out().bind(brk);
        Ok(())
    }

    /// Lower a `break` (`is_break`) or `continue` at `span`, to `label` or
    /// unlabelled: leave every `try` construct between it and its target,
    /// innermost first, so the handler stack is the target's again, then
    /// jump past the target or to its `continue` target (as QuickJS's
    /// `emit_break` walks its `BlockEnv` chain). The target is the innermost
    /// loop when unlabelled, else the innermost loop or labelled statement
    /// with that label (clj-quickjs-82). A `try` block with a catch handler
    /// pops it (`pop_catch`); a `try` block or catch body with a finally
    /// handler runs the finally block, which comes back to the next
    /// instruction (`call_finally`); a finally block discards its pending
    /// completion (`drop_completion`) (clj-quickjs-79,
    /// `crate::vm::exception`). Statements leave nothing on the operand
    /// stack, so no operand needs dropping and `call_finally` runs at its
    /// handler's height.
    fn jump(
        &mut self,
        is_break: bool,
        label: Option<&Ident>,
        span: Span,
    ) -> Result<(), SyntaxError> {
        let keyword = if is_break { "break" } else { "continue" };
        let at = Span::new(span.start, span.start + keyword.len());
        let named = |labels: &[String]| label.is_some_and(|l| labels.contains(&l.name));
        for i in (0..self.cur().exits.len()).rev() {
            let (op, target) = match &self.cur().exits[i] {
                Exit::Catch => (Some(Opcode::PopCatch), None),
                Exit::Finally => (Some(Opcode::CallFinally), None),
                Exit::FinallyBody => (Some(Opcode::DropCompletion), None),
                Exit::Loop { brk, cont, labels } if label.is_none() || named(labels) => {
                    (None, Some(if is_break { *brk } else { *cont }))
                }
                Exit::IteratorLoop { brk, cont, labels } if label.is_none() || named(labels) => (
                    is_break.then_some(Opcode::CallFinally),
                    Some(if is_break { *brk } else { *cont }),
                ),
                Exit::IteratorLoop { .. } => (Some(Opcode::CallFinally), None),
                Exit::Labelled { labels, brk } if is_break && named(labels) => (None, Some(*brk)),
                Exit::Switch { brk } if is_break && label.is_none() => (None, Some(*brk)),
                Exit::Loop { .. } | Exit::Labelled { .. } | Exit::Switch { .. } => (None, None),
            };
            if let Some(op) = op {
                self.out().op(op, at);
            }
            if let Some(target) = target {
                self.out().branch(Opcode::Jump, target, at);
                return Ok(());
            }
        }
        // The parser rejects this; kept as a guard.
        Err(self.error(
            SyntaxErrorKind::Invalid,
            at,
            format!("`{keyword}` has no target in the same function"),
        ))
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
    /// finally block runs with the value saved in a scratch local and the
    /// completion value reset to `undefined` (what a `break`/`continue` out
    /// of it carries if it produced no value).
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
        // A `break`/`continue` inside sees the finally handler below it in
        // the block and catch body, and the pending completion in the
        // finally body.
        if finally.is_some() {
            self.cur_mut().exits.push(Exit::Finally);
        }
        match handler {
            Some(handler) => {
                let catch = self.out().label();
                let end = self.out().label();
                self.out().branch(Opcode::PushCatch, catch, span);
                self.cur_mut().exits.push(Exit::Catch);
                self.statement_list(block, &[], false)?;
                self.cur_mut().exits.pop();
                self.out().op(Opcode::PopCatch, span);
                self.out().branch(Opcode::Jump, end, span);
                self.out().bind(catch);
                self.reset_completion(handler.span);
                match &handler.param {
                    Some(pattern) if pattern.identifier().is_none() => {
                        let saved = self.cur().next_slot;
                        let source = self.temp(pattern.span)?;
                        self.out().op_u16(Opcode::SetLocal, source, pattern.span);
                        self.cur_mut().scopes.push(Scope::new());
                        for name in pattern.names() {
                            self.declare(name, BindingKind::Let, false, false)?;
                            let b = self.local(name)?;
                            if b.repr == Repr::Cell {
                                self.out().op(Opcode::NewCell, name.span);
                                self.out().op_u16(Opcode::SetLocal, b.slot, name.span);
                            }
                        }
                        self.out().op_u16(Opcode::GetLocal, source, pattern.span);
                        self.pattern_store(pattern, true, false)?;
                        self.statement_list(&handler.body, &[], false)?;
                        self.exit_scope(saved);
                    }
                    Some(param) => {
                        let param = param.identifier().unwrap();
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
            if let Some(exit) = self.cur_mut().exits.last_mut() {
                *exit = Exit::FinallyBody;
            }
            let saved = match self.cur().completion {
                Some(slot) => {
                    let tmp = self.temp(span)?;
                    self.out().op_u16(Opcode::GetLocal, slot, span);
                    self.out().op_u16(Opcode::SetLocal, tmp, span);
                    // A `break`/`continue` out of the finally body carries
                    // only the value that body produced (UpdateEmpty with
                    // `undefined`), never the saved one.
                    self.out().op(Opcode::Undefined, span);
                    self.out().op_u16(Opcode::SetLocal, slot, span);
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
            self.cur_mut().exits.pop();
            self.out().op(Opcode::EndFinally, span);
        }
        Ok(())
    }

    // ----- expressions -----

    #[inline(never)]
    fn regexp_literal(
        &mut self,
        pattern: &JsString,
        flags: &str,
        span: Span,
    ) -> Result<(), SyntaxError> {
        let encoded = JsString::from(format!("{flags}\0").as_str())
            .concat(pattern)
            .ok_or_else(|| {
                self.error(SyntaxErrorKind::TooLarge, span, "regexp literal too long")
            })?;
        let index = self.string_index(encoded, span)?;
        self.out().op_u16(Opcode::RegExpLiteral, index, span);
        Ok(())
    }

    fn constant(&mut self, value: Primitive, span: Span) -> Result<(), SyntaxError> {
        let key = match &value {
            Primitive::Null => ConstKey::Null,
            Primitive::Bool(b) => ConstKey::Bool(*b),
            Primitive::Number(n) => ConstKey::Number(n.to_bits()),
            Primitive::String(s) => ConstKey::String(s.clone()),
            Primitive::Symbol(s) => ConstKey::Symbol(s.clone()),
            Primitive::BigInt(n) => ConstKey::BigInt(n.clone()),
            Primitive::Undefined => {
                self.out().op(Opcode::Undefined, span);
                return Ok(());
            }
        };
        let index = self.constant_index(key, value, span)?;
        self.out().op_u16(Opcode::Const, index, span);
        Ok(())
    }

    /// The index of `value` (whose pool key is `key`) in the constant pool,
    /// added if new.
    #[inline(never)]
    fn constant_index(
        &mut self,
        key: ConstKey,
        value: Primitive,
        span: Span,
    ) -> Result<u16, SyntaxError> {
        Ok(match self.cur().out.const_index.get(&key) {
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
        })
    }

    /// The constant-pool index of the property name `name` (a string
    /// constant), for `GetField`, `PutField` and `DefineField`.
    #[inline(never)]
    fn name_index(&mut self, name: &str, span: Span) -> Result<u16, SyntaxError> {
        let units: Vec<u16> = name.encode_utf16().collect();
        let Some(s) = JsString::from_code_units(units) else {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "a property name longer than 2^30 - 1 code units",
            ));
        };
        self.string_index(s, span)
    }

    /// The constant-pool index of the string `s`.
    fn string_index(&mut self, s: JsString, span: Span) -> Result<u16, SyntaxError> {
        self.constant_index(ConstKey::String(s.clone()), Primitive::String(s), span)
    }

    /// `object[index] = value`: the base, the key, the value, then
    /// `PutElem` (which converts the key and leaves the value), reported at
    /// the `[`.
    #[inline(never)]
    fn assign_index(
        &mut self,
        object: &Expr,
        index: &Expr,
        value: &Expr,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        if matches!(object.kind, ExprKind::Super) {
            self.super_base(object.span, false)?;
            self.expr(index)?;
            self.expr(value)?;
            let op = self.strict_op(Opcode::PutSuperElem);
            self.out().op(op, op_span);
            return Ok(());
        }
        self.expr(object)?;
        self.expr(index)?;
        self.expr(value)?;
        let op = self.strict_op(Opcode::PutElem);
        self.out().op(op, op_span);
        Ok(())
    }

    /// `object.property` and `object[index]`: the base, then `GetField`, or
    /// the index expression and `GetElem`. A chain `a.b[c].d` is lowered
    /// iteratively (its base once, then one access per link), so a long
    /// chain takes no Rust stack per link; only an index expression
    /// recurses.
    #[inline(never)]
    fn get_field(&mut self, member: &Expr) -> Result<(), SyntaxError> {
        let mut links = Vec::new();
        let mut base = member;
        loop {
            match &base.kind {
                ExprKind::Member {
                    object,
                    property,
                    op_span,
                } => {
                    links.push((Ok(property), *op_span));
                    base = object;
                }
                ExprKind::Index {
                    object,
                    index,
                    op_span,
                } => {
                    links.push((Err(&**index), *op_span));
                    base = object;
                }
                _ => break,
            }
        }
        match links.last() {
            None => return Err(self.internal(member.span, "not a member expression")),
            // `__rt[x]` is rejected by the parser.
            Some((Ok(property), _)) if self.is_intrinsic_binding(base) => {
                let span = base.span.to(property.span);
                return Err(self.error(
                    SyntaxErrorKind::ReservedWord,
                    span,
                    format!(
                        "the intrinsic `{RESERVED_BINDING}.{}` must be called directly: intrinsics are not values",
                        property.name
                    ),
                ));
            }
            Some(_) => {}
        }
        let mut super_access = matches!(base.kind, ExprKind::Super);
        if super_access {
            self.super_base(base.span, false)?;
        } else {
            // A plain name read as a property base (light-cljs-860).
            self.member_base = matches!(base.kind, ExprKind::Identifier(_));
            self.expr(base)?;
            self.member_base = false;
        }
        for &(link, op_span) in links.iter().rev() {
            if super_access {
                match link {
                    Ok(property) => {
                        self.check_private_super(base, property)?;
                        let name = self.name_index(&property.name, property.span)?;
                        self.out().op_u16(Opcode::Const, name, op_span);
                    }
                    Err(index) => {
                        self.expr(index)?;
                        self.out().op(Opcode::ToSuperKey, op_span);
                    }
                }
                self.out().op(Opcode::GetSuperElem, op_span);
                super_access = false;
                continue;
            }
            match link {
                Ok(property) => {
                    if property.name.starts_with('#') {
                        self.private_name(&property.name, property.span)?;
                        self.out().op(Opcode::GetPrivate, op_span);
                    } else {
                        let index = self.name_index(&property.name, property.span)?;
                        self.out().op_u16(Opcode::GetField, index, op_span);
                    }
                }
                Err(index) => {
                    self.expr(index)?;
                    self.out().op(Opcode::GetElem, op_span);
                }
            }
        }
        Ok(())
    }

    /// An array literal (clj-quickjs-118): `NewArray`, then each element in
    /// source order followed by `AppendElement`, or an `AppendHole` for an
    /// elision. Out of line, like [`Self::get_field`].
    #[inline(never)]
    fn array_literal(&mut self, elements: &[Option<Expr>], span: Span) -> Result<(), SyntaxError> {
        self.out().op(Opcode::NewArray, span);
        for element in elements {
            match element {
                Some(Expr {
                    kind: ExprKind::Spread(operand),
                    span,
                    ..
                }) => {
                    self.spread_element(operand, *span, false)?;
                }
                Some(e) => {
                    self.expr(e)?;
                    self.out().op(Opcode::AppendElement, e.span);
                }
                None => self.out().op(Opcode::AppendHole, span),
            }
        }
        Ok(())
    }

    /// Append to the private array on the operand stack. Reuse GetIterator
    /// and the for-of protocol, with every next/done/value operation in VM
    /// code. Hidden locals and the destination remain precise GC roots.
    #[inline(never)]
    fn spread_element(
        &mut self,
        operand: &Expr,
        span: Span,
        arguments: bool,
    ) -> Result<(), SyntaxError> {
        let saved_slot = self.cur().next_slot;
        let it = self.temp(span)?;
        let next = self.temp(span)?;
        let result = self.temp(span)?;
        self.iterator_acquire(operand, it, next, span)?;
        let start = self.out().label();
        let done = self.out().label();
        self.out().bind(start);
        self.iterator_step(it, next, result, done, span)?;
        self.out().op(Opcode::AppendElement, span);
        if arguments {
            self.out().op(Opcode::CheckSpreadArguments, span);
        }
        self.out().branch(Opcode::Jump, start, span);
        self.out().bind(done);
        for slot in [it, next, result] {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
        self.cur_mut().next_slot = saved_slot;
        Ok(())
    }

    fn spread_arguments(&mut self, args: &[Expr], span: Span) -> Result<(), SyntaxError> {
        self.out().op(Opcode::NewArray, span);
        for arg in args {
            if let ExprKind::Spread(operand) = &arg.kind {
                self.spread_element(operand, arg.span, true)?;
            } else {
                self.expr(arg)?;
                self.out().op(Opcode::AppendElement, arg.span);
            }
            self.out().op(Opcode::CheckSpreadArguments, arg.span);
        }
        Ok(())
    }

    /// The callee of a method call `object.property(...)`: the base once,
    /// kept below the property's value as the receiver (`CallMethod`). A
    /// parenthesized member callee is the same AST node, so `(o.m)()` keeps
    /// its receiver; `(0, o.m)()` and `(f = o.m)()` are not member
    /// callees and call with `this` undefined. Out of line, so the
    /// recursive [`Self::expr`] frame stays small.
    #[inline(never)]
    fn method_callee(&mut self, callee: &Expr) -> Result<(), SyntaxError> {
        match &callee.kind {
            ExprKind::OptionalChain { base, links } => {
                self.optional_chain(base, links, true, None, callee.span)?;
            }
            ExprKind::Member {
                object,
                property,
                op_span,
            } => {
                self.check_private_super(object, property)?;
                if matches!(object.kind, ExprKind::Super) {
                    self.super_base(object.span, true)?;
                    let name = self.name_index(&property.name, property.span)?;
                    self.out().op_u16(Opcode::Const, name, *op_span);
                    self.out().op(Opcode::GetSuperElem, *op_span);
                    return Ok(());
                }
                self.expr(object)?;
                self.out().op(Opcode::Dup, *op_span);
                if property.name.starts_with('#') {
                    self.private_name(&property.name, property.span)?;
                    self.out().op(Opcode::GetPrivate, *op_span);
                    return Ok(());
                }
                let index = self.name_index(&property.name, property.span)?;
                self.out().op_u16(Opcode::GetField, index, *op_span);
            }
            // `o[k](...)` (clj-quickjs-118): the base once, then the key.
            ExprKind::Index {
                object,
                index,
                op_span,
            } => {
                if matches!(object.kind, ExprKind::Super) {
                    self.super_base(object.span, true)?;
                    self.expr(index)?;
                    self.out().op(Opcode::ToSuperKey, *op_span);
                    self.out().op(Opcode::GetSuperElem, *op_span);
                    return Ok(());
                }
                self.expr(object)?;
                self.out().op(Opcode::Dup, *op_span);
                self.expr(index)?;
                self.out().op(Opcode::GetElem, *op_span);
            }
            _ => return Err(self.internal(callee.span, "not a member callee")),
        }
        Ok(())
    }

    /// One forward pass through the entire chain. Every optional link's
    /// nullish branch discards its live value/receiver and bypasses all
    /// remaining links, including their computed keys and arguments.
    /// Reference mode retains the final member's receiver for `(a?.m)()`.
    #[inline(never)]
    fn optional_chain(
        &mut self,
        base: &Expr,
        links: &[ChainLink],
        reference: bool,
        delete: Option<Span>,
        span: Span,
    ) -> Result<(), SyntaxError> {
        let end = self.out().label();
        let mut method = if matches!(links[0].kind, ChainLinkKind::Call(_)) {
            self.call_callee(base)?
        } else {
            self.expr(base)?;
            false
        };
        for (i, link) in links.iter().enumerate() {
            let at = link.op_span;
            let last = i + 1 == links.len();
            if link.optional {
                let check_null = self.out().label();
                let short = self.out().label();
                let next = self.out().label();
                self.out().op(Opcode::Dup, at);
                self.out().op(Opcode::Undefined, at);
                self.out().op(Opcode::StrictEq, at);
                self.out().branch(Opcode::JumpIfFalse, check_null, at);
                self.out().branch(Opcode::Jump, short, at);
                self.out().bind(check_null);
                self.out().op(Opcode::Dup, at);
                self.constant(Primitive::Null, at)?;
                self.out().op(Opcode::StrictEq, at);
                self.out().branch(Opcode::JumpIfFalse, next, at);
                self.out().bind(short);
                self.out().op(Opcode::Pop, at);
                if method {
                    self.out().op(Opcode::Pop, at);
                }
                if reference {
                    self.out().op(Opcode::Undefined, at);
                }
                if delete.is_some() {
                    self.constant(Primitive::Bool(true), at)?;
                } else {
                    self.out().op(Opcode::Undefined, at);
                }
                self.out().branch(Opcode::Jump, end, at);
                self.out().bind(next);
            }
            match &link.kind {
                ChainLinkKind::Call(args) => {
                    self.call_arguments(args, method, span, at, None)?;
                    method = false;
                    if last && let Some(delete_at) = delete {
                        self.out().op(Opcode::Pop, delete_at);
                        self.constant(Primitive::Bool(true), delete_at)?;
                    }
                }
                kind => {
                    let deleting = last && delete.is_some();
                    method = !deleting
                        && (if last {
                            reference
                        } else {
                            matches!(links[i + 1].kind, ChainLinkKind::Call(_))
                        });
                    if method {
                        self.out().op(Opcode::Dup, at);
                    }
                    match kind {
                        ChainLinkKind::Member(property) => {
                            if property.name.starts_with('#') {
                                self.private_name(&property.name, property.span)?;
                                self.out().op(Opcode::GetPrivate, at);
                                continue;
                            }
                            let name = self.name_index(&property.name, property.span)?;
                            self.out().op_u16(
                                if deleting {
                                    Opcode::Const
                                } else {
                                    Opcode::GetField
                                },
                                name,
                                at,
                            );
                        }
                        ChainLinkKind::Index(index) => {
                            self.expr(index)?;
                            if !deleting {
                                self.out().op(Opcode::GetElem, at);
                            }
                        }
                        ChainLinkKind::Call(_) => unreachable!(),
                    }
                    if deleting {
                        let op = self.strict_op(Opcode::Delete);
                        self.out().op(op, delete.unwrap());
                    }
                }
            }
        }
        self.out().bind(end);
        Ok(())
    }

    /// `object.property = value`: the base, then the value, then
    /// `PutField`, which leaves the value. Run-time failures are reported
    /// at the `.` (the operator-token convention of clj-quickjs-52).
    #[inline(never)]
    fn assign_member(
        &mut self,
        object: &Expr,
        property: &Ident,
        value: &Expr,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        self.check_private_super(object, property)?;
        if property.name.starts_with('#') {
            self.expr(object)?;
            self.private_name(&property.name, property.span)?;
            self.expr(value)?;
            self.out().op(Opcode::PutPrivate, op_span);
            return Ok(());
        }
        if matches!(object.kind, ExprKind::Super) {
            self.super_base(object.span, false)?;
            let name = self.name_index(&property.name, property.span)?;
            self.out().op_u16(Opcode::Const, name, op_span);
            self.expr(value)?;
            let op = self.strict_op(Opcode::PutSuperElem);
            self.out().op(op, op_span);
            return Ok(());
        }
        self.expr(object)?;
        self.expr(value)?;
        let index = self.name_index(&property.name, property.span)?;
        let op = self.strict_op(Opcode::PutField);
        self.out().op_u16(op, index, op_span);
        Ok(())
    }

    /// A property reference that is read and then written (`o.p++`,
    /// `o[k]--`, compound assignment `o.p op= v`, clj-quickjs-260, and
    /// logical assignment, clj-quickjs-314):
    /// evaluates the base once and, for `o[k]`, the key once, followed by
    /// `ToPropertyKey` (the base's `undefined`/`null` check and
    /// the only key conversion), leaving `o` or `o key` on the stack. Then
    /// [`Self::reference_get`] pushes the property's value above them and
    /// [`Self::reference_put`] writes the value on top and leaves it.
    /// `target` is an [`ExprKind::Member`] whose object is not the reserved
    /// intrinsic binding (the parser rejects that one) or an
    /// [`ExprKind::Index`].
    #[inline(never)]
    fn property_reference(
        &mut self,
        target: &Expr,
        convert_super: bool,
    ) -> Result<PropertyRef, SyntaxError> {
        match &target.kind {
            ExprKind::Member {
                object,
                property,
                op_span,
            } => {
                self.check_private_super(object, property)?;
                if matches!(object.kind, ExprKind::Super) {
                    self.super_base(object.span, false)?;
                    let name = self.name_index(&property.name, property.span)?;
                    self.out().op_u16(Opcode::Const, name, *op_span);
                    return Ok(PropertyRef::Super(*op_span));
                }
                self.expr(object)?;
                if property.name.starts_with('#') {
                    self.private_name(&property.name, property.span)?;
                    return Ok(PropertyRef::Private(*op_span));
                }
                let name = self.name_index(&property.name, property.span)?;
                Ok(PropertyRef::Field(name, *op_span))
            }
            ExprKind::Index {
                object,
                index,
                op_span,
            } => {
                if matches!(object.kind, ExprKind::Super) {
                    self.super_base(object.span, false)?;
                    self.expr(index)?;
                    if convert_super {
                        self.out().op(Opcode::ToSuperKey, *op_span);
                    }
                    return Ok(PropertyRef::Super(*op_span));
                }
                self.expr(object)?;
                self.expr(index)?;
                self.out().op(Opcode::ToPropertyKey, *op_span);
                Ok(PropertyRef::Elem(*op_span))
            }
            _ => Err(self.internal(target.span, "not a property reference")),
        }
    }

    /// Reads the reference [`Self::property_reference`] left (`o` or
    /// `o key` → the same, then the value), reported at the `.` or `[`.
    fn reference_get(&mut self, r: PropertyRef) {
        match r {
            PropertyRef::Private(at) => {
                self.out().op(Opcode::Dup2, at);
                self.out().op(Opcode::GetPrivate, at);
            }
            PropertyRef::Super(at) => {
                self.out().op(Opcode::Dup3, at);
                self.out().op(Opcode::GetSuperElem, at);
            }
            PropertyRef::Field(name, at) => {
                self.out().op(Opcode::Dup, at);
                self.out().op_u16(Opcode::GetField, name, at);
            }
            PropertyRef::Elem(at) => {
                self.out().op(Opcode::Dup2, at);
                self.out().op(Opcode::GetElem, at);
            }
        }
    }

    /// Writes the value on top to the reference below it (`o v` or
    /// `o key v` → `v`), strictly, reported at the `.` or `[`.
    fn reference_put(&mut self, r: PropertyRef) {
        let field = self.strict_op(Opcode::PutField);
        let elem = self.strict_op(Opcode::PutElem);
        match r {
            PropertyRef::Private(at) => self.out().op(Opcode::PutPrivate, at),
            PropertyRef::Super(at) => {
                let op = self.strict_op(Opcode::PutSuperElem);
                self.out().op(op, at);
            }
            PropertyRef::Field(name, at) => self.out().op_u16(field, name, at),
            PropertyRef::Elem(at) => self.out().op(elem, at),
        }
    }

    /// `++o.p`, `o.p++`, `--o[k]`, `o[k]--` (clj-quickjs-76; QuickJS
    /// `js_parse_postfix_expr` / `js_parse_unary` lvalue handling): the
    /// reference once, its value, ToNumeric, increment/decrement, the write.
    /// A postfix update keeps the converted old value in a scratch local
    /// across the write (it is numeric, so the local retains no object)
    /// and gives it instead of the written one. The conversion and the
    /// arithmetic are reported at the `++`/`--`.
    #[inline(never)]
    fn update_property(
        &mut self,
        op: UpdateOp,
        prefix: bool,
        target: &Expr,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        let r = self.property_reference(target, true)?;
        self.reference_get(r);
        self.out().op(Opcode::ToNumeric, op_span);
        let old = if prefix {
            None
        } else {
            let tmp = self.temp(op_span)?;
            self.out().op(Opcode::Dup, op_span);
            self.out().op_u16(Opcode::SetLocal, tmp, op_span);
            Some(tmp)
        };
        let arith = match op {
            UpdateOp::Inc => Opcode::Inc,
            UpdateOp::Dec => Opcode::Dec,
        };
        self.out().op(arith, op_span);
        self.reference_put(r);
        if let Some(tmp) = old {
            self.out().op(Opcode::Pop, op_span);
            self.out().op_u16(Opcode::GetLocal, tmp, op_span);
            self.cur_mut().next_slot -= 1;
        }
        Ok(())
    }

    /// `o.p op= v` and `o[k] op= v` (clj-quickjs-260; QuickJS
    /// `js_parse_assign_expr` with `get_lvalue`/`put_lvalue`): the reference
    /// once (so a `null`/`undefined` base fails before `v` runs), its value,
    /// `v`, the binary operator's own instructions (as for a binding,
    /// clj-quickjs-84), the write, whose value is the result. The operator
    /// is reported at the `op=`.
    #[inline(never)]
    fn compound_assign_property(
        &mut self,
        op: BinaryOp,
        target: &Expr,
        value: &Expr,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        let r = self.property_reference(target, true)?;
        self.reference_get(r);
        self.expr(value)?;
        self.binary(op, false, is_number(value), op_span);
        self.reference_put(r);
        Ok(())
    }

    /// An object literal: `NewObjectLiteral`, then each value in source order
    /// followed by a `DefineField` of its key (a later duplicate redefines
    /// the property in place, so the last value wins at the position of the
    /// first), or by `SetProto` for `__proto__: value` (clj-quickjs-121).
    /// Out of line, like [`Self::get_field`].
    #[inline(never)]
    fn object_literal(
        &mut self,
        properties: &[PropertyDef],
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.out().op(Opcode::NewObjectLiteral, span);
        for property in properties {
            if let ExprKind::Spread(source) = &property.value.kind {
                self.expr(source)?;
                self.out()
                    .op(Opcode::CopyDataProperties, property.name_span);
                continue;
            }
            if let PropertyName::Computed(key, _) = &property.name {
                self.computed_property(property, key)?;
                continue;
            }
            if property.sets_prototype() {
                self.expr(&property.value)?;
            } else {
                let name = match &property.name {
                    PropertyName::Identifier(n) => JsString::from(n.as_str()),
                    PropertyName::String(n) => n.clone(),
                    PropertyName::Number(n) => JsString::from(number_to_string(*n).as_str()),
                    PropertyName::Computed(_, _) => {
                        return Err(self.internal(span, "unconverted object cover"));
                    }
                };
                let name = match &property.value.kind {
                    ExprKind::Function(f) if f.kind == FunctionKind::Getter => {
                        JsString::concat(&JsString::from("get "), &name).ok_or_else(|| {
                            self.error(
                                SyntaxErrorKind::TooLarge,
                                property.name_span,
                                "accessor name too long",
                            )
                        })?
                    }
                    ExprKind::Function(f) if f.kind == FunctionKind::Setter => {
                        JsString::concat(&JsString::from("set "), &name).ok_or_else(|| {
                            self.error(
                                SyntaxErrorKind::TooLarge,
                                property.name_span,
                                "accessor name too long",
                            )
                        })?
                    }
                    _ => name,
                };
                self.named_expr(&property.value, name)?;
            }
            if property.sets_prototype() {
                // `__proto__: value` sets the prototype (clj-quickjs-121).
                self.out().op(Opcode::SetProto, property.name_span);
                continue;
            }
            let index = match &property.name {
                PropertyName::Identifier(name) => self.name_index(name, property.name_span)?,
                PropertyName::String(s) => self.string_index(s.clone(), property.name_span)?,
                PropertyName::Number(n) => {
                    self.name_index(&number_to_string(*n), property.name_span)?
                }
                PropertyName::Computed(_, _) => {
                    return Err(self.internal(span, "unconverted object cover"));
                }
            };
            let opcode = match &property.value.kind {
                ExprKind::Function(f) if f.kind == FunctionKind::Getter => Opcode::DefineGetter,
                ExprKind::Function(f) if f.kind == FunctionKind::Setter => Opcode::DefineSetter,
                _ => Opcode::DefineField,
            };
            self.out().op_u16(opcode, index, property.name_span);
        }
        Ok(())
    }

    /// Convert before evaluating the value. Stash the key while creating a
    /// method so `Method` sees the literal as its home object on the stack.
    #[inline(never)]
    fn computed_property(&mut self, property: &PropertyDef, key: &Expr) -> Result<(), SyntaxError> {
        let span = property.name_span;
        self.expr(key)?;
        self.out().op(Opcode::ToPropertyKey, span);
        let saved = self.cur().next_slot;
        let key_slot = self.temp(span)?;
        let value_slot = self.temp(span)?;
        self.out().op_u16(Opcode::SetLocal, key_slot, span);
        if let ExprKind::Class(c) = &property.value.kind
            && c.name.is_none()
        {
            self.class_expr(c, Some(ClassName::Computed(key_slot)))?;
        } else {
            self.expr(&property.value)?;
        }
        self.out().op_u16(Opcode::SetLocal, value_slot, span);
        self.out().op_u16(Opcode::GetLocal, key_slot, span);
        self.out().op_u16(Opcode::GetLocal, value_slot, span);
        if let ExprKind::Function(f) = &property.value.kind
            && f.name.is_none()
        {
            let prefix = match f.kind {
                FunctionKind::Getter => "get ",
                FunctionKind::Setter => "set ",
                _ => "",
            };
            let prefix = self.name_index(prefix, span)?;
            self.out().op_u16(Opcode::SetComputedName, prefix, span);
        }
        let opcode = match &property.value.kind {
            ExprKind::Function(f) if f.kind == FunctionKind::Getter => Opcode::DefineGetterElem,
            ExprKind::Function(f) if f.kind == FunctionKind::Setter => Opcode::DefineSetterElem,
            _ => Opcode::DefineElem,
        };
        self.out().op(opcode, span);
        for slot in [key_slot, value_slot] {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot, span);
        }
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// An expression's code. Only a dispatcher (clj-quickjs-262): each
    /// form's code is an out-of-line helper, called in tail position, so
    /// this frame holds no `?` temporaries or locals of any form. A nested
    /// expression recurses through this frame plus one helper's, which
    /// bounds the Rust stack each nesting level costs in a debug build
    /// (the parser's nesting budget, `parser::cost`, relies on it;
    /// `frontend::tests::nesting_at_the_limit_fits_a_2_mib_stack` checks
    /// it).
    fn expr(&mut self, e: &Expr) -> Result<(), SyntaxError> {
        let span = e.span;
        match &e.kind {
            ExprKind::AssignPattern { target, value } => self.assign_pattern(target, value, span),
            ExprKind::RegExp { pattern, flags } => self.regexp_literal(pattern, flags, span),
            ExprKind::Number(_)
            | ExprKind::BigInt(_)
            | ExprKind::Bool(_)
            | ExprKind::Null
            | ExprKind::This
            | ExprKind::NewTarget
            | ExprKind::Undefined
            | ExprKind::String(_) => self.literal(e),
            ExprKind::Identifier(name) => self.identifier(name, span),
            ExprKind::Unary {
                op: UnaryOp::Typeof,
                operand,
            } if matches!(operand.kind, ExprKind::Identifier(_)) => {
                self.typeof_reference(operand, span)
            }
            ExprKind::Unary { op, operand } => self.unary(*op, operand, span),
            ExprKind::Binary {
                op,
                left,
                right,
                op_span,
            } => self.binary_expr(*op, left, right, *op_span),
            ExprKind::Logical { op, left, right } => self.logical(*op, left, right, span),
            ExprKind::LogicalAssign {
                op,
                target,
                value,
                op_span,
            } => self.logical_assign(*op, target, value, *op_span),
            ExprKind::Conditional {
                test,
                consequent,
                alternate,
            } => self.conditional(test, consequent, alternate, span),
            ExprKind::Assign { target, value } => self.assign(target, value),
            ExprKind::CompoundAssign {
                op,
                target,
                value,
                op_span,
            } => self.compound_assign(*op, target, value, *op_span),
            ExprKind::Update {
                op,
                prefix,
                target,
                op_span,
            } => self.update(*op, *prefix, target, *op_span),
            ExprKind::Call {
                callee,
                args,
                op_span,
            } => self.call(callee, args, span, *op_span),
            ExprKind::SuperCall { args, op_span } => self.super_call(args, *op_span, false),
            ExprKind::Super => Err(self.internal(span, "standalone super")),
            ExprKind::New { .. } => self.new_expr(e),
            ExprKind::OptionalChain { base, links } => {
                self.optional_chain(base, links, false, None, span)
            }
            ExprKind::Member { .. } | ExprKind::Index { .. } => self.get_field(e),
            ExprKind::Object(properties) => self.object_literal(properties, span),
            ExprKind::AssignMember {
                object,
                property,
                value,
                op_span,
            } => self.assign_member(object, property, value, *op_span),
            ExprKind::Template {
                tag: Some(tag),
                quasis,
                exprs,
            } => self.tagged_template(tag, quasis, exprs, span),
            ExprKind::Template { quasis, exprs, .. } => self.template(quasis, exprs, span),
            ExprKind::Array(elements) => self.array_literal(elements, span),
            ExprKind::AssignIndex {
                object,
                index,
                value,
                op_span,
            } => self.assign_index(object, index, value, *op_span),
            ExprKind::UpdateProperty {
                op,
                prefix,
                target,
                op_span,
            } => self.update_property(*op, *prefix, target, *op_span),
            ExprKind::CompoundAssignProperty {
                op,
                target,
                value,
                op_span,
            } => self.compound_assign_property(*op, target, value, *op_span),
            ExprKind::Function(f) => self.function_expr(f),
            ExprKind::Class(c) => self.class_expr(c, None),
            ExprKind::Spread(_) => Err(self.internal(span, "spread outside element list")),

            ExprKind::Yield {
                value,
                delegate: false,
            } => {
                // Leave enclosing expression operands in place. Suspension
                // saves the entire activation stack above its base, and
                // resume pushes next(value) as this expression's result.
                if let Some(value) = value {
                    self.expr(value)?;
                } else {
                    self.out().op(Opcode::Undefined, span);
                }
                if self.cur().async_generator {
                    self.out().op(Opcode::Await, span);
                }
                self.out().op(Opcode::GeneratorYield, span);
                // Resume supplies [value, kind]. Abrupt completions run at
                // this yield site, under its saved catch/finally handlers.
                let abrupt = self.out().label();
                let throwing = self.out().label();
                let done = self.out().label();
                self.out().op(Opcode::Dup, span);
                self.constant(Primitive::Number(0.0), span)?;
                self.out().op(Opcode::StrictEq, span);
                self.out().branch(Opcode::JumpIfFalse, abrupt, span);
                self.out().op(Opcode::Pop, span);
                self.out().branch(Opcode::Jump, done, span);
                self.out().bind(abrupt);
                self.constant(Primitive::Number(1.0), span)?;
                self.out().op(Opcode::StrictEq, span);
                self.out().branch(Opcode::JumpIfFalse, throwing, span);
                if self.cur().async_generator {
                    self.out().op(Opcode::Await, span);
                }
                self.out().op(Opcode::Return, span);
                self.out().bind(throwing);
                self.out().op(Opcode::Throw, span);
                self.out().bind(done);
                Ok(())
            }
            ExprKind::Yield {
                value: Some(value),
                delegate: true,
            } => self.yield_star(value, span),
            ExprKind::Yield {
                value: None,
                delegate: true,
            } => Err(self.internal(span, "delegated yield without operand")),
            ExprKind::Await(value) => {
                self.expr(value)?;
                self.out().op(Opcode::Await, span);
                Ok(())
            }
            ExprKind::Sequence { left, right } => self.sequence(left, right, span),
            ExprKind::Delete { operand, op_span } => self.delete(operand, *op_span),
        }
    }

    /// A literal, `this` or `undefined`.
    #[inline(never)]
    fn literal(&mut self, e: &Expr) -> Result<(), SyntaxError> {
        let span = e.span;
        match &e.kind {
            ExprKind::Number(n) => self.constant(Primitive::Number(*n), span),
            ExprKind::BigInt(n) => self.constant(Primitive::BigInt(n.clone()), span),
            ExprKind::Bool(b) => self.constant(Primitive::Bool(*b), span),
            ExprKind::Null => self.constant(Primitive::Null, span),
            ExprKind::String(s) => self.constant(Primitive::String(s.clone()), span),
            ExprKind::This => {
                let place = self.resolve_this(self.fns.len() - 1, false);
                self.out().load(place, span);
                self.out().op(Opcode::ResolveThis, span);
                // Imported receivers have not been evaluated in the caller.
                // Check only when `this` is actually read, including inside
                // arrows created by eval. Ordinary function reads still use
                // GetThis and ordinary non-eval captures keep their fast path.
                let imported = self
                    .fns
                    .iter()
                    .rev()
                    .find(|ctx| !ctx.lexical_this)
                    .is_some_and(|ctx| ctx.completion.is_some() && self.eval_this.is_some());
                if imported && !self.cur().this_strict {
                    self.out().op(Opcode::CheckSloppyThis, span);
                }
                Ok(())
            }
            ExprKind::NewTarget => {
                let place = self
                    .resolve_new_target(self.fns.len() - 1)
                    .ok_or_else(|| self.internal(span, "new.target outside a function"))?;
                self.out().load(place, span);
                Ok(())
            }
            ExprKind::Undefined => {
                self.out().op(Opcode::Undefined, span);
                Ok(())
            }
            _ => Err(self.internal(span, "not a literal")),
        }
    }

    /// A reference to the binding `name`, read with the checks of any read.
    #[inline(never)]
    fn identifier(&mut self, name: &str, span: Span) -> Result<(), SyntaxError> {
        let at = self.name_span(name, span);
        let access = self.resolve_reference(name);
        self.member_base = false;
        self.load(access, at)
    }

    fn private_name(&mut self, name: &str, at: Span) -> Result<(), SyntaxError> {
        let access = self.resolve(name);
        if !matches!(access, Access::Found { .. }) {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                at,
                format!("undeclared private name {name}"),
            ));
        }
        self.load(access, at)
    }

    fn check_private_super(&self, object: &Expr, property: &Ident) -> Result<(), SyntaxError> {
        if property.name.starts_with('#') && matches!(object.kind, ExprKind::Super) {
            return Err(self.error(
                SyntaxErrorKind::Invalid,
                property.span,
                "super cannot access private names",
            ));
        }
        Ok(())
    }

    /// `delete operand` (clj-quickjs-124; QuickJS `js_parse_delete`): for
    /// `o.p` the base and the name as a string constant, for `o[k]` the
    /// base and the key, then `Delete`, which checks the base, converts the
    /// key and removes the own property (reported at the `delete`). Any
    /// other value operand is evaluated for its effects and gives `true`.
    /// Sloppy identifiers delete a global property or give `false` for a
    /// declarative binding without reading it (even in its temporal dead zone).
    /// The parser rejects strict identifier operands.
    #[inline(never)]
    fn delete(&mut self, operand: &Expr, op_span: Span) -> Result<(), SyntaxError> {
        if matches!(&operand.kind,
            ExprKind::Member { object, .. } | ExprKind::Index { object, .. }
                if matches!(object.kind, ExprKind::Super))
        {
            self.property_reference(operand, false)?;
            for _ in 0..3 {
                self.out().op(Opcode::Pop, op_span);
            }
            self.throw(ThrowKind::DeleteSuper, op_span);
            self.out().op(Opcode::Undefined, op_span);
            return Ok(());
        }
        match &operand.kind {
            ExprKind::OptionalChain { base, links } => {
                return self.optional_chain(base, links, false, Some(op_span), operand.span);
            }
            ExprKind::Member {
                object, property, ..
            } => {
                self.expr(object)?;
                let name = self.name_index(&property.name, property.span)?;
                self.out().op_u16(Opcode::Const, name, property.span);
            }
            ExprKind::Index { object, index, .. } => {
                self.expr(object)?;
                self.expr(index)?;
            }
            ExprKind::Identifier(name) => return self.delete_identifier(name, op_span),
            _ => {
                self.expr(operand)?;
                self.out().op(Opcode::Pop, op_span);
                return self.constant(Primitive::Bool(true), op_span);
            }
        }
        let op = self.strict_op(Opcode::Delete);
        self.out().op(op, op_span);
        Ok(())
    }

    #[inline(never)]
    fn delete_identifier(&mut self, name: &str, at: Span) -> Result<(), SyntaxError> {
        let access = self.resolve(name);
        self.delete_access(access, at)
    }

    fn delete_access(&mut self, access: Access, at: Span) -> Result<(), SyntaxError> {
        match access {
            Access::Prepared { .. } => {
                Err(self.internal(at, "cannot delete a prepared assignment"))
            }
            Access::Dynamic(i) => {
                let (env, _, name, fallback) = self.dynamic_accesses[i].clone();
                let missing = self.out().label();
                let done = self.out().label();
                self.environment_lookup(env, &name, at)?;
                self.out().branch(Opcode::JumpIfFalse, missing, at);
                self.out().load(env, at);
                let object = self.out().label();
                self.out().op(Opcode::Dup, at);
                self.out().op(Opcode::IsCell, at);
                self.out().branch(Opcode::JumpIfFalse, object, at);
                self.out().op(Opcode::CellGet, at);
                self.out().bind(object);
                self.constant(Primitive::String(JsString::from(name.as_str())), at)?;
                self.out().op(Opcode::Delete, at);
                self.out().branch(Opcode::Jump, done, at);
                self.out().bind(missing);
                self.delete_access(fallback, at)?;
                self.out().bind(done);
                Ok(())
            }
            Access::Global(global) => {
                self.global_op(Opcode::DeleteGlobal, global, at)?;
                Ok(())
            }
            Access::Found { .. } => self.constant(Primitive::Bool(false), at),
            Access::Intrinsic => Err(self.not_a_value(at)),
        }
    }

    /// A prefix operator other than `typeof` of an identifier.
    #[inline(never)]
    fn unary(&mut self, op: UnaryOp, operand: &Expr, span: Span) -> Result<(), SyntaxError> {
        self.expr(operand)?;
        match op {
            UnaryOp::Typeof => self.out().op(Opcode::Typeof, span),
            UnaryOp::Void => self.void_result(span),
            UnaryOp::Not => self.out().op(Opcode::Not, span),
            UnaryOp::Neg | UnaryOp::Plus => {
                if !(is_number(operand) || op == UnaryOp::Neg && is_numeric(operand)) {
                    self.out().op(
                        if op == UnaryOp::Neg {
                            Opcode::ToNumeric
                        } else {
                            Opcode::ToNumber
                        },
                        span,
                    );
                }
                if op == UnaryOp::Neg {
                    self.out().op(Opcode::Neg, span);
                }
            }
            UnaryOp::BitNot => {
                if !is_number(operand) {
                    self.out().op(Opcode::ToNumeric, span);
                }
                self.out().op(Opcode::BitNot, span);
            }
        }
        Ok(())
    }

    /// `left op right`: both operands, then the operator's instructions.
    #[inline(never)]
    fn binary_expr(
        &mut self,
        op: BinaryOp,
        left: &Expr,
        right: &Expr,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        if op == BinaryOp::In
            && let ExprKind::Identifier(name) = &left.kind
            && name.starts_with('#')
        {
            self.private_name(name, left.span)?;
            self.expr(right)?;
            self.out().op(Opcode::PrivateIn, op_span);
            return Ok(());
        }
        self.expr(left)?;
        self.expr(right)?;
        self.binary(op, is_number(left), is_number(right), op_span);
        Ok(())
    }

    /// `left && right`, `left || right`, `left ?? right`: the left value is the result
    /// unless it selects the right.
    #[inline(never)]
    fn logical(
        &mut self,
        op: LogicalOp,
        left: &Expr,
        right: &Expr,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.expr(left)?;
        let end = self.out().label();
        self.logical_skip(op, end, span)?;
        self.out().op(Opcode::Pop, span);
        self.expr(right)?;
        self.out().bind(end);
        Ok(())
    }

    /// Preserve the value on top and branch when the RHS is not selected.
    /// Strict comparisons avoid invoking any user conversion for ??/??=.
    fn logical_skip(&mut self, op: LogicalOp, end: usize, span: Span) -> Result<(), SyntaxError> {
        self.out().op(Opcode::Dup, span);
        match op {
            LogicalOp::And => {}
            LogicalOp::Or => self.out().op(Opcode::Not, span),
            LogicalOp::Coalesce => {
                let check_null = self.out().label();
                let rhs = self.out().label();
                self.out().op(Opcode::Undefined, span);
                self.out().op(Opcode::StrictEq, span);
                self.out().branch(Opcode::JumpIfFalse, check_null, span);
                self.out().branch(Opcode::Jump, rhs, span);
                self.out().bind(check_null);
                self.out().op(Opcode::Dup, span);
                self.constant(Primitive::Null, span)?;
                self.out().op(Opcode::StrictEq, span);
                self.out().branch(Opcode::JumpIfFalse, end, span);
                self.out().bind(rhs);
                return Ok(());
            }
        }
        self.out().branch(Opcode::JumpIfFalse, end, span);
        Ok(())
    }

    /// Resolve and read once, then bypass both RHS evaluation and PutValue
    /// on the short branch. Only identifier references use NamedEvaluation.
    #[inline(never)]
    fn logical_assign(
        &mut self,
        op: LogicalOp,
        target: &Expr,
        value: &Expr,
        span: Span,
    ) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let short = self.out().label();
        if let ExprKind::Identifier(name) = &target.kind {
            let at = self.name_span(name, target.span);
            let access = self.resolve_reference(name);
            if matches!(access, Access::Intrinsic) {
                return self.store(access, at);
            }
            let access = self.prepare_reference(access, at)?;
            self.load(access, at)?;
            self.logical_skip(op, short, span)?;
            self.out().op(Opcode::Pop, span);
            self.named_expr(value, JsString::from(name.as_str()))?;
            self.store(access, at)?;
            self.out().bind(short);
        } else {
            let r = self.property_reference(target, true)?;
            self.reference_get(r);
            self.logical_skip(op, short, span)?;
            self.out().op(Opcode::Pop, span);
            self.expr(value)?;
            self.reference_put(r);
            let end = self.out().label();
            self.out().branch(Opcode::Jump, end, span);
            self.out().bind(short);
            // Remove the reference underneath the old value without a
            // write. The scratch slot is cleared immediately after use.
            let result = self.temp(span)?;
            self.out().op_u16(Opcode::SetLocal, result, span);
            let depth = match r {
                PropertyRef::Field(..) => 1,
                PropertyRef::Elem(..) | PropertyRef::Private(..) => 2,
                PropertyRef::Super(..) => 3,
            };
            for _ in 0..depth {
                self.out().op(Opcode::Pop, span);
            }
            self.out().op_u16(Opcode::GetLocal, result, span);
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, result, span);
            self.out().bind(end);
        }
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// `test ? consequent : alternate`.
    #[inline(never)]
    fn conditional(
        &mut self,
        test: &Expr,
        consequent: &Expr,
        alternate: &Expr,
        span: Span,
    ) -> Result<(), SyntaxError> {
        self.expr(test)?;
        let otherwise = self.out().label();
        let end = self.out().label();
        self.out().branch(Opcode::JumpIfFalse, otherwise, test.span);
        self.expr(consequent)?;
        self.out().branch(Opcode::Jump, end, span);
        self.out().bind(otherwise);
        self.expr(alternate)?;
        self.out().bind(end);
        Ok(())
    }

    /// `target = value` for a binding: the value, then the store.
    #[inline(never)]
    fn assign(&mut self, target: &Ident, value: &Expr) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let at = self.name_span(&target.name, target.span);
        let access = self.resolve_reference(&target.name);
        let access = self.prepare_reference(access, at)?;
        self.named_expr(value, JsString::from(target.name.as_str()))?;
        self.store(access, at)?;
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// `target op= value` for a binding: read the binding (with the same
    /// checks as any read), then evaluate the right operand, apply the
    /// binary operator's own instructions, and store (with the same checks
    /// as `=`, so a `const` is a TypeError after the right operand).
    #[inline(never)]
    fn compound_assign(
        &mut self,
        op: BinaryOp,
        target: &Ident,
        value: &Expr,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let at = self.name_span(&target.name, target.span);
        let access = self.resolve_reference(&target.name);
        if matches!(access, Access::Intrinsic) {
            // The error of `__rt = x`, not of reading `__rt`.
            return self.store(access, at);
        }
        let access = self.prepare_reference(access, at)?;
        self.load(access, at)?;
        self.expr(value)?;
        self.binary(op, false, is_number(value), op_span);
        self.store(access, at)?;
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// `++target`, `target--`, ... for a binding: read the binding (with
    /// the same checks as any read), ToNumeric, increment/decrement, store
    /// (with the same checks as `=`). A postfix update keeps the converted
    /// old value under the new one and drops the stored value.
    #[inline(never)]
    fn update(
        &mut self,
        op: UpdateOp,
        prefix: bool,
        target: &Ident,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        let saved = self.cur().next_slot;
        let at = self.name_span(&target.name, target.span);
        let access = self.resolve_reference(&target.name);
        let access = self.prepare_reference(access, at)?;
        self.load(access, at)?;
        self.out().op(Opcode::ToNumeric, op_span);
        if !prefix {
            self.out().op(Opcode::Dup, op_span);
        }
        let arith = match op {
            UpdateOp::Inc => Opcode::Inc,
            UpdateOp::Dec => Opcode::Dec,
        };
        self.out().op(arith, op_span);
        self.store(access, at)?;
        if !prefix {
            self.out().op(Opcode::Pop, op_span);
        }
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// `callee(args)`: an intrinsic call, a method call (member callee,
    /// see [`Self::method_callee`]) or a plain call.
    #[inline(never)]
    fn call(
        &mut self,
        callee: &Expr,
        args: &[Expr],
        span: Span,
        op_span: Span,
    ) -> Result<(), SyntaxError> {
        let direct = matches!(&callee.kind, ExprKind::Identifier(name) if name == "eval");
        let saved = self.cur().next_slot;
        if let ExprKind::Member {
            object, property, ..
        } = &callee.kind
            && self.is_intrinsic_binding(object)
        {
            return self.intrinsic(property, args, span, op_span);
        }
        let method = self.call_callee(callee)?;
        self.call_arguments(args, method, span, op_span, direct.then_some(callee.span))?;
        self.cur_mut().next_slot = saved;
        Ok(())
    }

    /// Evaluate a call reference once, preserving WithBaseObject as `this`.
    fn call_callee(&mut self, callee: &Expr) -> Result<bool, SyntaxError> {
        let saved = self.cur().next_slot;
        let method = if let ExprKind::Identifier(name) = &callee.kind
            && let access = self.resolve_reference(name)
            && matches!(access, Access::Dynamic(_))
        {
            let prepared = self.prepare_reference(access, callee.span)?;
            let Access::Prepared { slot, .. } = prepared else {
                unreachable!()
            };
            let receiver_done = self.out().label();
            self.out().op_u16(Opcode::GetLocal, slot, callee.span);
            self.out().op(Opcode::Dup, callee.span);
            self.out().op(Opcode::IsCell, callee.span);
            self.out()
                .branch(Opcode::JumpIfFalse, receiver_done, callee.span);
            self.out().op(Opcode::Pop, callee.span);
            self.out().op(Opcode::Undefined, callee.span);
            self.out().bind(receiver_done);
            self.load(prepared, callee.span)?;
            true
        } else if callee.is_method_callee() {
            self.method_callee(callee)?;
            true
        } else {
            self.expr(callee)?;
            false
        };
        self.cur_mut().next_slot = saved;
        Ok(method)
    }

    #[inline(never)]
    fn call_arguments(
        &mut self,
        args: &[Expr],
        method: bool,
        span: Span,
        op_span: Span,
        direct: Option<Span>,
    ) -> Result<(), SyntaxError> {
        if args.len() > MAX_COUNT {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "more than 65,535 call arguments",
            ));
        }
        if args.iter().any(|a| matches!(a.kind, ExprKind::Spread(_))) {
            self.spread_arguments(args, span)?;
            if let Some(at) = direct {
                let site = self.eval_site(at)?;
                self.cur_mut().eval_sites.push(site);
            }
            self.out().op(
                if method {
                    Opcode::CallMethodSpread
                } else {
                    Opcode::CallSpread
                },
                op_span,
            );
            return Ok(());
        }
        for arg in args {
            self.expr(arg)?;
        }
        if let Some(at) = direct {
            let site = self.eval_site(at)?;
            self.cur_mut().eval_sites.push(site);
        }
        let call = if method {
            Opcode::CallMethod
        } else {
            Opcode::Call
        };
        self.out().op_u16(call, args.len() as u16, op_span);
        Ok(())
    }

    /// A function expression: a closure, bound to its own name inside if
    /// it has one.
    #[inline(never)]
    fn function_expr(&mut self, f: &Function) -> Result<(), SyntaxError> {
        let self_name = match &f.name {
            Some(name) => {
                self.check_not_reserved(name)?;
                Some((name.name.clone(), SelfKind::Expression))
            }
            None => None,
        };
        self.closure(f, self_name, None)
    }

    /// NamedEvaluation changes metadata, never the lexical self binding.
    #[inline(always)]
    fn named_expr(&mut self, e: &Expr, name: JsString) -> Result<(), SyntaxError> {
        if let ExprKind::Class(c) = &e.kind {
            return self.class_expr(c, Some(ClassName::Known(name)));
        }
        if let ExprKind::Function(f) = &e.kind
            && f.name.is_none()
        {
            self.closure(f, None, Some(name))
        } else {
            self.expr(e)
        }
    }

    /// Class names have an independent immutable cell. Keep it uninitialized
    /// through computed-key evaluation, then initialize it after all methods.
    #[inline(never)]
    fn class_expr(
        &mut self,
        class: &Class,
        inferred: Option<ClassName>,
    ) -> Result<(), SyntaxError> {
        let span = class.span;
        let outer_strict = self.cur().strict;
        let outer_expression = self.cur().strict_expression;
        self.cur_mut().strict_expression = outer_expression || !outer_strict;
        self.cur_mut().strict = true;
        let saved = self.cur().next_slot;
        self.cur_mut().scopes.push(Scope::new());
        // The keyword is a unique syntax occurrence separate from the outer
        // declaration's identifier; binding analysis keys both by occurrence.
        let inner = class.name.as_ref().map(|n| Ident {
            name: n.name.clone(),
            span: Span::new(span.start, span.start + 5),
        });
        if let Some(name) = &inner {
            self.declare(name, BindingKind::Const, false, true)?;
            let b = self.local(name)?;
            self.out().op(Opcode::NewFixedCell, span);
            self.out().op_u16(Opcode::SetLocal, b.slot, span);
        }
        // This inaccessible cell ties the constructor to its initializer
        // without exposing an own property or adding a new heap edge kind.
        let initializer = Ident {
            name: "*instance-fields*".into(),
            span: Span::new(span.end - 1, span.end),
        };
        self.declare(&initializer, BindingKind::Const, false, true)?;
        let binding = self.local(&initializer)?;
        self.out().op(Opcode::NewFixedCell, span);
        self.out().op_u16(Opcode::SetLocal, binding.slot, span);
        let ctor = self.temp(span)?;
        let proto = self.temp(span)?;
        let key = self.temp(span)?;
        let value = self.temp(span)?;
        let computed = match &inferred {
            Some(ClassName::Computed(slot)) => Some(*slot),
            _ => None,
        };
        let name = class
            .name
            .as_ref()
            .map(|n| JsString::from(n.name.as_str()))
            .or(match inferred {
                Some(ClassName::Known(name)) => Some(name),
                _ => None,
            })
            .unwrap_or_else(|| JsString::from(""));
        let heritage = if let Some(parent) = &class.heritage {
            let parent_slot = self.temp(span)?;
            self.expr(parent)?;
            self.out()
                .op(Opcode::CheckHeritage, class.heritage_span.unwrap_or(span));
            self.out().op_u16(Opcode::SetLocal, parent_slot, span);
            Some(parent_slot)
        } else {
            None
        };
        // Private names belong to the class body, not its heritage. Allocate
        // all identities before creating any member closure (forward references).
        let mut private_names = HashMap::new();
        for element in &class.elements {
            let ClassElement::Member(element) = element else {
                continue;
            };
            let p = &element.property;
            let PropertyName::Identifier(name) = &p.name else {
                continue;
            };
            if !name.starts_with('#') {
                continue;
            }
            let kind = if element.is_field {
                0
            } else {
                match &p.value.kind {
                    ExprKind::Function(f) if f.kind == FunctionKind::Getter => 1,
                    ExprKind::Function(f) if f.kind == FunctionKind::Setter => 2,
                    _ => 0,
                }
            };
            if let Some((previous_static, previous_kind)) = private_names.get_mut(name) {
                if *previous_static != element.is_static
                    || (*previous_kind | kind) != 3
                    || *previous_kind == 3
                    || kind == 0
                {
                    return Err(self.error(
                        SyntaxErrorKind::Invalid,
                        p.name_span,
                        "duplicate private name",
                    ));
                }
                *previous_kind = 3;
                continue;
            }
            private_names.insert(name.clone(), (element.is_static, kind));
            let ident = Ident {
                name: name.clone(),
                span: p.name_span,
            };
            self.declare(&ident, BindingKind::Const, false, false)?;
            let binding = self.local(&ident)?;
            if binding.repr == Repr::Cell {
                self.out().op(Opcode::NewFixedCell, p.name_span);
                self.out()
                    .op_u16(Opcode::SetLocal, binding.slot, p.name_span);
            }
            let index = self.name_index(name, p.name_span)?;
            self.out()
                .op_u16(Opcode::NewPrivateName, index, p.name_span);
            self.pattern_store(&Pattern::ident(ident), true, false)?;
        }
        self.class_text = Some(class.span);
        self.closure(&class.constructor, None, Some(name))?;
        if let Some(parent) = heritage {
            self.out().op_u16(Opcode::GetLocal, parent, span);
            self.out().op(Opcode::Dup, span);
            self.constant(Primitive::Null, span)?;
            self.out().op(Opcode::StrictEq, span);
            let non_null = self.out().label();
            let ready = self.out().label();
            self.out().branch(Opcode::JumpIfFalse, non_null, span);
            self.out().op(Opcode::Undefined, span);
            self.out().branch(Opcode::Jump, ready, span);
            self.out().bind(non_null);
            self.out().op(Opcode::Dup, span);
            let prototype = self.name_index("prototype", span)?;
            self.out().op_u16(
                Opcode::GetField,
                prototype,
                class.heritage_span.unwrap_or(span),
            );
            self.out().bind(ready);
            self.out()
                .op(Opcode::InitHeritage, class.heritage_span.unwrap_or(span));
        }
        self.out().op_u16(Opcode::SetLocal, ctor, span);
        if let Some(key) = computed {
            self.out().op_u16(Opcode::GetLocal, key, span);
            self.out().op_u16(Opcode::GetLocal, ctor, span);
            let prefix = self.name_index("", span)?;
            self.out().op_u16(Opcode::SetComputedName, prefix, span);
            self.out().op(Opcode::Pop, span);
            self.out().op(Opcode::Pop, span);
        }
        self.out().op_u16(Opcode::GetLocal, ctor, span);
        let prototype = self.name_index("prototype", span)?;
        self.out().op_u16(Opcode::GetField, prototype, span);
        self.out().op_u16(Opcode::SetLocal, proto, span);
        let mut instance_fields = Vec::new();
        let mut static_fields = Vec::new();
        let mut instance_private_methods = Vec::new();
        let mut static_private_methods = Vec::new();
        for (index, element) in class.elements.iter().enumerate() {
            let element = match element {
                ClassElement::Member(element) => element,
                ClassElement::StaticBlock(block) => {
                    static_fields.push(ClassInitializer::StaticBlock(block));
                    continue;
                }
            };
            let p = &element.property;
            let at = p.name_span;
            self.out().op_u16(
                Opcode::GetLocal,
                if element.is_static { ctor } else { proto },
                at,
            );
            match &p.name {
                PropertyName::Computed(e, _) => {
                    self.expr(e)?;
                    self.out().op(Opcode::ToPropertyKey, at);
                }
                PropertyName::Identifier(n) => {
                    if n.starts_with('#') {
                        self.private_name(n, at)?;
                    } else {
                        self.constant(Primitive::String(JsString::from(n.as_str())), at)?;
                    }
                }
                PropertyName::String(n) => {
                    self.constant(Primitive::String(n.clone()), at)?;
                }
                PropertyName::Number(n) => {
                    self.constant(
                        Primitive::String(JsString::from(number_to_string(*n).as_str())),
                        at,
                    )?;
                }
            }
            self.out().op_u16(Opcode::SetLocal, key, at);
            if element.is_field {
                if let PropertyName::Identifier(name) = &p.name
                    && name.starts_with('#')
                {
                    self.out().op(Opcode::Pop, at); // unused home object
                    let field = ClassInitializer::Member(
                        p,
                        Ident {
                            name: name.clone(),
                            span: at,
                        },
                        true,
                    );
                    if element.is_static {
                        static_fields.push(field);
                    } else {
                        instance_fields.push(field);
                    }
                    continue;
                }
                let field_key = Ident {
                    name: format!("*field-key-{index}*"),
                    span: at,
                };
                self.declare(&field_key, BindingKind::Const, false, true)?;
                let binding = self.local(&field_key)?;
                self.out().op(Opcode::NewFixedCell, at);
                self.out().op_u16(Opcode::SetLocal, binding.slot, at);
                self.out().op_u16(Opcode::GetLocal, key, at);
                self.pattern_store(&Pattern::ident(field_key.clone()), true, false)?;
                self.out().op(Opcode::Pop, at); // unused home object
                if element.is_static {
                    static_fields.push(ClassInitializer::Member(p, field_key, true));
                } else {
                    instance_fields.push(ClassInitializer::Member(p, field_key, true));
                }
                continue;
            }
            self.expr(&p.value)?;
            self.out().op_u16(Opcode::SetLocal, value, at);
            self.out().op_u16(Opcode::GetLocal, key, at);
            self.out().op_u16(Opcode::GetLocal, value, at);
            let ExprKind::Function(f) = &p.value.kind else {
                return Err(self.internal(at, "class element is not a method"));
            };
            let (prefix, opcode) = match f.kind {
                FunctionKind::Getter => ("get ", Opcode::DefineClassGetter),
                FunctionKind::Setter => ("set ", Opcode::DefineClassSetter),
                _ => ("", Opcode::DefineClassMethod),
            };
            if let PropertyName::Identifier(name) = &p.name
                && name.starts_with('#')
            {
                // Name metadata uses the spelling, never a public symbol name.
                self.out().op(Opcode::Pop, at);
                self.out().op(Opcode::Pop, at);
                self.constant(Primitive::String(JsString::from(name.as_str())), at)?;
                self.out().op_u16(Opcode::GetLocal, value, at);
                let prefix = self.name_index(prefix, at)?;
                self.out().op_u16(Opcode::SetComputedName, prefix, at);
                let method = Ident {
                    name: format!("*private-method-{index}*"),
                    span: Span::new(at.end - 1, at.end),
                };
                self.declare(&method, BindingKind::Const, false, true)?;
                let binding = self.local(&method)?;
                self.out().op(Opcode::NewFixedCell, at);
                self.out().op_u16(Opcode::SetLocal, binding.slot, at);
                self.pattern_store(&Pattern::ident(method.clone()), true, false)?;
                self.out().op(Opcode::Pop, at); // spelling
                self.out().op(Opcode::Pop, at); // home object
                if element.is_static {
                    static_private_methods.push(ClassInitializer::Member(p, method, false));
                } else {
                    instance_private_methods.push(ClassInitializer::Member(p, method, false));
                }
                continue;
            }
            let prefix = self.name_index(prefix, at)?;
            self.out().op_u16(Opcode::SetComputedName, prefix, at);
            self.out().op(opcode, at);
            self.out().op(Opcode::Pop, at);
        }
        self.out().op_u16(Opcode::GetLocal, ctor, span);
        if let Some(name) = &inner {
            self.out().op(Opcode::Dup, span);
            self.pattern_store(&Pattern::ident(name.clone()), true, false)?;
        }
        let function = Function::new(
            FunctionKind::Method,
            None,
            vec![],
            vec![],
            vec![],
            span,
            true,
        );
        // All private methods/accessors are branded before the first field.
        instance_private_methods.append(&mut instance_fields);
        instance_fields = instance_private_methods;
        static_private_methods.append(&mut static_fields);
        static_fields = static_private_methods;
        self.out().op_u16(Opcode::GetLocal, proto, span);
        self.closure_with_fields(&function, None, None, Some(&instance_fields))?;
        self.pattern_store(&Pattern::ident(initializer), true, false)?;
        self.out().op(Opcode::Pop, span); // initializer home object
        if !static_fields.is_empty() {
            self.out().op_u16(Opcode::GetLocal, ctor, span);
            self.closure_with_fields(&function, None, None, Some(&static_fields))?;
            self.out().op_u16(Opcode::CallMethod, 0, span);
            self.out().op(Opcode::Pop, span);
        }
        // Clear scratch roots, including the inner cell (closures retain it).
        for slot in saved..self.cur().next_slot {
            self.out().op(Opcode::Undefined, span);
            self.out().op_u16(Opcode::SetLocal, slot as u16, span);
        }
        self.exit_scope(saved);
        self.cur_mut().strict = outer_strict;
        self.cur_mut().strict_expression = outer_expression;
        Ok(())
    }

    fn strict_op(&self, opcode: Opcode) -> Opcode {
        if !self.cur().strict_expression {
            return opcode;
        }
        match opcode {
            Opcode::PutGlobal => Opcode::PutGlobalStrict,
            Opcode::PutField => Opcode::PutFieldStrict,
            Opcode::PutElem => Opcode::PutElemStrict,
            Opcode::PutSuperElem => Opcode::PutSuperElemStrict,
            Opcode::Delete => Opcode::DeleteStrict,
            _ => opcode,
        }
    }

    /// `left, right`: left to right; only the last operand's value is kept.
    #[inline(never)]
    fn sequence(&mut self, left: &Expr, right: &Expr, span: Span) -> Result<(), SyntaxError> {
        self.expr(left)?;
        self.out().op(Opcode::Pop, span);
        self.expr(right)
    }

    /// `new callee(args)` (clj-quickjs-71; ECMAScript EvaluateNew, QuickJS
    /// `OP_call_constructor`): the callee, then the arguments, then
    /// `Construct` at the `new` token, which checks the callee is a
    /// constructor. Out of line, like [`Self::get_field`].
    #[inline(never)]
    fn new_expr(&mut self, e: &Expr) -> Result<(), SyntaxError> {
        let ExprKind::New {
            callee,
            args,
            op_span,
        } = &e.kind
        else {
            return Err(self.internal(e.span, "not a new expression"));
        };
        let (span, op_span) = (e.span, *op_span);
        if args.len() > MAX_COUNT {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "more than 65,535 constructor arguments",
            ));
        }
        self.expr(callee)?;
        if args.iter().any(|a| matches!(a.kind, ExprKind::Spread(_))) {
            self.spread_arguments(args, span)?;
            self.construct_callee(callee);
            self.out().op(Opcode::ConstructSpread, op_span);
            return Ok(());
        }
        for arg in args {
            self.expr(arg)?;
        }
        self.construct_callee(callee);
        self.out()
            .op_u16(Opcode::Construct, args.len() as u16, op_span);
        Ok(())
    }

    fn construct_callee(&mut self, callee: &Expr) {
        if matches!(
            callee.kind,
            ExprKind::Identifier(_) | ExprKind::Member { .. } | ExprKind::Index { .. }
        ) {
            let out = self.out();
            out.construct_callees.insert(out.spans.len(), callee.span);
        }
    }

    /// An untagged template literal (clj-quickjs-57; ECMAScript
    /// TemplateLiteral evaluation, QuickJS `js_parse_template`): its first
    /// text part, then per substitution its value, `ToString` (reported at
    /// the substitution) and `Plus` onto the string so far, then the next
    /// text part unless it is empty. Each substitution is converted before
    /// the next one is evaluated. `Plus` of two strings is concatenation, so
    /// the only conversion is the substitution's ToString. An empty first
    /// part before a substitution is left out: the converted substitution
    /// is already the string so far. Out of line, like
    /// [`Self::get_field`].
    #[inline(never)]
    fn template(
        &mut self,
        quasis: &[TemplateElement],
        exprs: &[Expr],
        span: Span,
    ) -> Result<(), SyntaxError> {
        let Some((first, rest)) = quasis.split_first() else {
            return Err(self.internal(span, "a template literal without parts"));
        };
        let first_cooked = first
            .cooked
            .as_ref()
            .ok_or_else(|| self.internal(span, "invalid untagged template"))?;
        if exprs.is_empty() || !first_cooked.is_empty() {
            self.constant(Primitive::String(first_cooked.clone()), first.span)?;
        }
        for (i, (expr, part)) in exprs.iter().zip(rest).enumerate() {
            self.expr(expr)?;
            self.out().op(Opcode::ToString, expr.span);
            if i > 0 || !first_cooked.is_empty() {
                self.out().op(Opcode::Plus, expr.span);
            }
            let cooked = part
                .cooked
                .as_ref()
                .ok_or_else(|| self.internal(span, "invalid untagged template"))?;
            if !cooked.is_empty() {
                self.constant(Primitive::String(cooked.clone()), part.span)?;
                self.out().op(Opcode::Plus, part.span);
            }
        }
        Ok(())
    }

    /// GetTemplateObject precedes substitution evaluation. The tag reference
    /// uses exactly the receiver preservation of an ordinary member call.
    #[inline(never)]
    fn tagged_template(
        &mut self,
        tag: &Expr,
        quasis: &[TemplateElement],
        exprs: &[Expr],
        span: Span,
    ) -> Result<(), SyntaxError> {
        if quasis.len() > MAX_COUNT || exprs.len() + 1 > MAX_COUNT {
            return Err(self.error(
                SyntaxErrorKind::TooLarge,
                span,
                "more than 65,535 template arguments",
            ));
        }
        let method = self.call_callee(tag)?;
        for part in quasis {
            match &part.cooked {
                Some(s) => self.constant(Primitive::String(s.clone()), part.span)?,
                None => self.out().op(Opcode::Undefined, part.span),
            }
            self.constant(Primitive::String(part.raw.clone()), part.span)?;
        }
        let op_span = quasis[0].span;
        self.out()
            .op_u16(Opcode::TemplateObject, quasis.len() as u16, op_span);
        for expr in exprs {
            self.expr(expr)?;
        }
        self.out().op_u16(
            if method {
                Opcode::CallMethod
            } else {
                Opcode::Call
            },
            (exprs.len() + 1) as u16,
            op_span,
        );
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

    /// Lower `__rt.<property>(args)` through the intrinsic table. Compile
    /// errors are reported at the call (`span`); the operation runs at its
    /// `(` token (`at`), as a call does.
    fn intrinsic(
        &mut self,
        property: &Ident,
        args: &[Expr],
        span: Span,
        at: Span,
    ) -> Result<(), SyntaxError> {
        let name = &property.name;
        if let Some(arg) = args.iter().find(|a| matches!(a.kind, ExprKind::Spread(_))) {
            return Err(self.error(
                SyntaxErrorKind::Unsupported,
                arg.span,
                "spread arguments to compiler intrinsics are not supported",
            ));
        }
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
            Arity::AtLeast(k) if n < k => Some(format!(
                "`{RESERVED_BINDING}.{name}` takes at least {k} arguments, got {n}"
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
            Lowering::Call => {
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op_u16(Opcode::CallMethod, (n - 2) as u16, at);
            }
            Lowering::Op(op) => {
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op(op, at);
            }
            Lowering::OpThenPop(op) => {
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op(op, at);
                self.out().op(Opcode::Pop, at);
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
                self.out().op_u16(op, count as u16, at);
            }
            Lowering::FormOp(form_op) => {
                for arg in args {
                    self.expr(arg)?;
                }
                self.out().op_u16(Opcode::FormOp, form_op as u16, at);
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
                self.out().op_u16(Opcode::HostEffect, selector, at);
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
                self.out().op(Opcode::Need, at);
            }
        }
        Ok(())
    }

    /// `typeof name`: the operand is a reference (parentheses included),
    /// so a name with no binding is `undefined` rather than a
    /// ReferenceError (QuickJS `OP_get_var_undef`). A global name is looked
    /// up at run time. A binding in its temporal dead zone still throws, as
    /// a plain read does. Out of line, so the recursive [`Self::expr`]
    /// frame stays small.
    #[inline(never)]
    fn typeof_reference(&mut self, operand: &Expr, span: Span) -> Result<(), SyntaxError> {
        let ExprKind::Identifier(name) = &operand.kind else {
            return Err(self.internal(operand.span, "typeof operand is not a name"));
        };
        let at = self.name_span(name, operand.span);
        let access = self.resolve_reference(name);
        self.load_reference(access, at, true)?;
        self.out().op(Opcode::Typeof, span);
        Ok(())
    }

    /// `void operand`: the operand's value (on the stack) is discarded and
    /// the expression gives `undefined` (QuickJS `js_parse_unary`,
    /// `TOK_VOID`: `drop` then `undefined`). Out of line, so the recursive
    /// [`Self::expr`] frame stays small.
    #[inline(never)]
    fn void_result(&mut self, span: Span) {
        self.out().op(Opcode::Pop, span);
        self.out().op(Opcode::Undefined, span);
    }

    /// Emit a binary operator; both operands are on the stack.
    /// `left_number` / `right_number`: the operand is known to be a Number
    /// ([`is_number`]), so it needs no ToNumber.
    fn binary(&mut self, op: BinaryOp, left_number: bool, right_number: bool, span: Span) {
        let numeric = match op {
            BinaryOp::Mul => Opcode::Mul,
            BinaryOp::Div => Opcode::Div,
            BinaryOp::Rem => Opcode::Rem,
            // `+` is numeric addition only when both operands are numbers;
            // otherwise `plus` decides at run time (strings concatenate) and
            // no ToNumber may run first.
            BinaryOp::Add if left_number && right_number => Opcode::Add,
            BinaryOp::Add => {
                self.out().op(Opcode::Plus, span);
                return;
            }
            BinaryOp::Sub => Opcode::Sub,
            // Preserve an unknown operand's type for IsLessThan, including
            // exact comparison of BigInt with Number.
            BinaryOp::Lt | BinaryOp::Gt | BinaryOp::Le | BinaryOp::Ge
                if !left_number || !right_number =>
            {
                let compare = match op {
                    BinaryOp::Lt => Opcode::CompareLt,
                    BinaryOp::Gt => Opcode::CompareGt,
                    BinaryOp::Le => Opcode::CompareLe,
                    _ => Opcode::CompareGe,
                };
                self.out().op(compare, span);
                return;
            }
            BinaryOp::Lt => Opcode::Lt,
            BinaryOp::Gt => Opcode::Gt,
            BinaryOp::Le => Opcode::Le,
            BinaryOp::Ge => Opcode::Ge,
            BinaryOp::BitAnd => Opcode::BitAnd,
            BinaryOp::BitOr => Opcode::BitOr,
            BinaryOp::BitXor => Opcode::BitXor,
            BinaryOp::Shl => Opcode::Shl,
            BinaryOp::Sar => Opcode::Sar,
            BinaryOp::UShr => Opcode::UShr,
            BinaryOp::Exp => Opcode::Pow,
            // Operands of any type, no conversion (clj-quickjs-75).
            BinaryOp::Instanceof => {
                self.out().op(Opcode::Instanceof, span);
                return;
            }
            // Operands of any type; the key is converted after the base is
            // checked (clj-quickjs-74).
            BinaryOp::In => {
                self.out().op(Opcode::In, span);
                return;
            }
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
        // Both operands must reach ToNumeric together, even with a known
        // Number on the left: a BigInt on the right is a mixed-type error.
        // Conversion still follows evaluation of both operands.
        if !left_number || !right_number {
            self.out().op(Opcode::ToNumberPair, span);
        }
        self.out().op(numeric, span);
    }
}

/// Append the names `var` statements of `body` declare, in source order,
/// looking into nested statements but not nested functions. Recursion depth
/// is bounded by the tree height.
fn var_names<'a>(body: &'a [Stmt], out: &mut Vec<&'a Ident>) {
    for stmt in body {
        var_names_in(stmt, out);
    }
}

fn var_names_in<'a>(stmt: &'a Stmt, out: &mut Vec<&'a Ident>) {
    match &stmt.kind {
        StmtKind::Var(declarators) => out.extend(declarators.iter().flat_map(|d| d.name.names())),
        StmtKind::For { init, body, .. } => {
            if let Some(ForInit::Var(declarators)) = init {
                out.extend(declarators.iter().flat_map(|d| d.name.names()));
            }
            var_names_in(body, out);
        }
        StmtKind::ForIn { head, body, .. } | StmtKind::ForOf { head, body, .. } => {
            if let ForInHead::Var(name, _) = head {
                out.push(name);
            }
            if let ForInHead::Pattern(None, target) = head {
                out.extend(target.names());
            }
            var_names_in(body, out);
        }
        StmtKind::Block(body) => var_names(body, out),
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            var_names_in(consequent, out);
            if let Some(alternate) = alternate {
                var_names_in(alternate, out);
            }
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::Labelled { body, .. }
        | StmtKind::With { body, .. } => var_names_in(body, out),
        StmtKind::Try {
            block,
            handler,
            finalizer,
        } => {
            var_names(block, out);
            if let Some(handler) = handler {
                var_names(&handler.body, out);
            }
            if let Some(finalizer) = finalizer {
                var_names(finalizer, out);
            }
        }
        StmtKind::Switch { cases, .. } => {
            for case in cases {
                var_names(&case.body, out);
            }
        }
        StmtKind::FunctionDecl(_)
        | StmtKind::Lexical(_)
        | StmtKind::Expr(_)
        | StmtKind::Return(_)
        | StmtKind::Throw(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Empty => {}
    }
}

/// The plain (not generator or async) function declarations of a sloppy
/// body's blocks and case clauses, nested functions excluded, that
/// ECMAScript Annex B.3.3 also binds with `var`: those that a `var`
/// declaration of their name could replace without an early error, so no
/// enclosing block, loop head or catch binding, nor the body's top level
/// (`clauses`), declares the name lexically. A simple catch parameter does
/// not count (Annex B.3.4, "VariableStatements in Catch Blocks"); a
/// destructuring one does. In source order; names may repeat.
fn annex_b_functions<'a>(clauses: &[&'a [Stmt]], out: &mut Vec<&'a Function>) {
    let mut lexical = Vec::new();
    for clause in clauses {
        lexical_names(clause, false, &mut lexical);
    }
    for clause in clauses {
        for stmt in clause.iter() {
            annex_b_in(stmt, &mut lexical, out);
        }
    }
}

/// The names `body` declares with `let`, `const`, `using` or `class`, and
/// with function declarations if `functions`.
fn lexical_names<'a>(body: &'a [Stmt], functions: bool, out: &mut Vec<&'a str>) {
    for stmt in body {
        match &stmt.kind {
            StmtKind::Lexical(decl) => out.extend(
                decl.declarators
                    .iter()
                    .flat_map(|d| d.name.names())
                    .map(|n| n.name.as_str()),
            ),
            StmtKind::FunctionDecl(f) if functions => {
                out.extend(f.name.as_ref().map(|n| n.name.as_str()))
            }
            _ => {}
        }
    }
}

/// [`annex_b_functions`] for a block (or a `switch`'s case clauses), inside
/// enclosing blocks that declare the names `lexical`.
fn annex_b_block<'a>(
    clauses: &[&'a [Stmt]],
    lexical: &mut Vec<&'a str>,
    out: &mut Vec<&'a Function>,
) {
    let depth = lexical.len();
    for stmt in clauses.iter().flat_map(|c| c.iter()) {
        if let StmtKind::FunctionDecl(f) = &stmt.kind
            && !f.generator
            && !f.asynchronous
            && let Some(name) = &f.name
            && !lexical.contains(&name.name.as_str())
        {
            out.push(f);
        }
    }
    for clause in clauses {
        lexical_names(clause, true, lexical);
    }
    for stmt in clauses.iter().flat_map(|c| c.iter()) {
        annex_b_in(stmt, lexical, out);
    }
    lexical.truncate(depth);
}

fn annex_b_in<'a>(stmt: &'a Stmt, lexical: &mut Vec<&'a str>, out: &mut Vec<&'a Function>) {
    let depth = lexical.len();
    match &stmt.kind {
        StmtKind::Block(body) => annex_b_block(&[body], lexical, out),
        StmtKind::Switch { cases, .. } => {
            let clauses: Vec<&[Stmt]> = cases.iter().map(|c| c.body.as_slice()).collect();
            annex_b_block(&clauses, lexical, out);
        }
        StmtKind::If {
            consequent,
            alternate,
            ..
        } => {
            annex_b_in(consequent, lexical, out);
            if let Some(alternate) = alternate {
                annex_b_in(alternate, lexical, out);
            }
        }
        StmtKind::While { body, .. }
        | StmtKind::DoWhile { body, .. }
        | StmtKind::Labelled { body, .. }
        | StmtKind::With { body, .. } => annex_b_in(body, lexical, out),
        StmtKind::For { init, body, .. } => {
            if let Some(ForInit::Lexical(decl)) = init {
                lexical.extend(
                    decl.declarators
                        .iter()
                        .flat_map(|d| d.name.names())
                        .map(|n| n.name.as_str()),
                );
            }
            annex_b_in(body, lexical, out);
        }
        StmtKind::ForIn { head, body, .. } | StmtKind::ForOf { head, body, .. } => {
            match head {
                ForInHead::Lexical(_, name) => lexical.push(name.name.as_str()),
                ForInHead::Pattern(Some(_), pattern) => {
                    lexical.extend(pattern.names().into_iter().map(|n| n.name.as_str()))
                }
                _ => {}
            }
            annex_b_in(body, lexical, out);
        }
        StmtKind::Try {
            block,
            handler,
            finalizer,
        } => {
            annex_b_block(&[block], lexical, out);
            if let Some(handler) = handler {
                if let Some(pattern) = handler.param.as_ref().filter(|p| p.identifier().is_none()) {
                    lexical.extend(pattern.names().into_iter().map(|n| n.name.as_str()));
                }
                annex_b_block(&[&handler.body], lexical, out);
                lexical.truncate(depth);
            }
            if let Some(finalizer) = finalizer {
                annex_b_block(&[finalizer], lexical, out);
            }
        }
        StmtKind::FunctionDecl(_)
        | StmtKind::Lexical(_)
        | StmtKind::Var(_)
        | StmtKind::Expr(_)
        | StmtKind::Return(_)
        | StmtKind::Throw(_)
        | StmtKind::Break(_)
        | StmtKind::Continue(_)
        | StmtKind::Empty => {}
    }
    lexical.truncate(depth);
}

/// True when `e` always evaluates to a Number (if it completes), so no
/// ToNumber is needed.
fn is_number(e: &Expr) -> bool {
    match &e.kind {
        ExprKind::Number(_) => true,
        ExprKind::Unary {
            op: UnaryOp::Plus, ..
        } => true,
        ExprKind::Unary {
            op: UnaryOp::Neg | UnaryOp::BitNot,
            operand,
            ..
        } => is_number(operand),
        // `+` concatenates when an operand is a string, so it is a number
        // only when both operands are.
        ExprKind::Binary {
            op: BinaryOp::Add,
            left,
            right,
            ..
        } => is_number(left) && is_number(right),
        ExprKind::Binary {
            op, left, right, ..
        } => {
            is_number(left)
                && is_number(right)
                && matches!(
                    op,
                    BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Rem
                        | BinaryOp::Sub
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::Shl
                        | BinaryOp::Sar
                        | BinaryOp::UShr
                        | BinaryOp::Exp
                )
        }
        ExprKind::Conditional {
            consequent,
            alternate,
            ..
        } => is_number(consequent) && is_number(alternate),
        ExprKind::Assign { value, .. }
        | ExprKind::AssignMember { value, .. }
        | ExprKind::AssignIndex { value, .. } => is_number(value),
        // An unknown binding can hold BigInt; only unsigned shift is
        // guaranteed to return Number when it completes.
        ExprKind::CompoundAssign { op, .. } | ExprKind::CompoundAssignProperty { op, .. } => {
            *op == BinaryOp::UShr
        }
        ExprKind::Sequence { right, .. } => is_number(right),
        ExprKind::Update { .. } | ExprKind::UpdateProperty { .. } => false,
        _ => false,
    }
}

/// Numeric expressions need no second ToNumeric before unary minus, even
/// when their result may be a BigInt. Unary plus still needs ToNumber.
fn is_numeric(e: &Expr) -> bool {
    is_number(e)
        || matches!(
            &e.kind,
            ExprKind::BigInt(_)
                | ExprKind::Unary {
                    op: UnaryOp::Neg | UnaryOp::BitNot,
                    ..
                }
                | ExprKind::Update { .. }
                | ExprKind::UpdateProperty { .. }
                | ExprKind::Binary {
                    op: BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::Rem
                        | BinaryOp::Exp
                        | BinaryOp::BitAnd
                        | BinaryOp::BitOr
                        | BinaryOp::BitXor
                        | BinaryOp::Shl
                        | BinaryOp::Sar
                        | BinaryOp::UShr,
                    ..
                }
        )
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
