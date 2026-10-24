//! The instruction format.
//!
//! A new, compact byte format (not QuickJS-compatible). Each instruction is
//! one opcode byte followed by its fixed-width little-endian operand:
//!
//! | Opcode          | Byte | Operand            | Stack (before → after) | Meaning |
//! |-----------------|------|--------------------|------------------------|---------|
//! | `Const`         | 0x01 | u16 constant index | → v                    | push constant |
//! | `Undefined`     | 0x02 | –                  | → undefined            | push `undefined` |
//! | `GetLocal`      | 0x03 | u16 local index    | → v                    | push local |
//! | `SetLocal`      | 0x04 | u16 local index    | v →                    | pop into local |
//! | `Pop`           | 0x05 | –                  | v →                    | discard |
//! | `Dup`           | 0x06 | –                  | v → v v                | duplicate top |
//! | `Add`           | 0x07 | –                  | a b → a+b              | number addition |
//! | `Sub`           | 0x08 | –                  | a b → a-b              | number subtraction |
//! | `Mul`           | 0x09 | –                  | a b → a*b              | number multiplication |
//! | `Lt`            | 0x0A | –                  | a b → a<b              | number less-than |
//! | `StrictEq`      | 0x0B | –                  | a b → a===b            | JavaScript `===` |
//! | `Not`           | 0x0C | –                  | v → !v                 | JavaScript `!` (ToBoolean) |
//! | `Jump`          | 0x0D | u32 byte offset    | –                      | unconditional branch |
//! | `JumpIfFalse`   | 0x0E | u32 byte offset    | v →                    | branch if ToBoolean(v) is false |
//! | `Return`        | 0x0F | –                  | v →                    | complete the task with v |
//! | `NewObject`     | 0x10 | –                  | → obj                  | allocate a fresh plain object |
//! | `HostEffect`    | 0x11 | u16 host op        | arg → result           | leaf host effect (see [`crate::host`]) |
//! | `NewVector`     | 0x12 | u16 count n        | v1 … vn → vec          | new vector of the n values, in order |
//! | `NewMap`        | 0x13 | u16 entry count n  | k1 v1 … kn vn → map    | new map; a later duplicate key wins |
//! | `Count`         | 0x14 | –                  | coll → n               | element or entry count |
//! | `Nth`           | 0x15 | –                  | vec i → v              | vector element at index `i` |
//! | `Get`           | 0x16 | –                  | coll key → v           | lookup; fails if absent |
//! | `GetOr`         | 0x17 | –                  | coll key d → v         | lookup; `d` if absent |
//! | `Assoc`         | 0x18 | –                  | coll key v → coll'     | persistent assoc |
//! | `Dissoc`        | 0x19 | –                  | map key → map'         | persistent dissoc |
//! | `Conj`          | 0x1A | –                  | vec v → vec'           | persistent append |
//! | `PopLast`       | 0x1B | –                  | vec → vec'             | persistent remove-last |
//! | `NewVectorBuilder` | 0x1C | –               | → b                    | new task-owned vector builder |
//! | `NewMapBuilder` | 0x1D | –                  | → b                    | new task-owned map builder |
//! | `BuilderConj`   | 0x1E | –                  | b v → b                | append to a vector builder in place |
//! | `BuilderAssoc`  | 0x1F | –                  | b key v → b            | set an index or key in a builder in place |
//! | `Freeze`        | 0x20 | –                  | b → coll               | persistent value of a builder; invalidates it |
//! | `Div`           | 0x21 | –                  | a b → a/b              | number division |
//! | `Rem`           | 0x22 | –                  | a b → a%b              | number remainder (sign of the dividend) |
//! | `Neg`           | 0x23 | –                  | a → -a                 | number negation |
//! | `ToNumber`      | 0x24 | –                  | v → n                  | JavaScript ToNumber of a primitive |
//! | `ToNumberPair`  | 0x25 | –                  | a b → n m              | ToNumber of both, `a` first |
//! | `Gt`            | 0x26 | –                  | a b → a>b              | number greater-than |
//! | `Le`            | 0x27 | –                  | a b → a<=b             | number less-than-or-equal |
//! | `Ge`            | 0x28 | –                  | a b → a>=b             | number greater-than-or-equal |
//! | `LooseEq`       | 0x29 | –                  | a b → a==b             | JavaScript `==` |
//! | `ThrowError`    | 0x2A | u16 [`ThrowKind`]  | → (never returns)      | fail with a static JavaScript error |
//! | `Function`      | 0x2B | u16 function index | c1 … cn → f            | new function object (closure) for child code `i` |
//! | `Call`          | 0x2C | u16 argument count n | f a1 … an → r        | call `f` in a new task-owned frame |
//! | `CurrentFunction` | 0x2D | –                | → f                    | the function object of the running frame |
//! | `GetCapture`    | 0x2E | u16 capture index  | → v                    | captured value `i` of the running closure |
//! | `NewCell`       | 0x2F | –                  | → cell                 | new uninitialized binding cell |
//! | `CellInit`      | 0x30 | –                  | cell v →               | initialize (or re-initialize) a cell |
//! | `CellGet`       | 0x31 | –                  | cell → v               | read a cell; ReferenceError if uninitialized |
//! | `CellSet`       | 0x32 | –                  | cell v →               | assign a cell; ReferenceError if uninitialized |
//! | `AtomNew`       | 0x33 | –                  | v → atom               | new shared atom holding `v` |
//! | `AtomLoad`      | 0x34 | –                  | atom → v               | the atom's current value |
//! | `AtomCas`       | 0x35 | –                  | atom e n → w ok        | identity compare-and-set; `w` is the value compared |
//! | `Need`          | 0x36 | –                  | def kind → v           | read a dependency result, or suspend until it is published |
//!
//! Opcodes `0x12`–`0x20` are the private collection intrinsics and builder
//! operations. Their contract (operand validation, errors, the builder
//! lifecycle) is in [`crate::vm::intrinsics`]. They are VM operations a
//! compiler may lower known collection calls to, not user-visible globals.
//!
//! Opcodes `0x21`–`0x2A` are what the source compiler
//! ([`crate::frontend::compile_script`]) needs beyond the core set.
//! `ToNumber` converts `undefined` to NaN, `null` to `+0` and booleans to 0
//! or 1; an object operand fails with `UnsupportedOperand` (ToPrimitive on
//! objects is deferred). `LooseEq` is JavaScript `==` (IsLooselyEqual) for
//! the supported value types: `null == undefined`, a boolean compares as its
//! number, objects compare by identity, and comparing an object with a
//! number or boolean fails with `UnsupportedOperand`. `ThrowError` always
//! fails the task with the [`ThrowKind`] named by its operand
//! ([`VmErrorKind::ReferenceError`](crate::VmErrorKind::ReferenceError) or
//! [`VmErrorKind::TypeError`](crate::VmErrorKind::TypeError)); until
//! exceptions exist (clj-quickjs-11) it cannot be caught. For stack checking
//! it counts as pushing one value, so it can stand in for an expression.
//!
//! Opcodes `0x2B`–`0x2D` are calls (clj-quickjs-4). An executable carries a
//! table of child function code ([`ExecutableSpec::functions`](crate::ExecutableSpec::functions),
//! shared `Arc` handles, never copied); `Function i` allocates a new function
//! object for child `i` each time it runs, so two evaluations are distinct
//! objects, as with JavaScript function expressions. `Call n` pops the callee
//! and `n` arguments, pushes a task-owned frame for the callee's code and
//! continues there; the callee's `Return` pops its frame and pushes the result
//! onto the caller's operands. Arguments bind to the callee's parameter
//! locals: missing ones are `undefined`, extra ones are ignored, its other
//! locals start `undefined`. There is no `this` or `arguments` object. A
//! callee that is not a function object fails the task with
//! [`VmErrorKind::TypeError`](crate::VmErrorKind::TypeError) (`… is not a
//! function`); a call beyond the task's [`VmLimits`](crate::VmLimits) fails it
//! with [`VmErrorKind::StackOverflow`](crate::VmErrorKind::StackOverflow).
//! `CurrentFunction` pushes the function object of the running frame (for the
//! task's entry frame, one is allocated on first use and reused): the
//! self-reference of a named function expression, like QuickJS's
//! `special_object` with `OP_SPECIAL_OBJECT_THIS_FUNC`. Frames are described in
//! `crate::vm::frames`.
//!
//! Opcodes `0x2E`–`0x32` are lexical closures (clj-quickjs-20); their contract
//! is in `crate::vm::function`. A child function declares how many captured
//! values a closure over it carries
//! ([`ExecutableSpec::captures`](crate::ExecutableSpec::captures), `n`
//! above); `Function i` pops exactly that many (pushed first capture first)
//! and stores them, in order, in the new function object, which is the
//! closure's environment. `GetCapture i` pushes captured value `i` of the
//! running frame's function object; operands are checked against the
//! declaration at load time. A captured value is either an immutable value
//! (captured by value) or a *binding cell*, the shared, task-owned storage of
//! a mutable binding: `NewCell` creates one uninitialized, `CellInit` stores
//! its initial value, `CellGet` and `CellSet` read and assign it and fail
//! with `ReferenceError` (as `ThrowError Uninitialized` does) while it is
//! uninitialized, which is the run-time temporal-dead-zone check of a
//! captured `let`/`const`. Cell operations on another task's cell fail with
//! [`VmErrorKind::ForeignBinding`](crate::VmErrorKind::ForeignBinding); on a
//! value that is not a cell, with `UnsupportedOperand`.
//!
//! Opcodes `0x33`–`0x35` are the private atom intrinsics (clj-quickjs-10),
//! documented in [`crate::vm::intrinsics`] and [`crate::atom`].
//!
//! Opcode `0x36`, `Need`, is the dependency read behind `__rt.need`
//! (clj-quickjs-23): the resumable wait of `docs/design.md` §7, documented in
//! [`crate::task`] ("Dependency waits in the VM"). It pops a definition id
//! and a result kind (non-negative integer numbers; anything else fails with
//! `UnsupportedOperand`) and pushes the shared result value, which may
//! require ending the run suspended and continuing at this instruction when
//! the task is run again.
//!
//! Byte `0x00` is deliberately unassigned so zeroed memory never decodes.
//! Branch targets are absolute byte offsets that must land on an instruction
//! boundary. The opcode list is an append-only registry: new opcodes take the
//! next byte and are handled in every exhaustive match.
//!
//! `Add`, `Sub`, `Mul`, `Lt`, `Div`, `Rem`, `Neg`, `Gt`, `Le` and `Ge`
//! support Number operands only. Other operand types fail the task with
//! [`VmErrorKind::UnsupportedOperand`](crate::VmErrorKind::UnsupportedOperand)
//! rather than applying JavaScript's coercions; a compiler applies them
//! explicitly with `ToNumber`/`ToNumberPair`.
//!
//! The local/branch/return families follow the shape of QuickJS's
//! `get_loc`/`put_loc`, `goto`/`if_false` and `return` opcodes
//! (`research/quickjs/quickjs-opcode.h`, revision 7d1c13c5d8f8); no QuickJS
//! code or encoding is reused. `ThrowError` plays the role of QuickJS's
//! `throw_error` (emitted for assignment to a constant and similar statically
//! known failures). The collection intrinsics follow QuickJS's
//! practice of dedicated opcodes for known operations (for example
//! `array_from`, `get_array_el`, `append` in the same file); their semantics
//! are this project's own collection contract.

