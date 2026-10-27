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
//! [`SyntaxErrorKind::Invalid`](super::SyntaxErrorKind::Invalid).
//!
//! **`__rt.need(definition, kind)`** (clj-quickjs-23, `docs/design.md` §7)
//! is the dependency read: it lowers to the VM `need` instruction, which
//! records the dependency on result `kind` of `definition` in the running
//! task's world (the world is execution context, never an argument) and
//! evaluates to the shared result, suspending the whole task while the
//! result is pending and continuing at this call when the task is run
//! again ([`crate::task`], "Dependency waits in the VM"). Both arguments are
//! expressions; a numeric literal argument that is not a valid identity (a
//! non-negative integer, at most 2^53 - 1 for the definition and
//! 4,294,967,295 for the kind) is rejected at compile time, other values at
//! run time.
//!
//! **`__rt.reduce(f, init, vec)`** and **`__rt.needAll(defs, kind)`**
//! (clj-quickjs-12) are resumable native operations (`crate::vm::native`):
//! `reduce` calls the JavaScript function `f(acc, x)` for each element of the
//! persistent vector `vec` in order and evaluates to the last result (`init`
//! for an empty vector); `needAll` needs result `kind` of each definition id
//! in `defs` in order and evaluates to a new vector of the results. Either
//! may suspend the whole task part-way (in a callback, or at a pending
//! dependency) and continues where it stopped when the task is run again,
//! without repeating completed elements.
//!
//! **`__rt.formList(…)`, `__rt.formVector(…)`, `__rt.formMap(k, v, …)`**
//! and the fixed-arity **`__rt.formKeyword(ns, name)`** … **`__rt.formValue(v)`**
//! (light-cljs-466) construct, inspect, compare and update native Clojure
//! form values ([`crate::collections::form`]); the table is in
//! [`crate::vm::intrinsics`] ("Form intrinsics"). **`__rt.formSet(…)`**,
//! **`__rt.formDisj(s, x)`** and the **`__rt.clj*`** ClojureScript core
//! operations (`cljCount(c)` … `cljWithMeta(v, m)`, light-cljs-477;
//! [`crate::collections::clj`]) are in the same table, and so are the lazy
//! sequence operations `__rt.lazySeq(thunk)` and `__rt.cljChunk*`
//! (light-cljs-479; [`crate::collections::lazy`]).
//!
//! **`__rt.call(receiver, callee, args…)`** (light-cljs-501) uses the ordinary
//! `CallMethod` instruction with an explicitly evaluated receiver and callable.
//! Operands evaluate left to right; call errors and dependency waits use the
//! same VM frames as a JavaScript method call.
//!
//! A call evaluates its arguments left to right, then runs one VM
//! instruction; the operand order is the instruction's stack order
//! ([`crate::vm::intrinsics`] documents operands, results and errors).

use crate::vm::{FormOp, Opcode};

/// The name of the compiler-reserved intrinsic binding.
pub(crate) const RESERVED_BINDING: &str = "__rt";

