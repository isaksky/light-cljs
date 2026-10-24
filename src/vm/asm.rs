//! Bytecode assembler: an **early VM test tool**.
//!
//! Hand-assembled bytecode exists so the VM core can be tested before a
//! JavaScript frontend exists. It is not JavaScript source support; programs
//! built here are early VM tests only (`docs/design.md` §14).
//!
//! ```
//! use clj_quickjs::{Assembler, Opcode, Primitive};
//! let mut asm = Assembler::new();
//! asm.constant(Primitive::Number(2.0)).constant(Primitive::Number(3.0));
//! asm.op(Opcode::Add).op(Opcode::Return);
//! let exe = asm.finish("add", 0, 0).unwrap();
//! assert_eq!(exe.name(), "add");
//! ```

use super::executable::{Executable, ExecutableSpec, ValidationError};
use super::instr::{Opcode, OperandKind};
use crate::value::Primitive;

/// A forward- or backward-referenceable branch target.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label(usize);

/// Builds encoded bytecode plus its constant pool. See the module docs.
#[derive(Debug, Default)]
pub struct Assembler {
    code: Vec<u8>,
    constants: Vec<Primitive>,
    labels: Vec<Option<u32>>,
    /// (byte position of a u32 target operand, label)
    fixups: Vec<(usize, Label)>,
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// Emit an operand-less opcode. Opcodes that take an operand must use
    /// their dedicated method; emitting them here writes only the opcode byte
    /// (useful for building deliberately truncated code in tests).
    pub fn op(&mut self, op: Opcode) -> &mut Self {
        self.code.push(op as u8);
        self
    }

    /// Append a raw byte (for malformed-code tests).
    pub fn raw(&mut self, byte: u8) -> &mut Self {
        self.code.push(byte);
        self
    }

    /// Add `value` to the constant pool and emit `Const` for it.
    pub fn constant(&mut self, value: Primitive) -> &mut Self {
        let index = self.constants.len() as u16;
        self.constants.push(value);
        self.op(Opcode::Const).u16(index)
    }

    pub fn get_local(&mut self, index: u16) -> &mut Self {
        self.op(Opcode::GetLocal).u16(index)
    }

    pub fn set_local(&mut self, index: u16) -> &mut Self {
        self.op(Opcode::SetLocal).u16(index)
    }

    pub fn jump(&mut self, label: Label) -> &mut Self {
        self.branch(Opcode::Jump, label)
    }

    pub fn jump_if_false(&mut self, label: Label) -> &mut Self {
        self.branch(Opcode::JumpIfFalse, label)
    }

    /// Create an unbound label.
    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label(self.labels.len() - 1)
    }

    /// Bind `label` to the current position.
    pub fn bind(&mut self, label: Label) -> &mut Self {
        // A label from another assembler is ignored and stays unbound here.
        if let Some(slot) = self.labels.get_mut(label.0) {
            *slot = Some(self.code.len() as u32);
        }
        self
    }

    fn u16(&mut self, v: u16) -> &mut Self {
        self.code.extend_from_slice(&v.to_le_bytes());
        self
    }

    fn branch(&mut self, op: Opcode, label: Label) -> &mut Self {
        debug_assert_eq!(op.operand(), OperandKind::Target);
        self.op(op);
        self.fixups.push((self.code.len(), label));
        self.code.extend_from_slice(&u32::MAX.to_le_bytes());
        self
    }

    /// Resolve labels and return the encoded spec. Unbound labels keep the
    /// target `u32::MAX`, which validation rejects.
    pub fn into_spec(mut self, name: &str, params: u16, locals: u16) -> ExecutableSpec {
        for &(at, label) in &self.fixups {
            if let Some(target) = self.labels.get(label.0).copied().flatten() {
                self.code[at..at + 4].copy_from_slice(&target.to_le_bytes());
            }
        }
        ExecutableSpec {
            name: name.to_string(),
            params,
            locals,
            code: self.code,
            constants: self.constants,
        }
    }

    /// Resolve labels and load (validate) the executable.
    pub fn finish(
        &mut self,
        name: &str,
        params: u16,
        locals: u16,
    ) -> Result<Executable, ValidationError> {
        Executable::load(std::mem::take(self).into_spec(name, params, locals))
    }
}