/// One opcode of the instruction format. See the module docs for the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum Opcode {
    Const = 0x01,
    Undefined = 0x02,
    GetLocal = 0x03,
    SetLocal = 0x04,
    Pop = 0x05,
    Dup = 0x06,
    Add = 0x07,
    Sub = 0x08,
    Mul = 0x09,
    Lt = 0x0A,
    StrictEq = 0x0B,
    Not = 0x0C,
    Jump = 0x0D,
    JumpIfFalse = 0x0E,
    Return = 0x0F,
    NewObject = 0x10,
    HostEffect = 0x11,
    NewVector = 0x12,
    NewMap = 0x13,
    Count = 0x14,
    Nth = 0x15,
    Get = 0x16,
    GetOr = 0x17,
    Assoc = 0x18,
    Dissoc = 0x19,
    Conj = 0x1A,
    PopLast = 0x1B,
    NewVectorBuilder = 0x1C,
    NewMapBuilder = 0x1D,
    BuilderConj = 0x1E,
    BuilderAssoc = 0x1F,
    Freeze = 0x20,
    Div = 0x21,
    Rem = 0x22,
    Neg = 0x23,
    ToNumber = 0x24,
    ToNumberPair = 0x25,
    Gt = 0x26,
    Le = 0x27,
    Ge = 0x28,
    LooseEq = 0x29,
    ThrowError = 0x2A,
    Function = 0x2B,
    Call = 0x2C,
    CurrentFunction = 0x2D,
    GetCapture = 0x2E,
    NewCell = 0x2F,
    CellInit = 0x30,
    CellGet = 0x31,
    CellSet = 0x32,
    AtomNew = 0x33,
    AtomLoad = 0x34,
    AtomCas = 0x35,
    Need = 0x36,
}

