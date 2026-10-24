//! Immutable, shared executable code and its load-time validation.
//!
//! [`Executable::load`] decodes the byte format of [`super::instr`] and
//! rejects malformed code before any task can run it: unknown opcodes,
//! truncated operands, out-of-range constant/local indices and `ThrowError`
//! kinds, branch targets that are not instruction boundaries, operand-stack
//! underflow, inconsistent stack depth where control flow merges, and control
//! falling off the end. Code compiled from source
//! ([`crate::frontend::compile_script`]) passes the same validation; only its
//! per-instruction source locations are added beside it.
//! The stack check also computes the maximum operand-stack depth, in the
//! spirit of QuickJS's `compute_stack_size` (`research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8); the algorithm here is written fresh.
//!
//! A loaded [`Executable`] is an `Arc` handle: cloning it or spawning tasks
//! from it never copies the code. Constants are primitives only in this slice,
//! so an executable holds no heap edges and is not tied to one runtime.
//! BigInt constants carry the private, normalized `JsBigInt` type: its only
//! constructors enforce the magnitude cap, so a loaded constant cannot
//! bypass that limit. Source overflow is rejected as frontend `TooLarge`.
//!
//! **Function code.** Each executable is the code of one function (the task
//! entry is just the function a task starts in). Its child function table
//! ([`ExecutableSpec::functions`]) holds already-validated executables by
//! `Arc`, so a function tree is loaded bottom-up, each child exactly once,
//! and is acyclic; `Function` operands are checked against the table.
//! Recursion therefore goes through function *values* (`CurrentFunction`, or
//! a function passed as an argument or captured by a closure), never through
//! a code cycle. A child also declares how many values a closure over it
//! captures ([`ExecutableSpec::captures`]); `Function` operands are decoded
//! with that count and `GetCapture` operands are checked against it.

use std::fmt;
use std::sync::Arc;

use super::instr::{Instr, Opcode, OperandKind, ThrowKind};
use crate::error::SourceLocation;
use crate::heap::gc::Trace;
use crate::value::{JsString, Primitive, Value};

/// The shared code's `[[Construct]]` capability. Further constructor kinds
/// (such as class constructors) can extend this independently of closures.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
#[non_exhaustive]
pub enum ConstructorKind {
    /// Script code or a callable without `[[Construct]]`.
    #[default]
    None,
    /// An ordinary function declaration or expression.
    Ordinary,
    /// A base class, requiring construction rather than an ordinary call.
    BaseClass,
    /// A class whose receiver is initialized by super().
    DerivedClass,
    /// A synthesized derived constructor forwarding all actual arguments.
    DefaultDerivedClass,
}

impl ConstructorKind {
    pub(crate) fn is_class(self) -> bool {
        matches!(
            self,
            Self::BaseClass | Self::DerivedClass | Self::DefaultDerivedClass
        )
    }

    pub(crate) fn is_derived(self) -> bool {
        matches!(self, Self::DerivedClass | Self::DefaultDerivedClass)
    }
    /// Whether this kind provides `[[Construct]]`.
    pub fn is_constructor(self) -> bool {
        !matches!(self, Self::None)
    }
}

/// A caller value exposed to direct eval. These are validated frame indices,
/// never pointers into a worker's stack.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalSource {
    Local(u16),
    Capture(u16),
    CurrentFunction,
    This,
    GlobalObject,
    NewTarget,
}

/// Assignment semantics retained when compiling against a caller's scope.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EvalBindingKind {
    Mutable,
    Const,
    FunctionName,
    Import,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalBinding {
    pub name: String,
    pub source: EvalSource,
    pub kind: EvalBindingKind,
    pub cell: bool,
    /// Number of nearer dynamic variable environments to consult first.
    pub environments: u16,
}

/// The lexical scope at a syntactic `eval(...)` call. Offsets refer to the
/// encoded Call/CallSpread instruction, and survive executable round trips.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EvalSite {
    pub offset: u32,
    /// Call-site mode, including strict class keys in a sloppy activation.
    pub strict: bool,
    pub bindings: Vec<EvalBinding>,
    pub receiver: EvalSource,
    /// Originating ordinary function's mode, independent of eval directives.
    pub receiver_strict: bool,
    /// None at script scope; arrows and eval inherit the enclosing function's
    /// binding, whose value can itself be undefined on an ordinary call.
    pub new_target: Option<EvalSource>,
    /// Dynamic variable environments, nearest first. Each source holds a
    /// task-owned cell containing a private null-prototype binding object.
    pub environments: Vec<EvalSource>,
    /// None denotes the global variable environment.
    pub variable_environment: Option<EvalSource>,
    /// Statically allocated bindings in the caller's variable environment.
    pub variable_bindings: Vec<String>,
    /// All intervening lexical declarations, including shadowed ones.
    pub lexical_conflicts: Vec<String>,
}

/// Input to [`Executable::load`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutableSpec {
    /// Name used in diagnostics.
    pub name: String,
    /// Number of parameters; arguments are copied into locals `0..params`.
    pub params: u16,
    /// JavaScript `length`; `None` uses `params` for hand-assembled code.
    pub function_length: Option<u16>,
    /// JavaScript `name`; `None` uses the diagnostic `name`. Source code
    /// supplies an explicit name (including the empty anonymous name).
    pub function_name: Option<JsString>,
    /// Number of locals (parameters included).
    pub locals: u16,
    /// Encoded instructions.
    pub code: Vec<u8>,
    /// Constant pool referenced by `Const`.
    pub constants: Vec<Primitive>,
    /// Child function code referenced by `Function` (its u16 operand is an
    /// index here). The handles are shared, not copied: the loaded code keeps
    /// the same `Arc`s, so every function object and every task running a
    /// child runs the one loaded copy.
    pub functions: Vec<Executable>,
    /// Number of captured values a closure over this code carries (its
    /// lexical environment, `crate::vm::function`): the parent's `Function`
    /// instruction pops this many, and `GetCapture` operands are checked
    /// against it. 0 for code that captures nothing, such as a task entry.
    pub captures: u16,
    /// The function is strict-mode code (clj-quickjs-95). Compiled code
    /// takes it from the source ([`crate::frontend::ScriptOptions`] and
    /// `"use strict"` directives); hand-assembled code is strict, the only
    /// mode before sloppy code existed. Instructions whose JavaScript
    /// semantics differ by mode branch on [`Executable::is_strict`] of the
    /// running code, or are chosen by the compiler from it.
    pub strict: bool,
    /// Constructor capability. Ordinary, non-generator, non-async function
    /// declarations/expressions use `Ordinary`; scripts, methods, accessors,
    /// arrows and hand-assembled code use `None`. Generator code also uses
    /// `None` despite having an own `prototype` (see `crate::vm::function`).
    pub constructor_kind: ConstructorKind,
    /// Direct-eval call sites; empty for hand-assembled code by default.
    pub eval_sites: Vec<EvalSite>,
}

