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
//!
//! Opcodes `0x12`–`0x20` are the private collection intrinsics and builder
//! operations. Their contract (operand validation, errors, the builder
//! lifecycle) is in [`crate::vm::intrinsics`]. They are VM operations a
//! compiler may lower known collection calls to, not user-visible globals.
//!
//! Byte `0x00` is deliberately unassigned so zeroed memory never decodes.
//! Branch targets are absolute byte offsets that must land on an instruction
//! boundary. The opcode list is an append-only registry: new opcodes take the
//! next byte and are handled in every exhaustive match.
//!
//! `Add`, `Sub`, `Mul` and `Lt` support Number operands only. Other operand
//! types fail the task with
//! [`VmErrorKind::UnsupportedOperand`](crate::VmErrorKind::UnsupportedOperand)
//! rather than applying JavaScript's coercions, which are deferred.
//!
//! The local/branch/return families follow the shape of QuickJS's
//! `get_loc`/`put_loc`, `goto`/`if_false` and `return` opcodes
//! (`research/quickjs/quickjs-opcode.h`, revision 7d1c13c5d8f8); no QuickJS
//! code or encoding is reused. The collection intrinsics follow QuickJS's
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
}

impl OperandKind {
    pub(crate) fn size(self) -> usize {
        match self {
            OperandKind::None => 0,
            OperandKind::Const | OperandKind::Local | OperandKind::HostOp | OperandKind::Count => 2,
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
            Undefined | Pop | Dup | Add | Sub | Mul | Lt | StrictEq | Not | Return | NewObject
            | Count | Nth | Get | GetOr | Assoc | Dissoc | Conj | PopLast | NewVectorBuilder
            | NewMapBuilder | BuilderConj | BuilderAssoc | Freeze => OperandKind::None,
        }
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
        }
    }

    /// Operand-stack effect as (values popped, values pushed). Constructors
    /// pop as many values as their count operand says.
    pub(crate) fn stack_effect(self) -> (usize, usize) {
        use Instr::*;
        match self {
            Const(_) | Undefined | GetLocal(_) | NewObject | NewVectorBuilder | NewMapBuilder => {
                (0, 1)
            }
            SetLocal(_) | Pop | JumpIfFalse(_) | Return => (1, 0),
            Dup => (1, 2),
            Add | Sub | Mul | Lt | StrictEq | Nth | Get | Dissoc | Conj | BuilderConj => (2, 1),
            GetOr | Assoc | BuilderAssoc => (3, 1),
            Not | HostEffect(_) | Count | PopLast | Freeze => (1, 1),
            Jump(_) => (0, 0),
            NewVector(n) => (usize::from(n), 1),
            NewMap(n) => (2 * usize::from(n), 1),
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
        assert_eq!(assigned, 32);
    }
}