/// Operand encoding of an opcode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OperandKind {
    None,
    /// u16 index into the constant pool.
    Const,
    /// u16 index into the frame's locals.
    Local,
    /// u32 absolute byte offset of the branch target.
    Target,
    /// u16 host-defined operation selector, passed through to the host.
    HostOp,
    /// u16 element (vector) or entry (map) count of a constructor.
    Count,
    /// u16 [`ThrowKind`] selector.
    ThrowKind,
    /// u16 index into the executable's child function table.
    Function,
    /// u16 number of call arguments.
    ArgCount,
    /// u16 index into the running closure's captured values.
    Capture,
}

impl OperandKind {
    pub(crate) fn size(self) -> usize {
        match self {
            OperandKind::None => 0,
            OperandKind::Const
            | OperandKind::Local
            | OperandKind::HostOp
            | OperandKind::Count
            | OperandKind::ThrowKind
            | OperandKind::Function
            | OperandKind::ArgCount
            | OperandKind::Capture => 2,
            OperandKind::Target => 4,
        }
    }
}

impl Opcode {
    /// Decode an opcode byte; `None` for unassigned bytes.
    pub fn from_byte(byte: u8) -> Option<Opcode> {
        use Opcode::*;
        Some(match byte {
            0x01 => Const,
            0x02 => Undefined,
            0x03 => GetLocal,
            0x04 => SetLocal,
            0x05 => Pop,
            0x06 => Dup,
            0x07 => Add,
            0x08 => Sub,
            0x09 => Mul,
            0x0A => Lt,
            0x0B => StrictEq,
            0x0C => Not,
            0x0D => Jump,
            0x0E => JumpIfFalse,
            0x0F => Return,
            0x10 => NewObject,
            0x11 => HostEffect,
            0x12 => NewVector,
            0x13 => NewMap,
            0x14 => Count,
            0x15 => Nth,
            0x16 => Get,
            0x17 => GetOr,
            0x18 => Assoc,
            0x19 => Dissoc,
            0x1A => Conj,
            0x1B => PopLast,
            0x1C => NewVectorBuilder,
            0x1D => NewMapBuilder,
            0x1E => BuilderConj,
            0x1F => BuilderAssoc,
            0x20 => Freeze,
            0x21 => Div,
            0x22 => Rem,
            0x23 => Neg,
            0x24 => ToNumber,
            0x25 => ToNumberPair,
            0x26 => Gt,
            0x27 => Le,
            0x28 => Ge,
            0x29 => LooseEq,
            0x2A => ThrowError,
            0x2B => Function,
            0x2C => Call,
            0x2D => CurrentFunction,
            0x2E => GetCapture,
            0x2F => NewCell,
            0x30 => CellInit,
            0x31 => CellGet,
            0x32 => CellSet,
            0x33 => AtomNew,
            0x34 => AtomLoad,
            0x35 => AtomCas,
            0x36 => Need,
            _ => return None,
        })
    }