/// Validated code shared by every task that runs it.
#[derive(Debug)]
pub(crate) struct Code {
    pub(crate) name: Arc<str>,
    pub(crate) params: u16,
    pub(crate) function_length: u16,
    pub(crate) function_name: JsString,
    pub(crate) locals: u16,
    pub(crate) constants: Box<[Value]>,
    pub(crate) regexps: std::collections::HashMap<
        u16,
        Result<Arc<crate::regexp::object::Compiled>, crate::regexp::object::CompileError>,
    >,
    /// Child function code, shared with the [`Executable`]s it was loaded from.
    pub(crate) functions: Box<[Arc<Code>]>,
    /// Number of captured values a closure over this code carries.
    pub(crate) captures: u16,
    /// Strict-mode code ([`ExecutableSpec::strict`]).
    pub(crate) strict: bool,
    /// An ordinary function ([`ExecutableSpec::constructor_kind`]).
    pub(crate) constructor_kind: ConstructorKind,
    /// Derived from the bytecode when loaded, including spec round trips.
    pub(crate) uses_arguments: bool,
    pub(crate) instrs: Box<[Instr]>,
    /// Byte offset of each instruction, for diagnostics.
    pub(crate) offsets: Box<[u32]>,
    pub(crate) max_stack: usize,
    /// Source text and per-instruction source locations, for code compiled
    /// from JavaScript; `None` for code loaded from an [`ExecutableSpec`].
    pub(crate) debug: Option<DebugInfo>,
    /// Indexed by decoded instruction position; absent for ordinary calls.
    pub(crate) eval_sites: std::collections::HashMap<usize, EvalSite>,
}

impl Code {
    pub(crate) fn is_generator(&self) -> bool {
        matches!(self.instrs.first(), Some(Instr::GeneratorStart))
    }
}

/// Iterative teardown: a function tree may be nested arbitrarily deep (each
/// level is a separate load), so dropping the last handle to a chain must not
/// recurse once per level on the Rust stack. Child tables of uniquely owned
/// children are detached onto a worklist before the child itself is dropped.
impl Drop for Code {
    fn drop(&mut self) {
        let mut pending: Vec<Arc<Code>> = std::mem::take(&mut self.functions).into_vec();
        while let Some(child) = pending.pop() {
            if let Some(mut child) = Arc::into_inner(child) {
                pending.extend(std::mem::take(&mut child.functions).into_vec());
            }
        }
    }
}

/// Source information of compiled code. Diagnostic only: it never changes
/// what an instruction does.
#[derive(Debug)]
pub(crate) struct DebugInfo {
    /// Name supplied when the whole script/function tree was compiled.
    pub(crate) filename: Arc<str>,
    /// The compiled source text (failure messages quote binding names from it).
    pub(crate) source: Arc<str>,
    /// One location per instruction, in instruction order.
    pub(crate) locations: Box<[SourceLocation]>,
    /// Callee spans are independent of the `new` operator's error position.
    pub(crate) construct_callees: std::collections::HashMap<usize, crate::frontend::Span>,
}

impl Code {
    /// Source location of instruction `index`, if the code has debug info.
    pub(crate) fn location(&self, index: usize) -> Option<SourceLocation> {
        self.debug.as_ref()?.locations.get(index).copied()
    }

    /// The source text `location` covers, if the code has debug info.
    pub(crate) fn source_text(&self, location: SourceLocation) -> Option<&str> {
        let source = &self.debug.as_ref()?.source;
        source.get(location.span.start..location.span.end)
    }
}

/// Immutable executable code; a cheap-to-clone shared handle.
#[derive(Clone, Debug)]
pub struct Executable(pub(crate) Arc<Code>);

/// Identity, not structure: two handles are equal when they share the same
/// loaded code ([`Executable::same_code`]).
impl PartialEq for Executable {
    fn eq(&self, other: &Executable) -> bool {
        Executable::same_code(self, other)
    }
}

/// Why [`Executable::load`] rejected a spec.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    /// Byte offset of the offending instruction (0 for whole-spec errors).
    pub offset: u32,
    pub kind: ValidationErrorKind,
}

/// The specific validation failure.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ValidationErrorKind {
    EmptyCode,
    CodeTooLarge {
        len: usize,
    },
    ParamsExceedLocals {
        params: u16,
        locals: u16,
    },
    UnknownOpcode {
        byte: u8,
    },
    TruncatedOperand {
        opcode: Opcode,
    },
    ConstantOutOfRange {
        index: u16,
        count: usize,
    },
    LocalOutOfRange {
        index: u16,
        count: u16,
    },
    BadBranchTarget {
        target: u32,
    },
    StackUnderflow {
        opcode: Opcode,
        depth: usize,
    },
    StackDepthMismatch {
        expected: usize,
        found: usize,
    },
    FallsOffEnd,
    /// More constants than a u16 `Const` operand can address (65,536).
    ConstantPoolTooLarge {
        count: usize,
    },
    /// A `ThrowError` operand that names no [`ThrowKind`].
    UnknownThrowKind {
        code: u16,
    },
    /// A `Function` operand past the end of the child function table.
    FunctionOutOfRange {
        index: u16,
        count: usize,
    },
    /// More child functions than a u16 `Function` operand can address
    /// (65,536).
    FunctionTableTooLarge {
        count: usize,
    },
    /// A `GetCapture` operand at or past the code's declared captures.
    CaptureOutOfRange {
        index: u16,
        count: u16,
    },
    /// Compiled code whose source map does not have one location per
    /// instruction (a compiler bug; never produced by [`Executable::load`]).
    SourceMapMismatch {
        instructions: usize,
        locations: usize,
    },
    /// A property-name operand (`GetField`, `PutField`, `DefineField`) that
    /// names a constant that is not a string.
    NameNotString {
        index: u16,
    },
    /// A `GetBuiltinGlobal` operand that names no built-in global.
    UnknownBuiltinGlobal {
        index: u16,
    },
    InvalidEvalSite,
}

impl fmt::Display for ValidationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid bytecode at offset {}: {:?}",
            self.offset, self.kind
        )
    }
}

impl std::error::Error for ValidationError {}

/// Constant-pool capacity: every index must fit the u16 `Const` operand.
const MAX_CONSTANTS: usize = 1 << 16;

/// Child-function-table capacity: every index must fit the u16 `Function`
/// operand.
const MAX_FUNCTIONS: usize = 1 << 16;