/// How many arguments an intrinsic takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Arity {
    Exactly(usize),
    AtLeast(usize),
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
    /// `__rt.call(receiver, callee, ...args)`: evaluate the receiver and
    /// callable before the operands, then use the ordinary VM method call.
    Call,
    /// `__rt.effect(op, arg)`: `op` must be an integer literal in
    /// `0..=65535` (the `host_effect` operand); `arg` is evaluated, then
    /// `host_effect op`.
    HostEffect,
    /// `__rt.need(definition, kind)`: both arguments, then `need` (a
    /// suspension point). Literal arguments are range-checked when compiled.
    Need,
    /// The arguments, then `form_op` with this operation as its operand
    /// (`__rt.formKeyword(ns, name)` and the other fixed-arity form
    /// intrinsics).
    FormOp(FormOp),
    /// The arguments, then this instruction, then `pop`: an instruction
    /// that pushes two results evaluates to the lower one
    /// (`__rt.atomCas(a, e, n)` is `atom_cas`'s witness, light-cljs-480).
    OpThenPop(Opcode),
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
        name: "call",
        arity: Arity::AtLeast(2),
        lowering: Lowering::Call,
    },
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
        lowering: Lowering::Need,
    },
    // Shared atoms (`crate::atom`; light-cljs-480): a new multi-writer
    // atom, its current value, and an identity compare-and-set evaluating
    // to its witness (`expected` itself when it swapped).
    op("atomNew", 1, Opcode::AtomNew),
    op("atomLoad", 1, Opcode::AtomLoad),
    Intrinsic {
        name: "atomCas",
        arity: Arity::Exactly(3),
        lowering: Lowering::OpThenPop(Opcode::AtomCas),
    },
    op("reduce", 3, Opcode::Reduce),
    op("needAll", 2, Opcode::NeedAll),
    Intrinsic {
        name: "formList",
        arity: Arity::Any,
        lowering: Lowering::Counted(Opcode::NewFormList),
    },
    Intrinsic {
        name: "formVector",
        arity: Arity::Any,
        lowering: Lowering::Counted(Opcode::NewFormVector),
    },
    Intrinsic {
        name: "formMap",
        arity: Arity::Pairs,
        lowering: Lowering::Counted(Opcode::NewFormMap),
    },
    form_op(FormOp::Keyword),
    form_op(FormOp::Symbol),
    form_op(FormOp::Kind),
    form_op(FormOp::Namespace),
    form_op(FormOp::Name),
    form_op(FormOp::Count),
    form_op(FormOp::Nth),
    form_op(FormOp::Get),
    form_op(FormOp::Assoc),
    form_op(FormOp::Dissoc),
    form_op(FormOp::Conj),
    form_op(FormOp::Equals),
    form_op(FormOp::Hash),
    form_op(FormOp::Meta),
    form_op(FormOp::WithMeta),
    form_op(FormOp::Value),
    form_op(FormOp::Keys),
    Intrinsic {
        name: "formSet",
        arity: Arity::Any,
        lowering: Lowering::Counted(Opcode::NewFormSet),
    },
    form_op(FormOp::Disj),
    form_op(FormOp::LiteralMap),
    form_op(FormOp::LiteralSet),
    form_op(FormOp::ArrayMap),
    form_op(FormOp::CljTruth),
    form_op(FormOp::CljCount),
    form_op(FormOp::CljSeq),
    form_op(FormOp::CljFirst),
    form_op(FormOp::CljRest),
    form_op(FormOp::CljNext),
    form_op(FormOp::CljCons),
    form_op(FormOp::CljNth),
    form_op(FormOp::CljNthOr),
    form_op(FormOp::CljGet),
    form_op(FormOp::CljContains),
    form_op(FormOp::CljAssoc),
    form_op(FormOp::CljDissoc),
    form_op(FormOp::CljConj),
    form_op(FormOp::CljDisj),
    form_op(FormOp::CljMeta),
    form_op(FormOp::CljWithMeta),
    form_op(FormOp::CljConcat),
    form_op(FormOp::CljConcatAll),
    form_op(FormOp::CljZipMap),
    form_op(FormOp::CljKeyword),
    form_op(FormOp::CljKeywordNs),
    form_op(FormOp::CljSymbol),
    form_op(FormOp::CljSymbolNs),
    // Structs and protocol dispatch (light-cljs-478).
    Intrinsic {
        name: "structNew",
        arity: Arity::Any,
        lowering: Lowering::Counted(Opcode::NewStruct),
    },
    form_op(FormOp::StructType),
    form_op(FormOp::StructField),
    form_op(FormOp::StructTypeOf),
    form_op(FormOp::RecordFromMap),
    form_op(FormOp::CljDispatch),
    form_op(FormOp::CljIsAtom),
    // Lazy and chunked sequences (light-cljs-479).
    form_op(FormOp::LazySeq),
    form_op(FormOp::CljRealized),
    form_op(FormOp::CljList),
    form_op(FormOp::CljChunkedSeq),
    form_op(FormOp::CljChunkFirst),
    form_op(FormOp::CljChunkRest),
    form_op(FormOp::CljChunkNext),
    form_op(FormOp::CljChunkCons),
    form_op(FormOp::CljRealizeAll),
    form_op(FormOp::CljRangeCons),
    form_op(FormOp::CljIsZero),
    form_op(FormOp::CljIsFalse),
    form_op(FormOp::CljInvoke),
    form_op(FormOp::CljReduced),
    form_op(FormOp::CljIsReduced),
    form_op(FormOp::CljUnreduced),
    form_op(FormOp::CljIsEditable),
    form_op(FormOp::PrintItems),
    form_op(FormOp::ReaderCollection),
    form_op(FormOp::SortedMap),
];

const fn form_op(op: FormOp) -> Intrinsic {
    Intrinsic {
        name: op.intrinsic(),
        arity: Arity::Exactly(op.arity()),
        lowering: Lowering::FormOp(op),
    }
}

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
        for op in FormOp::ALL {
            assert_eq!(
                lookup(op.intrinsic()).map(|i| i.lowering),
                Some(Lowering::FormOp(op))
            );
        }
        assert!(lookup("toString").is_none());
    }
}