    /// Mnemonic used in diagnostics.
    pub fn name(self) -> &'static str {
        use Opcode::*;
        match self {
            Const => "const",
            Undefined => "undefined",
            GetLocal => "get_local",
            SetLocal => "set_local",
            Pop => "pop",
            Dup => "dup",
            Add => "add",
            Sub => "sub",
            Mul => "mul",
            Lt => "lt",
            StrictEq => "strict_eq",
            Not => "not",
            Jump => "jump",
            JumpIfFalse => "jump_if_false",
            Return => "return",
            NewObject => "new_object",
            HostEffect => "host_effect",
            NewVector => "new_vector",
            NewMap => "new_map",
            Count => "count",
            Nth => "nth",
            Get => "get",
            GetOr => "get_or",
            Assoc => "assoc",
            Dissoc => "dissoc",
            Conj => "conj",
            PopLast => "pop_last",
            NewVectorBuilder => "new_vector_builder",
            NewMapBuilder => "new_map_builder",
            BuilderConj => "builder_conj",
            BuilderAssoc => "builder_assoc",
            Freeze => "freeze",
            Div => "div",
            Rem => "rem",
            Neg => "neg",
            ToNumber => "to_number",
            ToNumberPair => "to_number_pair",
            Gt => "gt",
            Le => "le",
            Ge => "ge",
            LooseEq => "loose_eq",
            ThrowError => "throw_error",
            Function => "function",
            Call => "call",
            CurrentFunction => "current_function",
            GetCapture => "get_capture",
            NewCell => "new_cell",
            CellInit => "cell_init",
            CellGet => "cell_get",
            CellSet => "cell_set",
            AtomNew => "atom_new",
            AtomLoad => "atom_load",
            AtomCas => "atom_cas",
            Need => "need",
        }
    }

    pub(crate) fn operand(self) -> OperandKind {
        use Opcode::*;
        match self {
            Const => OperandKind::Const,
            GetLocal | SetLocal => OperandKind::Local,
            Jump | JumpIfFalse => OperandKind::Target,
            HostEffect => OperandKind::HostOp,
            NewVector | NewMap => OperandKind::Count,
            ThrowError => OperandKind::ThrowKind,
            Function => OperandKind::Function,
            Call => OperandKind::ArgCount,
            GetCapture => OperandKind::Capture,
            Undefined | Pop | Dup | Add | Sub | Mul | Lt | StrictEq | Not | Return | NewObject
            | Count | Nth | Get | GetOr | Assoc | Dissoc | Conj | PopLast | NewVectorBuilder
            | NewMapBuilder | BuilderConj | BuilderAssoc | Freeze | Div | Rem | Neg | ToNumber
            | ToNumberPair | Gt | Le | Ge | LooseEq | CurrentFunction | NewCell | CellInit
            | CellGet | CellSet | AtomNew | AtomLoad | AtomCas | Need => OperandKind::None,
        }
    }
}