fn err(offset: u32, kind: ValidationErrorKind) -> ValidationError {
    ValidationError { offset, kind }
}

impl Executable {
    /// Validate and load code. Never panics on malformed input.
    pub fn load(spec: ExecutableSpec) -> Result<Executable, ValidationError> {
        Executable::load_with(spec, None)
    }

    /// Validate and load code compiled from `source`, with one source
    /// location per instruction. The code passes exactly the same validation
    /// as [`Executable::load`]; the locations only feed diagnostics.
    pub(crate) fn load_compiled(
        spec: ExecutableSpec,
        filename: Arc<str>,
        source: Arc<str>,
        locations: Vec<SourceLocation>,
        construct_callees: std::collections::HashMap<usize, crate::frontend::Span>,
    ) -> Result<Executable, ValidationError> {
        Executable::load_with(
            spec,
            Some(DebugInfo {
                filename,
                source,
                locations: locations.into(),
                construct_callees,
            }),
        )
    }

    fn load_with(
        spec: ExecutableSpec,
        debug: Option<DebugInfo>,
    ) -> Result<Executable, ValidationError> {
        use ValidationErrorKind::*;
        let ExecutableSpec {
            name,
            params,
            function_length,
            function_name,
            locals,
            code,
            constants,
            functions,
            captures,
            strict,
            constructor_kind,
            eval_sites,
        } = spec;
        if code.is_empty() {
            return Err(err(0, EmptyCode));
        }
        if u32::try_from(code.len()).is_err() {
            return Err(err(0, CodeTooLarge { len: code.len() }));
        }
        if params > locals {
            return Err(err(0, ParamsExceedLocals { params, locals }));
        }
        if constants.len() > MAX_CONSTANTS {
            let count = constants.len();
            return Err(err(0, ConstantPoolTooLarge { count }));
        }
        if functions.len() > MAX_FUNCTIONS {
            let count = functions.len();
            return Err(err(0, FunctionTableTooLarge { count }));
        }

        // Pass 1: decode, checking opcodes, operand bytes and index ranges.
        // `index_at[byte offset]` maps instruction starts to instruction indices.
        let mut index_at: Vec<Option<u32>> = vec![None; code.len()];
        let mut decoded: Vec<(Opcode, u32)> = Vec::new();
        let mut offsets: Vec<u32> = Vec::new();
        let mut pos = 0usize;
        while pos < code.len() {
            let offset = pos as u32;
            let byte = code[pos];
            let opcode = Opcode::from_byte(byte).ok_or(err(offset, UnknownOpcode { byte }))?;
            let size = opcode.operand().size();
            let bytes = code
                .get(pos + 1..pos + 1 + size)
                .ok_or(err(offset, TruncatedOperand { opcode }))?;
            let operand = bytes
                .iter()
                .rev()
                .fold(0u32, |acc, &b| (acc << 8) | u32::from(b));
            match opcode.operand() {
                OperandKind::Const | OperandKind::Name if operand as usize >= constants.len() => {
                    let index = operand as u16;
                    let count = constants.len();
                    return Err(err(offset, ConstantOutOfRange { index, count }));
                }
                OperandKind::Name
                    if !matches!(constants[operand as usize], Primitive::String(_)) =>
                {
                    let index = operand as u16;
                    return Err(err(offset, NameNotString { index }));
                }
                OperandKind::Function if operand as usize >= functions.len() => {
                    let index = operand as u16;
                    let count = functions.len();
                    return Err(err(offset, FunctionOutOfRange { index, count }));
                }
                OperandKind::ThrowKind if ThrowKind::from_u16(operand as u16).is_none() => {
                    let code = operand as u16;
                    return Err(err(offset, UnknownThrowKind { code }));
                }
                OperandKind::BuiltinGlobal
                    if crate::realm::Global::from_u16(operand as u16).is_none() =>
                {
                    let index = operand as u16;
                    return Err(err(offset, UnknownBuiltinGlobal { index }));
                }
                OperandKind::Capture if operand >= u32::from(captures) => {
                    let index = operand as u16;
                    let count = captures;
                    return Err(err(offset, CaptureOutOfRange { index, count }));
                }
                OperandKind::Local if operand >= u32::from(locals) => {
                    let index = operand as u16;
                    return Err(err(
                        offset,
                        LocalOutOfRange {
                            index,
                            count: locals,
                        },
                    ));
                }
                _ => {}
            }
            index_at[pos] = Some(decoded.len() as u32);
            decoded.push((opcode, operand));
            offsets.push(offset);
            pos += 1 + size;
        }

        // Pass 2: resolve branch targets to instruction indices.
        let mut instrs = Vec::with_capacity(decoded.len());
        for (i, &(opcode, operand)) in decoded.iter().enumerate() {
            let target = || {
                index_at
                    .get(operand as usize)
                    .copied()
                    .flatten()
                    .ok_or(err(offsets[i], BadBranchTarget { target: operand }))
            };
            instrs.push(match opcode {
                Opcode::Const => Instr::Const(operand as u16),
                Opcode::Undefined => Instr::Undefined,
                Opcode::GetLocal => Instr::GetLocal(operand as u16),
                Opcode::SetLocal => Instr::SetLocal(operand as u16),
                Opcode::Pop => Instr::Pop,
                Opcode::Dup => Instr::Dup,
                Opcode::Add => Instr::Add,
                Opcode::Sub => Instr::Sub,
                Opcode::Mul => Instr::Mul,
                Opcode::Lt => Instr::Lt,
                Opcode::StrictEq => Instr::StrictEq,
                Opcode::Not => Instr::Not,
                Opcode::Jump => Instr::Jump(target()?),
                Opcode::JumpIfFalse => Instr::JumpIfFalse(target()?),
                Opcode::Return => Instr::Return,
                Opcode::NewObject => Instr::NewObject,
                Opcode::HostEffect => Instr::HostEffect(operand as u16),
                Opcode::NewVector => Instr::NewVector(operand as u16),
                Opcode::NewMap => Instr::NewMap(operand as u16),
                Opcode::Count => Instr::Count,
                Opcode::Nth => Instr::Nth,
                Opcode::Get => Instr::Get,
                Opcode::GetOr => Instr::GetOr,
                Opcode::Assoc => Instr::Assoc,
                Opcode::Dissoc => Instr::Dissoc,
                Opcode::Conj => Instr::Conj,
                Opcode::PopLast => Instr::PopLast,
                Opcode::NewVectorBuilder => Instr::NewVectorBuilder,
                Opcode::NewMapBuilder => Instr::NewMapBuilder,
                Opcode::BuilderConj => Instr::BuilderConj,
                Opcode::BuilderAssoc => Instr::BuilderAssoc,
                Opcode::Freeze => Instr::Freeze,
                Opcode::Div => Instr::Div,
                Opcode::Rem => Instr::Rem,
                Opcode::Neg => Instr::Neg,
                Opcode::ToNumber => Instr::ToNumber,
                Opcode::ToNumberPair => Instr::ToNumberPair,
                Opcode::Gt => Instr::Gt,
                Opcode::Le => Instr::Le,
                Opcode::Ge => Instr::Ge,
                Opcode::LooseEq => Instr::LooseEq,
                // Pass 1 checked the index; the capture count comes from the child.
                Opcode::Function => {
                    let index = operand as u16;
                    Instr::Function(index, functions[usize::from(index)].0.captures)
                }
                Opcode::Method => {
                    let index = operand as u16;
                    Instr::Method(index, functions[usize::from(index)].0.captures)
                }
                Opcode::Call => Instr::Call(operand as u16),
                Opcode::CurrentFunction => Instr::CurrentFunction,
                Opcode::GetCapture => Instr::GetCapture(operand as u16),
                Opcode::NewCell => Instr::NewCell,
                Opcode::CellInit => Instr::CellInit,
                Opcode::CellGet => Instr::CellGet,
                Opcode::CellSet => Instr::CellSet,
                Opcode::AtomNew => Instr::AtomNew,
                Opcode::AtomLoad => Instr::AtomLoad,
                Opcode::AtomCas => Instr::AtomCas,
                Opcode::Need => Instr::Need,
                Opcode::Throw => Instr::Throw,
                Opcode::PushCatch => Instr::PushCatch(target()?),
                Opcode::PushFinally => Instr::PushFinally(target()?),
                Opcode::PopCatch => Instr::PopCatch,
                Opcode::PopFinally => Instr::PopFinally,
                Opcode::EndFinally => Instr::EndFinally,
                Opcode::CallFinally => Instr::CallFinally,
                Opcode::DropCompletion => Instr::DropCompletion,
                Opcode::Plus => Instr::Plus,
                Opcode::CompareLt => Instr::CompareLt,
                Opcode::CompareGt => Instr::CompareGt,
                Opcode::CompareLe => Instr::CompareLe,
                Opcode::CompareGe => Instr::CompareGe,
                Opcode::Typeof => Instr::Typeof,
                Opcode::GetField => Instr::GetField(operand as u16),
                Opcode::PutField => Instr::PutField(operand as u16),
                Opcode::DefineField => Instr::DefineField(operand as u16),
                Opcode::DefineGetter => Instr::DefineGetter(operand as u16),
                Opcode::DefineSetter => Instr::DefineSetter(operand as u16),
                Opcode::GetBuiltinGlobal => Instr::GetBuiltinGlobal(operand as u16),
                Opcode::CallMethod => Instr::CallMethod(operand as u16),
                Opcode::NewObjectLiteral => Instr::NewObjectLiteral,
                Opcode::ToString => Instr::ToString,
                Opcode::GetThis => Instr::GetThis,
                Opcode::NewArray => Instr::NewArray,
                Opcode::AppendElement => Instr::AppendElement,
                Opcode::AppendHole => Instr::AppendHole,
                Opcode::GetElem => Instr::GetElem,
                Opcode::PutElem => Instr::PutElem,
                Opcode::Dup2 => Instr::Dup2,
                Opcode::ToPropertyKey => Instr::ToPropertyKey,
                Opcode::Construct => Instr::Construct(operand as u16),
                Opcode::SetProto => Instr::SetProto,
                Opcode::Instanceof => Instr::Instanceof,
                Opcode::Delete => Instr::Delete,
                Opcode::In => Instr::In,
                Opcode::GetIterator => Instr::GetIterator,
                Opcode::RequireObject => Instr::RequireObject,
                Opcode::PendingThrow => Instr::PendingThrow,
                Opcode::CallSpread => Instr::CallSpread,
                Opcode::CallMethodSpread => Instr::CallMethodSpread,
                Opcode::ConstructSpread => Instr::ConstructSpread,
                Opcode::CheckSpreadArguments => Instr::CheckSpreadArguments,
                Opcode::ForInStart => Instr::ForInStart,
                Opcode::ForInNext => Instr::ForInNext,
                Opcode::NewArguments => Instr::NewArguments,
                Opcode::GeneratorStart => Instr::GeneratorStart,
                Opcode::GeneratorYield => Instr::GeneratorYield,
                Opcode::GeneratorInitialYield => Instr::GeneratorInitialYield,
                Opcode::GeneratorYieldStar => Instr::GeneratorYieldStar,
                Opcode::AsyncStart => Instr::AsyncStart,
                Opcode::Await => Instr::Await,
                Opcode::CheckDirectEval => Instr::CheckDirectEval,
                Opcode::GetRawThis => Instr::GetRawThis,
                Opcode::GetNewTarget => Instr::GetNewTarget,
                Opcode::CheckSloppyThis => Instr::CheckSloppyThis,
                Opcode::DeclareGlobalLet => Instr::DeclareGlobalLet(operand as u16),
                Opcode::DeclareGlobalConst => Instr::DeclareGlobalConst(operand as u16),
                Opcode::InitGlobalLexical => Instr::InitGlobalLexical(operand as u16),
                Opcode::CheckGlobalVar => Instr::CheckGlobalVar(operand as u16),
                Opcode::CheckGlobalFunction => Instr::CheckGlobalFunction(operand as u16),
                Opcode::DeleteGlobal => Instr::DeleteGlobal(operand as u16),
                Opcode::DeclareGlobalConstValue => Instr::DeclareGlobalConstValue(operand as u16),
                Opcode::CheckGlobalLexicalConflict => {
                    Instr::CheckGlobalLexicalConflict(operand as u16)
                }

                Opcode::DefineElem => Instr::DefineElem,
                Opcode::DefineClassMethod => Instr::DefineClassMethod,
                Opcode::DefineClassGetter => Instr::DefineClassGetter,
                Opcode::DefineClassSetter => Instr::DefineClassSetter,
                Opcode::CheckHeritage => Instr::CheckHeritage,
                Opcode::InitHeritage => Instr::InitHeritage,
                Opcode::GetSuperConstructor => Instr::GetSuperConstructor,
                Opcode::ConstructSuper => Instr::ConstructSuper(operand as u16),
                Opcode::ConstructSuperSpread => Instr::ConstructSuperSpread,
                Opcode::BindThis => Instr::BindThis,
                Opcode::ResolveThis => Instr::ResolveThis,
                Opcode::ForwardArguments => Instr::ForwardArguments,
                Opcode::DefineGetterElem => Instr::DefineGetterElem,
                Opcode::DefineSetterElem => Instr::DefineSetterElem,
                Opcode::SetComputedName => Instr::SetComputedName(operand as u16),
                Opcode::IsEnumerable => Instr::IsEnumerable,
                Opcode::OwnKeys => Instr::OwnKeys,
                Opcode::RequireCoercible => Instr::RequireCoercible,

                Opcode::GetGlobalObject => Instr::GetGlobalObject,
                Opcode::GetGlobal => Instr::GetGlobal(operand as u16),
                Opcode::GetGlobalOrUndefined => Instr::GetGlobalOrUndefined(operand as u16),
                Opcode::PutGlobal => Instr::PutGlobal(operand as u16),
                Opcode::PutGlobalStrict => Instr::PutGlobalStrict(operand as u16),
                Opcode::PutFieldStrict => Instr::PutFieldStrict(operand as u16),
                Opcode::PutElemStrict => Instr::PutElemStrict,
                Opcode::DeleteStrict => Instr::DeleteStrict,
                Opcode::DeclareGlobalVar => Instr::DeclareGlobalVar(operand as u16),
                Opcode::DeclareGlobalFunction => Instr::DeclareGlobalFunction(operand as u16),
                Opcode::TemplateObject => Instr::TemplateObject(operand as u16),
                Opcode::ToNumeric => Instr::ToNumeric,
                Opcode::Inc => Instr::Inc,
                Opcode::Dec => Instr::Dec,
                Opcode::RegExpLiteral => Instr::RegExpLiteral(operand as u16),
                Opcode::Reduce => Instr::Reduce,
                Opcode::NeedAll => Instr::NeedAll,
                Opcode::BitAnd => Instr::BitAnd,
                Opcode::BitOr => Instr::BitOr,
                Opcode::BitXor => Instr::BitXor,
                Opcode::Shl => Instr::Shl,
                Opcode::Sar => Instr::Sar,
                Opcode::UShr => Instr::UShr,
                Opcode::BitNot => Instr::BitNot,
                Opcode::Pow => Instr::Pow,
                Opcode::ThrowError => match ThrowKind::from_u16(operand as u16) {
                    Some(kind) => Instr::ThrowError(kind),
                    // Pass 1 rejected unknown kinds.
                    None => {
                        return Err(err(
                            offsets[i],
                            UnknownThrowKind {
                                code: operand as u16,
                            },
                        ));
                    }
                },
            });
        }

        let mut sites = std::collections::HashMap::new();
        for site in eval_sites {
            let offset = site.offset;
            let index = index_at
                .get(offset as usize)
                .copied()
                .flatten()
                .ok_or(err(offset, InvalidEvalSite))? as usize;
            let valid_source = |source| match source {
                EvalSource::Local(i) => i < locals,
                EvalSource::Capture(i) => i < captures,
                _ => true,
            };
            let mut names = std::collections::HashSet::new();
            // Runtime compilation imports the receiver and optional new.target
            // after the named bindings; the complete capture count is u16.
            if !matches!(instrs[index], Instr::Call(_) | Instr::CallSpread)
                || site.bindings.len() + site.environments.len()
                    > usize::from(u16::MAX) - 1 - usize::from(site.new_target.is_some())
                || !valid_source(site.receiver)
                || site.new_target.is_some_and(|source| !valid_source(source))
                || site.environments.iter().any(|s| !valid_source(*s))
                || site.variable_environment.is_some_and(|s| !valid_source(s))
                || site
                    .variable_environment
                    .is_some_and(|s| !site.environments.contains(&s))
                || site.variable_bindings.iter().any(|name| {
                    !site
                        .bindings
                        .iter()
                        .any(|b| b.name == *name && b.kind == EvalBindingKind::Mutable && b.cell)
                })
                || site.bindings.iter().any(|b| {
                    !valid_source(b.source)
                        || !names.insert(&b.name)
                        || (b.kind == EvalBindingKind::Mutable && !b.cell)
                        || usize::from(b.environments) > site.environments.len()
                })
                || sites.contains_key(&index)
            {
                return Err(err(offset, InvalidEvalSite));
            }
            sites.insert(index, site);
        }
        let max_stack = check_stack(&instrs, &offsets)?;
        let debug = match debug {
            Some(info) if info.locations.len() != instrs.len() => {
                return Err(err(
                    0,
                    SourceMapMismatch {
                        instructions: instrs.len(),
                        locations: info.locations.len(),
                    },
                ));
            }
            Some(info) => Some(info),
            None => None,
        };
        let mut regexps = std::collections::HashMap::new();
        for instr in &instrs {
            if let Instr::RegExpLiteral(index) = instr
                && let Primitive::String(encoded) = &constants[*index as usize]
            {
                regexps
                    .entry(*index)
                    .or_insert_with(|| crate::regexp::object::compile_literal(encoded));
            }
        }
        Ok(Executable(Arc::new(Code {
            regexps,
            function_length: function_length.unwrap_or(params),
            function_name: function_name.unwrap_or_else(|| JsString::from(name.as_str())),
            name: name.into(),
            params,
            locals,
            constants: constants.into_iter().map(Value::from).collect(),
            functions: functions.into_iter().map(|f| f.0).collect(),
            captures,
            strict,
            constructor_kind,
            uses_arguments: instrs
                .iter()
                .any(|i| matches!(i, Instr::NewArguments | Instr::ForwardArguments)),
            instrs: instrs.into(),
            offsets: offsets.into(),
            max_stack,
            debug,
            eval_sites: sites,
        })))
    }

