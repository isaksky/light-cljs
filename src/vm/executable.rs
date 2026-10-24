//! Immutable, shared executable code and its load-time validation.
//!
//! [`Executable::load`] decodes the byte format of [`super::instr`] and
//! rejects malformed code before any task can run it: unknown opcodes,
//! truncated operands, out-of-range constant/local indices, branch targets
//! that are not instruction boundaries, operand-stack underflow, inconsistent
//! stack depth where control flow merges, and control falling off the end.
//! The stack check also computes the maximum operand-stack depth, in the
//! spirit of QuickJS's `compute_stack_size` (`research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8); the algorithm here is written fresh.
//!
//! A loaded [`Executable`] is an `Arc` handle: cloning it or spawning tasks
//! from it never copies the code. Constants are primitives only in this slice,
//! so an executable holds no heap edges and is not tied to one runtime.

use std::fmt;
use std::sync::Arc;

use super::instr::{Instr, Opcode, OperandKind};
use crate::value::{Primitive, Value};

/// Input to [`Executable::load`].
#[derive(Clone, Debug, PartialEq)]
pub struct ExecutableSpec {
    /// Name used in diagnostics.
    pub name: String,
    /// Number of parameters; arguments are copied into locals `0..params`.
    pub params: u16,
    /// Number of locals (parameters included).
    pub locals: u16,
    /// Encoded instructions.
    pub code: Vec<u8>,
    /// Constant pool referenced by `Const`.
    pub constants: Vec<Primitive>,
}

/// Validated code shared by every task that runs it.
#[derive(Debug)]
pub(crate) struct Code {
    pub(crate) name: Arc<str>,
    pub(crate) params: u16,
    pub(crate) locals: u16,
    pub(crate) constants: Box<[Value]>,
    pub(crate) instrs: Box<[Instr]>,
    /// Byte offset of each instruction, for diagnostics.
    pub(crate) offsets: Box<[u32]>,
    pub(crate) max_stack: usize,
}

/// Immutable executable code; a cheap-to-clone shared handle.
#[derive(Clone, Debug)]
pub struct Executable(pub(crate) Arc<Code>);

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

fn err(offset: u32, kind: ValidationErrorKind) -> ValidationError {
    ValidationError { offset, kind }
}

impl Executable {
    /// Validate and load code. Never panics on malformed input.
    pub fn load(spec: ExecutableSpec) -> Result<Executable, ValidationError> {
        use ValidationErrorKind::*;
        let ExecutableSpec {
            name,
            params,
            locals,
            code,
            constants,
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
                OperandKind::Const if operand as usize >= constants.len() => {
                    let index = operand as u16;
                    let count = constants.len();
                    return Err(err(offset, ConstantOutOfRange { index, count }));
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
            });
        }

        let max_stack = check_stack(&instrs, &offsets)?;
        Ok(Executable(Arc::new(Code {
            name: name.into(),
            params,
            locals,
            constants: constants.into_iter().map(Value::from).collect(),
            instrs: instrs.into(),
            offsets: offsets.into(),
            max_stack,
        })))
    }

    /// Diagnostic name.
    pub fn name(&self) -> &str {
        &self.0.name
    }

    /// Maximum operand-stack depth computed at load time.
    pub fn max_stack(&self) -> usize {
        self.0.max_stack
    }

    /// True when both handles share the same loaded code (no copy was made).
    pub fn same_code(a: &Executable, b: &Executable) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
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
        let (pops, pushes) = opcode.stack_effect();
        if depth < pops {
            return Err(err(offset, StackUnderflow { opcode, depth }));
        }
        let after = depth - pops + pushes;
        max = max.max(after);
        let (a, b) = match instr {
            Instr::Return => (None, None),
            Instr::Jump(t) => (Some(t as usize), None),
            Instr::JumpIfFalse(t) => (Some(t as usize), Some(i + 1)),
            _ => (Some(i + 1), None),
        };
        for next in [a, b].into_iter().flatten() {
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
            locals,
            code,
            constants,
        })
    }

    fn kind(r: Result<Executable, ValidationError>) -> ValidationErrorKind {
        r.unwrap_err().kind
    }

    const RET: u8 = Opcode::Return as u8;
    const UNDEF: u8 = Opcode::Undefined as u8;

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
            locals: 2,
            code: vec![UNDEF, RET],
            constants: vec![],
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
}