/// The statically known JavaScript error a `ThrowError` instruction raises
/// (its u16 operand). The compiler emits these where JavaScript throws at run
/// time for a reason known at compile time, so the error still happens only
/// if and when that code runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u16)]
pub enum ThrowKind {
    /// ReferenceError: a reference to a name with no binding (strict mode).
    Unresolvable = 0,
    /// ReferenceError: a `let`/`const` binding used or assigned before its
    /// declaration ran (temporal dead zone).
    Uninitialized = 1,
    /// TypeError: assignment to a `const` binding.
    ConstAssignment = 2,
}

impl ThrowKind {
    /// Decode an operand; `None` for unassigned values.
    pub fn from_u16(code: u16) -> Option<ThrowKind> {
        Some(match code {
            0 => ThrowKind::Unresolvable,
            1 => ThrowKind::Uninitialized,
            2 => ThrowKind::ConstAssignment,
            _ => return None,
        })
    }
}

/// A decoded, validated instruction. Branch targets are instruction indices.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Instr {
    Const(u16),
    Undefined,
    GetLocal(u16),
    SetLocal(u16),
    Pop,
    Dup,
    Add,
    Sub,
    Mul,
    Lt,
    StrictEq,
    Not,
    Jump(u32),
    JumpIfFalse(u32),
    Return,
    NewObject,
    HostEffect(u16),
    /// Element count.
    NewVector(u16),
    /// Entry count (pops twice as many values).
    NewMap(u16),
    Count,
    Nth,
    Get,
    GetOr,
    Assoc,
    Dissoc,
    Conj,
    PopLast,
    NewVectorBuilder,
    NewMapBuilder,
    BuilderConj,
    BuilderAssoc,
    Freeze,
    Div,
    Rem,
    Neg,
    ToNumber,
    ToNumberPair,
    Gt,
    Le,
    Ge,
    LooseEq,
    ThrowError(ThrowKind),
    /// Index into the child function table, and the number of captured
    /// values the child declares (copied from it at load time, so the stack
    /// effect is known without the table).
    Function(u16, u16),
    /// Argument count.
    Call(u16),
    CurrentFunction,
    /// Index into the running closure's captured values.
    GetCapture(u16),
    NewCell,
    CellInit,
    CellGet,
    CellSet,
    AtomNew,
    AtomLoad,
    AtomCas,
    Need,
}