    /// Diagnostic name.
    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// True for strict-mode code ([`ExecutableSpec::strict`]): code compiled
    /// in strict mode or under a `"use strict"` directive, and all
    /// hand-assembled code.
    pub fn is_strict(&self) -> bool {
        self.0.strict
    }

    /// Maximum operand-stack depth computed at load time.
    pub fn max_stack(&self) -> usize {
        self.0.max_stack
    }

    /// True when both handles share the same loaded code (no copy was made).
    pub fn same_code(a: &Executable, b: &Executable) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }

    /// Child function `index` of this code's function table: the same shared
    /// code the spec supplied (no copy), or `None` out of range.
    pub fn function(&self, index: usize) -> Option<Executable> {
        self.0.functions.get(index).cloned().map(Executable)
    }

    /// Number of instructions.
    pub fn instruction_count(&self) -> usize {
        self.0.instrs.len()
    }

    /// Source location of instruction `index`, for code compiled from
    /// JavaScript source; `None` for hand-assembled code or an index out of
    /// range.
    pub fn source_location(&self, index: usize) -> Option<SourceLocation> {
        self.0.location(index)
    }

    /// The code re-encoded in the byte format, as a spec that
    /// [`Executable::load`] accepts again. Source locations are not part of
    /// the byte format and are not included. Loaded code has only primitive
    /// constants in this slice, so the constant pool is reproduced exactly.
    pub fn to_spec(&self) -> ExecutableSpec {
        let code = &self.0;
        let mut bytes = Vec::new();
        for &instr in code.instrs.iter() {
            let opcode = instr.opcode();
            bytes.push(opcode as u8);
            let operand = match instr {
                Instr::RegExpLiteral(v)
                | Instr::Const(v)
                | Instr::GetLocal(v)
                | Instr::SetLocal(v)
                | Instr::HostEffect(v)
                | Instr::NewVector(v)
                | Instr::NewMap(v)
                | Instr::Function(v, _)
                | Instr::Method(v, _)
                | Instr::Call(v)
                | Instr::CallMethod(v)
                | Instr::Construct(v)
                | Instr::ConstructSuper(v)
                | Instr::GetBuiltinGlobal(v)
                | Instr::GetCapture(v)
                | Instr::GetGlobal(v)
                | Instr::GetGlobalOrUndefined(v)
                | Instr::PutGlobal(v)
                | Instr::PutGlobalStrict(v)
                | Instr::PutFieldStrict(v)
                | Instr::DeclareGlobalVar(v)
                | Instr::DeclareGlobalFunction(v)
                | Instr::DeclareGlobalLet(v)
                | Instr::DeclareGlobalConst(v)
                | Instr::InitGlobalLexical(v)
                | Instr::CheckGlobalVar(v)
                | Instr::CheckGlobalFunction(v)
                | Instr::DeleteGlobal(v)
                | Instr::DeclareGlobalConstValue(v)
                | Instr::CheckGlobalLexicalConflict(v)
                | Instr::TemplateObject(v)
                | Instr::GetField(v)
                | Instr::PutField(v)
                | Instr::DefineField(v)
                | Instr::DefineGetter(v)
                | Instr::DefineSetter(v)
                | Instr::SetComputedName(v) => u32::from(v),
                Instr::ThrowError(kind) => u32::from(kind as u16),
                // Branch targets are instruction indices once loaded.
                Instr::Jump(t)
                | Instr::JumpIfFalse(t)
                | Instr::PushCatch(t)
                | Instr::PushFinally(t) => code.offsets[t as usize],
                _ => 0,
            };
            let size = opcode.operand().size();
            bytes.extend_from_slice(&operand.to_le_bytes()[..size]);
        }
        ExecutableSpec {
            name: code.name.to_string(),
            params: code.params,
            function_length: Some(code.function_length),
            function_name: Some(code.function_name.clone()),
            locals: code.locals,
            code: bytes,
            constants: code
                .constants
                .iter()
                .filter_map(|v| v.to_primitive())
                .collect(),
            functions: code.functions.iter().cloned().map(Executable).collect(),
            captures: code.captures,
            strict: code.strict,
            constructor_kind: code.constructor_kind,
            eval_sites: {
                let mut sites: Vec<_> = code.eval_sites.values().cloned().collect();
                sites.sort_by_key(|s| s.offset);
                sites
            },
        }
    }
}

