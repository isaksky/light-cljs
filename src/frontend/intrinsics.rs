//! The compiler-reserved intrinsic binding contract (`docs/design.md` §§5,
//! 7; `docs/grammar.md`, "Reserved intrinsic binding").
//!
//! The compiler, not the program, owns one binding, [`RESERVED_BINDING`]
//! (`__rt`). It is in scope everywhere, as if declared outside the script,
//! and it is not a JavaScript value: it denotes a fixed table of private VM
//! operations. The binding resolver finds it like any other binding (it is
//! the outermost scope), and the contract makes that resolution unambiguous:
//!
//! * **No shadowing.** Declaring `__rt` anywhere (`let`, `const`, a
//!   parameter, a function declaration or a function expression's name) is a
//!   compile error, so every `__rt` in a program resolves to the reserved
//!   binding. Intrinsics are never recognized by the spelling of an arbitrary
//!   callee: a user function named `vector` or `need` is an ordinary binding.
//! * **No reassignment.** `__rt = …` and `__rt.name = …` are compile errors.
//! * **No computed access.** `__rt[name]` is a compile (parse) error: the
//!   intrinsic must be known statically.
//! * **No aliasing.** `__rt` may appear only as `__rt.name(arguments)`, a
//!   direct call. `const r = __rt`, `f(__rt)`, `return __rt` and an uncalled
//!   `__rt.name` are compile errors, so an intrinsic never becomes a value
//!   that could escape the contract.
//! * **Known names and arities.** An unknown name, a wrong argument count or
//!   a non-literal host operation selector is a compile error.
//!
//! Misuse of the binding is reported as
//! [`SyntaxErrorKind::ReservedWord`](super::SyntaxErrorKind::ReservedWord);
//! unknown names and bad arguments as
//! [`SyntaxErrorKind::Invalid`](super::SyntaxErrorKind::Invalid);
//! `__rt.need`, whose lowering to a resumable dependency wait is pellet
//! clj-quickjs-23, as [`SyntaxErrorKind::Unsupported`](super::SyntaxErrorKind::Unsupported).
//!
//! A call evaluates its arguments left to right, then runs one VM
//! instruction; the operand order is the instruction's stack order
//! ([`crate::vm::intrinsics`] documents operands, results and errors).

use crate::vm::Opcode;

/// The name of the compiler-reserved intrinsic binding.
pub(crate) const RESERVED_BINDING: &str = "__rt";

/// How many arguments an intrinsic takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arity {
    Exactly(usize),
    /// Any number (up to the instruction's u16 count operand).
    Any,
    /// An even number: key/value pairs.
    Pairs,
}

/// What a call lowers to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Lowering {
    /// The arguments, then this operand-less instruction.
    Op(Opcode),
    /// The arguments, then this instruction with the element count (vector)
    /// or entry count (map) as its operand.
    Counted(Opcode),
    /// `__rt.effect(op, arg)`: `op` must be an integer literal in
    /// `0..=65535` (the `host_effect` operand); `arg` is evaluated, then
    /// `host_effect op`.
    HostEffect,
    /// Reserved, not lowered yet; the message names the pellet that adds it.
    Deferred(&'static str),
}

/// One entry of the intrinsic table.
#[derive(Clone, Copy, Debug)]
pub(crate) struct Intrinsic {
    pub(crate) name: &'static str,
    pub(crate) arity: Arity,
    pub(crate) lowering: Lowering,
}

const fn op(name: &'static str, n: usize, opcode: Opcode) -> Intrinsic {
    Intrinsic {
        name,
        arity: Arity::Exactly(n),
        lowering: Lowering::Op(opcode),
    }
}

/// The intrinsic table: `__rt.<name>(…)` lowers as listed.
pub(crate) const INTRINSICS: &[Intrinsic] = &[
    Intrinsic {
        name: "vector",
        arity: Arity::Any,
        lowering: Lowering::Counted(Opcode::NewVector),
    },
    Intrinsic {
        name: "map",
        arity: Arity::Pairs,
        lowering: Lowering::Counted(Opcode::NewMap),
    },
    op("count", 1, Opcode::Count),
    op("nth", 2, Opcode::Nth),
    op("get", 2, Opcode::Get),
    op("getOr", 3, Opcode::GetOr),
    op("assoc", 3, Opcode::Assoc),
    op("dissoc", 2, Opcode::Dissoc),
    op("conj", 2, Opcode::Conj),
    op("popLast", 1, Opcode::PopLast),
    op("vectorBuilder", 0, Opcode::NewVectorBuilder),
    op("mapBuilder", 0, Opcode::NewMapBuilder),
    op("builderConj", 2, Opcode::BuilderConj),
    op("builderAssoc", 3, Opcode::BuilderAssoc),
    op("freeze", 1, Opcode::Freeze),
    Intrinsic {
        name: "effect",
        arity: Arity::Exactly(2),
        lowering: Lowering::HostEffect,
    },
    Intrinsic {
        name: "need",
        arity: Arity::Exactly(2),
        lowering: Lowering::Deferred(
            "`__rt.need` (a resumable dependency wait) is not lowered yet (clj-quickjs-23)",
        ),
    },
];

/// The table entry for `name`, if it is an intrinsic.
pub(crate) fn lookup(name: &str) -> Option<&'static Intrinsic> {
    INTRINSICS.iter().find(|i| i.name == name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn intrinsic_names_are_distinct() {
        for (i, a) in INTRINSICS.iter().enumerate() {
            assert!(
                INTRINSICS[i + 1..].iter().all(|b| b.name != a.name),
                "{}",
                a.name
            );
        }
        assert!(lookup("vector").is_some());
        assert!(lookup("toString").is_none());
    }
}
