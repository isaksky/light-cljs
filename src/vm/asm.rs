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
use std::sync::atomic::{AtomicU64, Ordering};

/// A forward- or backward-referenceable branch target.
///
/// A label belongs to the assembler that created it, and only until that
/// assembler is reset by [`Assembler::finish`]. A foreign or stale label is
/// never resolved: binding it has no effect and branches to it keep the
/// invalid target `u32::MAX`, so validation rejects the code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Label {
    owner: u64,
    index: usize,
}

/// Source of assembler identities for label ownership checks.
static NEXT_ASSEMBLER: AtomicU64 = AtomicU64::new(1);

/// Builds encoded bytecode plus its constant pool. See the module docs.
#[derive(Debug)]
pub struct Assembler {
    /// Identity checked against [`Label::owner`].
    id: u64,
    code: Vec<u8>,
    constants: Vec<Primitive>,
    functions: Vec<Executable>,
    /// Declared captured values ([`ExecutableSpec::captures`]).
    captures: u16,
    labels: Vec<Option<u32>>,
    /// (byte position of a u32 target operand, label)
    fixups: Vec<(usize, Label)>,
}

impl Default for Assembler {
    fn default() -> Self {
        Assembler {
            // Relaxed: only uniqueness matters.
            id: NEXT_ASSEMBLER.fetch_add(1, Ordering::Relaxed),
            code: Vec::new(),
            constants: Vec::new(),
            functions: Vec::new(),
            captures: 0,
            labels: Vec::new(),
            fixups: Vec::new(),
        }
    }
}

impl Assembler {
    pub fn new() -> Self {
        Self::default()
    }

    /// The slot of `label` if this assembler created it.
    fn own(&self, label: Label) -> Option<usize> {
        (label.owner == self.id && label.index < self.labels.len()).then_some(label.index)
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

    /// Add `value` to the constant pool and emit `Const` for it. Pools larger
    /// than the u16 index range are rejected by validation.
    pub fn constant(&mut self, value: Primitive) -> &mut Self {
        let index = u16::try_from(self.constants.len()).unwrap_or(u16::MAX);
        self.constants.push(value);
        self.op(Opcode::Const).u16(index)
    }

    pub fn get_local(&mut self, index: u16) -> &mut Self {
        self.op(Opcode::GetLocal).u16(index)
    }

    pub fn set_local(&mut self, index: u16) -> &mut Self {
        self.op(Opcode::SetLocal).u16(index)
    }

    /// Emit `HostEffect` with the host-defined operation selector `op`.
    pub fn host_effect(&mut self, op: u16) -> &mut Self {
        self.op(Opcode::HostEffect).u16(op)
    }

    /// Emit `NewVector` taking the top `count` operands as elements.
    pub fn new_vector(&mut self, count: u16) -> &mut Self {
        self.op(Opcode::NewVector).u16(count)
    }

    /// Emit `NewMap` taking the top `2 * entries` operands as key/value pairs.
    pub fn new_map(&mut self, entries: u16) -> &mut Self {
        self.op(Opcode::NewMap).u16(entries)
    }

    /// Add `code` to the child function table (sharing it, not copying it)
    /// and emit `Function` for it: at run time this pops the captured values
    /// `code` declares and pushes a new function object (closure) for `code`. Tables larger than the u16 index range are rejected
    /// by validation.
    pub fn function(&mut self, code: &Executable) -> &mut Self {
        let index = u16::try_from(self.functions.len()).unwrap_or(u16::MAX);
        self.functions.push(code.clone());
        self.op(Opcode::Function).u16(index)
    }

    /// Declare that this code is a closure body carrying `count` captured
    /// values ([`ExecutableSpec::captures`]); a parent's `Function` for it
    /// pops that many. Reset, like everything else, by [`Assembler::finish`].
    pub fn captures(&mut self, count: u16) -> &mut Self {
        self.captures = count;
        self
    }

    /// Emit `GetCapture`: push captured value `index` of the running closure.
    pub fn get_capture(&mut self, index: u16) -> &mut Self {
        self.op(Opcode::GetCapture).u16(index)
    }

    /// Emit `Call` with `argc` arguments: pops the callee and the arguments
    /// (pushed after it, first argument first), pushes the callee's result.
    pub fn call(&mut self, argc: u16) -> &mut Self {
        self.op(Opcode::Call).u16(argc)
    }

    pub fn jump(&mut self, label: Label) -> &mut Self {
        self.branch(Opcode::Jump, label)
    }

    pub fn jump_if_false(&mut self, label: Label) -> &mut Self {
        self.branch(Opcode::JumpIfFalse, label)
    }

    /// Emit `PushCatch` with the catch block at `label`.
    pub fn push_catch(&mut self, label: Label) -> &mut Self {
        self.branch(Opcode::PushCatch, label)
    }

    /// Emit `PushFinally` with the finally block at `label`.
    pub fn push_finally(&mut self, label: Label) -> &mut Self {
        self.branch(Opcode::PushFinally, label)
    }

    /// Create an unbound label.
    pub fn label(&mut self) -> Label {
        self.labels.push(None);
        Label {
            owner: self.id,
            index: self.labels.len() - 1,
        }
    }

    /// Bind `label` to the current position.
    pub fn bind(&mut self, label: Label) -> &mut Self {
        // Foreign or stale labels are ignored; see [`Label`].
        if let Some(index) = self.own(label) {
            self.labels[index] = Some(self.code.len() as u32);
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

    /// Resolve labels and return the encoded spec. Branches to unbound,
    /// foreign or stale labels keep the target `u32::MAX`, which validation
    /// rejects.
    pub fn into_spec(mut self, name: &str, params: u16, locals: u16) -> ExecutableSpec {
        for &(at, label) in &self.fixups {
            let target = self.own(label).and_then(|i| self.labels[i]);
            if let Some(target) = target {
                self.code[at..at + 4].copy_from_slice(&target.to_le_bytes());
            }
        }
        ExecutableSpec {
            name: name.to_string(),
            params,
            locals,
            code: self.code,
            constants: self.constants,
            functions: self.functions,
            captures: self.captures,
            strict: true,
            constructor: false,
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