/// Executable constants are a root edge (QuickJS marks bytecode constant
/// pools in `mark_children`). They are traced through every live task that
/// runs the code (`RootCategory::ExecutableConstant`) and live function objects.
/// Symbol constants in descendant code are reachable before a closure over
/// that code is created. Walk the immutable DAG iteratively and visit shared
/// children once; Rust Executable handles alone are not collector roots.
impl Trace for Code {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        let mut pending = vec![self];
        let mut seen = std::collections::HashSet::new();
        while let Some(code) = pending.pop() {
            if seen.insert(std::ptr::from_ref(code)) {
                code.constants.trace(visit);
                pending.extend(code.functions.iter().map(AsRef::as_ref));
            }
        }
    }
}

/// Abstract interpretation of operand-stack depth over all reachable paths.
/// Returns the maximum depth.
fn check_stack(instrs: &[Instr], offsets: &[u32]) -> Result<usize, ValidationError> {
    use ValidationErrorKind::*;
    let mut depth_at: Vec<Option<usize>> = vec![None; instrs.len()];
    let mut work = vec![0usize];
    depth_at[0] = Some(0);
    let mut max = 0;
    while let Some(i) = work.pop() {
        let (instr, offset) = (instrs[i], offsets[i]);
        let depth = depth_at[i].unwrap_or(0);
        let opcode = instr.opcode();
        let (pops, pushes) = instr.stack_effect();
        if depth < pops {
            return Err(err(offset, StackUnderflow { opcode, depth }));
        }
        let after = depth - pops + pushes;
        max = max.max(after);
        // Successors and their depths. A catch target starts with the
        // exception value pushed, a finally target at its handler's depth.
        // `Return` and `EndFinally` may also continue at a finally target of
        // their frame, which that target's `PushFinally` edge already checks.
        let (a, b) = match instr {
            Instr::Return | Instr::Throw => (None, None),
            Instr::Jump(t) => (Some((t as usize, after)), None),
            Instr::JumpIfFalse(t) | Instr::PushFinally(t) => {
                (Some((t as usize, after)), Some((i + 1, after)))
            }
            Instr::PushCatch(t) => (Some((t as usize, after + 1)), Some((i + 1, after))),
            _ => (Some((i + 1, after)), None),
        };
        for (next, after) in [a, b].into_iter().flatten() {
            max = max.max(after);
            if next >= instrs.len() {
                return Err(err(offset, FallsOffEnd));
            }
            match depth_at[next] {
                None => {
                    depth_at[next] = Some(after);
                    work.push(next);
                }
                Some(expected) if expected != after => {
                    let at = offsets[next];
                    return Err(err(
                        at,
                        StackDepthMismatch {
                            expected,
                            found: after,
                        },
                    ));
                }
                Some(_) => {}
            }
        }
    }
    Ok(max)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ValidationErrorKind::*;

    fn load(
        code: Vec<u8>,
        constants: Vec<Primitive>,
        locals: u16,
    ) -> Result<Executable, ValidationError> {
        Executable::load(ExecutableSpec {
            name: "t".into(),
            params: 0,
            function_length: None,
            function_name: None,
            locals,
            code,
            constants,
            functions: Vec::new(),
            captures: 0,
            strict: true,
            constructor_kind: ConstructorKind::None,
            eval_sites: Vec::new(),
        })
    }

    fn kind(r: Result<Executable, ValidationError>) -> ValidationErrorKind {
        r.unwrap_err().kind
    }

    const RET: u8 = Opcode::Return as u8;
    const UNDEF: u8 = Opcode::Undefined as u8;

    #[test]
    fn eval_site_capture_limits() {
        for (count, environments, new_target, valid) in [
            (65_534, 0, None, true),
            (65_534, 0, Some(EvalSource::NewTarget), false),
            (65_533, 0, Some(EvalSource::NewTarget), true),
            (65_535, 0, None, false),
            (65_533, 1, None, true),
            (65_534, 1, None, false),
            (65_532, 1, Some(EvalSource::NewTarget), true),
            (65_533, 1, Some(EvalSource::NewTarget), false),
            (0, 65_534, None, true),
            (0, 65_535, None, false),
            (0, 65_533, Some(EvalSource::NewTarget), true),
            (0, 65_534, Some(EvalSource::NewTarget), false),
        ] {
            let mut spec = with_functions(vec![UNDEF, Opcode::Call as u8, 0, 0, RET], vec![]);
            spec.eval_sites.push(EvalSite {
                offset: 1,
                strict: true,
                bindings: (0..count)
                    .map(|i| EvalBinding {
                        name: format!("b{i}"),
                        source: EvalSource::This,
                        kind: EvalBindingKind::Const,
                        cell: false,
                        environments: 0,
                    })
                    .collect(),
                receiver: EvalSource::This,
                receiver_strict: true,
                new_target,
                environments: vec![EvalSource::This; environments],
                variable_environment: None,
                variable_bindings: Vec::new(),
                lexical_conflicts: Vec::new(),
            });
            let result = Executable::load(spec);
            if valid {
                result.unwrap_or_else(|error| {
                    panic!("{count}, {environments}, {new_target:?}: {error:?}")
                });
            } else {
                assert!(
                    result.is_err(),
                    "accepted {count} bindings and {environments} environments with {new_target:?}"
                );
                if let Err(error) = result {
                    assert_eq!(
                        error.kind, InvalidEvalSite,
                        "{count}, {environments}, {new_target:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn regexp_literal_operands_validate_and_programs_survive_reload() {
        let bytes = vec![Opcode::RegExpLiteral as u8, 0, 0, RET];
        assert_eq!(
            kind(load(bytes.clone(), vec![], 0)),
            ConstantOutOfRange { index: 0, count: 0 }
        );
        assert_eq!(
            kind(load(bytes.clone(), vec![Primitive::Null], 0)),
            NameNotString { index: 0 }
        );
        let code = load(bytes, vec![Primitive::String("g\0a".into())], 0).unwrap();
        assert!(code.0.regexps[&0].is_ok());
        let reloaded = Executable::load(code.to_spec()).unwrap();
        assert!(reloaded.0.regexps[&0].is_ok());
    }

    #[test]
    fn property_name_operands_must_name_string_constants() {
        let get = Opcode::GetField as u8;
        let put = Opcode::PutField as u8;
        let name = || vec![Primitive::String("x".into())];
        // undefined.x is a run-time TypeError, but valid code.
        let ok = load(vec![UNDEF, get, 0, 0, RET], name(), 0).unwrap();
        // The operand survives a round trip through the byte format.
        assert_eq!(
            Executable::load(ok.to_spec()).unwrap().to_spec(),
            ok.to_spec()
        );
        assert_eq!(
            kind(load(
                vec![UNDEF, get, 0, 0, RET],
                vec![Primitive::Number(1.0)],
                0
            )),
            NameNotString { index: 0 }
        );
        assert_eq!(
            kind(load(vec![UNDEF, UNDEF, put, 1, 0, RET], name(), 0)),
            ConstantOutOfRange { index: 1, count: 1 }
        );
        // `PutField` pops two values.
        assert!(matches!(
            kind(load(vec![UNDEF, put, 0, 0, RET], name(), 0)),
            StackUnderflow { .. }
        ));
    }

    #[test]
    fn accepts_minimal_program_and_computes_max_stack() {
        let exe = load(vec![UNDEF, UNDEF, Opcode::Pop as u8, RET], vec![], 0).unwrap();
        assert_eq!(exe.max_stack(), 2);
    }

    #[test]
    fn rejects_unknown_opcode_and_empty_code() {
        assert_eq!(kind(load(vec![], vec![], 0)), EmptyCode);
        assert_eq!(kind(load(vec![0x00], vec![], 0)), UnknownOpcode { byte: 0 });
        let e = load(vec![UNDEF, 0xFF], vec![], 0).unwrap_err();
        assert_eq!((e.offset, e.kind), (1, UnknownOpcode { byte: 0xFF }));
    }

    #[test]
    fn rejects_truncated_and_out_of_range_operands() {
        let c = Opcode::Const as u8;
        assert_eq!(
            kind(load(vec![c, 0], vec![Primitive::Null], 0)),
            TruncatedOperand {
                opcode: Opcode::Const
            }
        );
        assert_eq!(
            kind(load(vec![c, 1, 0, RET], vec![Primitive::Null], 0)),
            ConstantOutOfRange { index: 1, count: 1 }
        );
        assert_eq!(
            kind(load(vec![Opcode::GetLocal as u8, 2, 0, RET], vec![], 2)),
            LocalOutOfRange { index: 2, count: 2 }
        );
        let e = Executable::load(ExecutableSpec {
            name: "t".into(),
            params: 3,
            function_length: None,
            function_name: None,
            locals: 2,
            code: vec![UNDEF, RET],
            constants: vec![],
            functions: vec![],
            captures: 0,
            strict: true,
            constructor_kind: ConstructorKind::None,
            eval_sites: Vec::new(),
        });
        assert_eq!(
            kind(e),
            ParamsExceedLocals {
                params: 3,
                locals: 2
            }
        );
    }

    #[test]
    fn rejects_bad_branch_targets() {
        let j = Opcode::Jump as u8;
        // Target 2 is inside the jump's own operand.
        assert_eq!(
            kind(load(vec![j, 2, 0, 0, 0, UNDEF, RET], vec![], 0)),
            BadBranchTarget { target: 2 }
        );
        // Target past the end of the code.
        assert_eq!(
            kind(load(vec![j, 0xFF, 0xFF, 0xFF, 0xFF, UNDEF, RET], vec![], 0)),
            BadBranchTarget { target: u32::MAX }
        );
    }

    #[test]
    fn builtin_global_operands_are_validated() {
        let g = Opcode::GetBuiltinGlobal as u8;
        assert_eq!(
            kind(load(vec![g, 42, 0, RET], vec![], 0)),
            UnknownBuiltinGlobal { index: 42 }
        );
        // FinalizationRegistry appends operand 41; prior operands keep their meaning.
        for i in 0..42 {
            assert!(load(vec![g, i, 0, RET], vec![], 0).is_ok());
        }
    }

    #[test]
    fn throw_error_kinds_are_validated_and_specs_round_trip() {
        let t = Opcode::ThrowError as u8;
        assert_eq!(
            kind(load(vec![t, 4, 0, RET], vec![], 0)),
            UnknownThrowKind { code: 4 }
        );
        // Every instruction shape survives `to_spec` and reloads identically.
        let (c, j, jf) = (
            Opcode::Const as u8,
            Opcode::Jump as u8,
            Opcode::JumpIfFalse as u8,
        );
        let code = vec![
            c,
            0,
            0,
            jf,
            12,
            0,
            0,
            0,
            t,
            1,
            0,
            Opcode::Pop as u8, // 0..12
            Opcode::GetLocal as u8,
            0,
            0,
            j,
            20,
            0,
            0,
            0, // 12..20
            RET,
        ];
        let exe = load(code.clone(), vec![Primitive::Bool(true)], 1).unwrap();
        let spec = exe.to_spec();
        assert_eq!(spec.code, code);
        assert_eq!(spec.constants, vec![Primitive::Bool(true)]);
        assert_eq!(exe.instruction_count(), 7);
        let again = Executable::load(spec).unwrap();
        assert_eq!(again.0.instrs, exe.0.instrs);
        assert_eq!(exe.source_location(0), None);
    }

    #[test]
    fn compiled_code_needs_one_location_per_instruction() {
        let spec = ExecutableSpec {
            name: "t".into(),
            params: 0,
            function_length: None,
            function_name: None,
            locals: 0,
            code: vec![UNDEF, RET],
            constants: vec![],
            functions: vec![],
            captures: 0,
            strict: true,
            constructor_kind: ConstructorKind::None,
            eval_sites: Vec::new(),
        };
        let at = SourceLocation {
            span: crate::frontend::Span::new(0, 1),
            line: 1,
            column: 1,
        };
        let e = Executable::load_compiled(
            spec.clone(),
            "file".into(),
            "x".into(),
            vec![at],
            Default::default(),
        )
        .unwrap_err();
        assert_eq!(
            e.kind,
            SourceMapMismatch {
                instructions: 2,
                locations: 1
            }
        );
        let exe = Executable::load_compiled(
            spec,
            "file".into(),
            "x".into(),
            vec![at, at],
            Default::default(),
        )
        .unwrap();
        assert_eq!(exe.source_location(1), Some(at));
        assert_eq!(exe.source_location(2), None);
    }

    #[test]
    fn rejects_stack_underflow_mismatch_and_fall_through() {
        assert_eq!(
            kind(load(vec![RET], vec![], 0)),
            StackUnderflow {
                opcode: Opcode::Return,
                depth: 0
            }
        );
        assert_eq!(kind(load(vec![UNDEF], vec![], 0)), FallsOffEnd);
        // if (undefined) {push} ; merge with depths 0 and 1.
        let jf = Opcode::JumpIfFalse as u8;
        let code = vec![UNDEF, jf, 7, 0, 0, 0, UNDEF, UNDEF, RET];
        assert_eq!(
            kind(load(code, vec![], 0)),
            StackDepthMismatch {
                expected: 0,
                found: 1
            }
        );
    }

    fn with_functions(code: Vec<u8>, functions: Vec<Executable>) -> ExecutableSpec {
        ExecutableSpec {
            name: "t".into(),
            params: 0,
            function_length: None,
            function_name: None,
            locals: 0,
            code,
            constants: vec![],
            functions,
            captures: 0,
            strict: true,
            constructor_kind: ConstructorKind::None,
            eval_sites: Vec::new(),
        }
    }

    #[test]
    fn function_and_call_operands_are_validated() {
        let (f, call, cur) = (
            Opcode::Function as u8,
            Opcode::Call as u8,
            Opcode::CurrentFunction as u8,
        );
        let child = load(vec![UNDEF, RET], vec![], 0).unwrap();
        // `Function 1` with a one-entry table.
        let e = Executable::load(with_functions(vec![f, 1, 0, RET], vec![child.clone()]));
        assert_eq!(kind(e), FunctionOutOfRange { index: 1, count: 1 });
        let e = Executable::load(with_functions(vec![f, 0, 0, RET], vec![]));
        assert_eq!(kind(e), FunctionOutOfRange { index: 0, count: 0 });
        let e = Executable::load(with_functions(vec![f, 0], vec![child.clone()]));
        assert_eq!(
            kind(e),
            TruncatedOperand {
                opcode: Opcode::Function
            }
        );
        // `Call 2` needs the callee plus two arguments.
        let e = Executable::load(with_functions(
            vec![f, 0, 0, UNDEF, call, 2, 0, RET],
            vec![child.clone()],
        ));
        assert_eq!(
            kind(e),
            StackUnderflow {
                opcode: Opcode::Call,
                depth: 2
            }
        );
        // callee + 2 arguments -> 1 result; max depth 3.
        let exe = Executable::load(with_functions(
            vec![cur, UNDEF, UNDEF, call, 2, 0, RET],
            vec![],
        ))
        .unwrap();
        assert_eq!(exe.max_stack(), 3);
        let too_many = vec![child.clone(); MAX_FUNCTIONS + 1];
        let e = Executable::load(with_functions(vec![UNDEF, RET], too_many));
        assert_eq!(
            kind(e),
            FunctionTableTooLarge {
                count: MAX_FUNCTIONS + 1
            }
        );
    }

    #[test]
    fn child_functions_are_shared_and_round_trip() {
        let child = load(vec![UNDEF, RET], vec![], 0).unwrap();
        let code = vec![Opcode::Function as u8, 0, 0, Opcode::Call as u8, 0, 0, RET];
        let exe = Executable::load(with_functions(code.clone(), vec![child.clone()])).unwrap();
        assert!(Executable::same_code(&exe.function(0).unwrap(), &child));
        assert!(Arc::ptr_eq(&exe.0.functions[0], &child.0), "not copied");
        let spec = exe.to_spec();
        assert_eq!(spec.code, code);
        assert_eq!(spec.functions, vec![child.clone()]);
        let again = Executable::load(spec).unwrap();
        assert_eq!(again.0.instrs, exe.0.instrs);
        assert!(Executable::same_code(&again.function(0).unwrap(), &child));
    }

    #[test]
    fn dropping_deep_function_code_does_not_overflow_the_rust_stack() {
        // A chain of 50,000 levels, each executable owning the previous one
        // as its only child; only the outermost handle is kept.
        let mut exe = load(vec![UNDEF, RET], vec![], 0).unwrap();
        for _ in 0..50_000 {
            exe = Executable::load(with_functions(vec![UNDEF, RET], vec![exe])).unwrap();
        }
        std::thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || drop(exe))
            .unwrap()
            .join()
            .expect("teardown is iterative");
    }
}
