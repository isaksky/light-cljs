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
//! code or encoding is reused.

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
}

impl OperandKind {
    pub(crate) fn size(self) -> usize {
        match self {
            OperandKind::None => 0,
            OperandKind::Const | OperandKind::Local | OperandKind::HostOp => 2,
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
        }
    }

    pub(crate) fn operand(self) -> OperandKind {
        use Opcode::*;
        match self {
            Const => OperandKind::Const,
            GetLocal | SetLocal => OperandKind::Local,
            Jump | JumpIfFalse => OperandKind::Target,
            HostEffect => OperandKind::HostOp,
            Undefined | Pop | Dup | Add | Sub | Mul | Lt | StrictEq | Not | Return | NewObject => {
                OperandKind::None
            }
        }
    }

    /// Operand-stack effect as (values popped, values pushed).
    pub(crate) fn stack_effect(self) -> (usize, usize) {
        use Opcode::*;
        match self {
            Const | Undefined | GetLocal | NewObject => (0, 1),
            SetLocal | Pop | JumpIfFalse | Return => (1, 0),
            Dup => (1, 2),
            Add | Sub | Mul | Lt | StrictEq => (2, 1),
            Not | HostEffect => (1, 1),
            Jump => (0, 0),
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
        assert_eq!(assigned, 17);
    }
}