impl Instr {
    pub(crate) fn opcode(self) -> Opcode {
        match self {
            Instr::Const(_) => Opcode::Const,
            Instr::Undefined => Opcode::Undefined,
            Instr::GetLocal(_) => Opcode::GetLocal,
            Instr::SetLocal(_) => Opcode::SetLocal,
            Instr::Pop => Opcode::Pop,
            Instr::Dup => Opcode::Dup,
            Instr::Add => Opcode::Add,
            Instr::Sub => Opcode::Sub,
            Instr::Mul => Opcode::Mul,
            Instr::Lt => Opcode::Lt,
            Instr::StrictEq => Opcode::StrictEq,
            Instr::Not => Opcode::Not,
            Instr::Jump(_) => Opcode::Jump,
            Instr::JumpIfFalse(_) => Opcode::JumpIfFalse,
            Instr::Return => Opcode::Return,
            Instr::NewObject => Opcode::NewObject,
            Instr::HostEffect(_) => Opcode::HostEffect,
            Instr::NewVector(_) => Opcode::NewVector,
            Instr::NewMap(_) => Opcode::NewMap,
            Instr::Count => Opcode::Count,
            Instr::Nth => Opcode::Nth,
            Instr::Get => Opcode::Get,
            Instr::GetOr => Opcode::GetOr,
            Instr::Assoc => Opcode::Assoc,
            Instr::Dissoc => Opcode::Dissoc,
            Instr::Conj => Opcode::Conj,
            Instr::PopLast => Opcode::PopLast,
            Instr::NewVectorBuilder => Opcode::NewVectorBuilder,
            Instr::NewMapBuilder => Opcode::NewMapBuilder,
            Instr::BuilderConj => Opcode::BuilderConj,
            Instr::BuilderAssoc => Opcode::BuilderAssoc,
            Instr::Freeze => Opcode::Freeze,
            Instr::Div => Opcode::Div,
            Instr::Rem => Opcode::Rem,
            Instr::Neg => Opcode::Neg,
            Instr::ToNumber => Opcode::ToNumber,
            Instr::ToNumberPair => Opcode::ToNumberPair,
            Instr::Gt => Opcode::Gt,
            Instr::Le => Opcode::Le,
            Instr::Ge => Opcode::Ge,
            Instr::LooseEq => Opcode::LooseEq,
            Instr::ThrowError(_) => Opcode::ThrowError,
            Instr::Function(..) => Opcode::Function,
            Instr::Call(_) => Opcode::Call,
            Instr::CurrentFunction => Opcode::CurrentFunction,
            Instr::GetCapture(_) => Opcode::GetCapture,
            Instr::NewCell => Opcode::NewCell,
            Instr::CellInit => Opcode::CellInit,
            Instr::CellGet => Opcode::CellGet,
            Instr::CellSet => Opcode::CellSet,
            Instr::AtomNew => Opcode::AtomNew,
            Instr::AtomLoad => Opcode::AtomLoad,
            Instr::AtomCas => Opcode::AtomCas,
            Instr::Need => Opcode::Need,
        }
    }

    /// Operand-stack effect as (values popped, values pushed). Constructors
    /// pop as many values as their count operand says.
    pub(crate) fn stack_effect(self) -> (usize, usize) {
        use Instr::*;
        match self {
            Const(_) | Undefined | GetLocal(_) | NewObject | NewVectorBuilder | NewMapBuilder
            | ThrowError(_) | CurrentFunction | GetCapture(_) | NewCell => (0, 1),
            SetLocal(_) | Pop | JumpIfFalse(_) | Return => (1, 0),
            CellInit | CellSet => (2, 0),
            Dup => (1, 2),
            Add | Sub | Mul | Lt | StrictEq | Nth | Get | Dissoc | Conj | BuilderConj | Div
            | Rem | Gt | Le | Ge | LooseEq | Need => (2, 1),
            ToNumberPair => (2, 2),
            AtomNew | AtomLoad => (1, 1),
            AtomCas => (3, 2),
            GetOr | Assoc | BuilderAssoc => (3, 1),
            Not | HostEffect(_) | Count | PopLast | Freeze | Neg | ToNumber | CellGet => (1, 1),
            Jump(_) => (0, 0),
            NewVector(n) => (usize::from(n), 1),
            NewMap(n) => (2 * usize::from(n), 1),
            Call(n) => (usize::from(n) + 1, 1),
            Function(_, captures) => (usize::from(captures), 1),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_assigned_byte_round_trips_and_zero_is_invalid() {
        assert_eq!(Opcode::from_byte(0), None);
        let mut assigned = 0;
        for byte in 0..=u8::MAX {
            if let Some(op) = Opcode::from_byte(byte) {
                assert_eq!(op as u8, byte);
                assigned += 1;
            }
        }
        assert_eq!(assigned, 54);
    }
}
