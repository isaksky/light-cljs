//! Collection intrinsics and task-owned transient builders
//! (`docs/design.md` §§4–5).
//!
//! Private VM operations over the persistent collections of
//! [`crate::collections`]. They are opcodes (`0x12`–`0x20`, see
//! [`crate::vm::instr`]), not globals or properties: no JavaScript name
//! reaches them, and they never run user hashing, equality, getters or
//! protocol code. A future compiler may lower known collection operations
//! to them. Every intrinsic validates its operands at run time; a rejected
//! operand fails the task with a [`VmError`](crate::VmError) whose kind is
//! [`VmErrorKind::Collection`](crate::VmErrorKind::Collection) or
//! [`VmErrorKind::Builder`](crate::VmErrorKind::Builder) and never panics.
//! (Operand *counts* are checked at load time like every other opcode.)
//!
//! # Persistent intrinsics
//!
//! Stack notation is (before → after), top of stack on the right. Index and
//! key rules are the collection contract of [`crate::collections`]: an index
//! is a number with an integral value in `0 ..= 2^53 - 1`; map keys compare
//! by SameValueZero, heap objects by identity.
//!
//! | Mnemonic | Stack | Result | Errors ([`CollectionError`]) |
//! |----------|-------|--------|--------|
//! | `new_vector n` | v1 … vn → vec | a new vector of the n values, in order | – |
//! | `new_map n` | k1 v1 … kn vn → map | a new map; a later duplicate key wins | – |
//! | `count` | coll → n | element or entry count (a number) | `WrongType` |
//! | `nth` | vec i → v | element at index `i` | `WrongType` (not a vector), `InvalidIndex`, `IndexOutOfBounds` |
//! | `get` | coll key → v | vector: element at index `key`; map: value for `key` | `WrongType`, `InvalidIndex`, `IndexOutOfBounds` (vector), `MissingKey` (map) |
//! | `get_or` | coll key d → v | as `get`, but `d` when out of bounds or absent | `WrongType`, `InvalidIndex` |
//! | `assoc` | coll key v → coll' | new version with `key` set (vector: index `<= count`, `count` appends) | `WrongType`, `InvalidIndex`, `IndexOutOfBounds` |
//! | `dissoc` | map key → map' | new version without `key` (absent: the same map) | `WrongType` |
//! | `conj` | vec v → vec' | new version with `v` appended | `WrongType` |
//! | `pop_last` | vec → vec' | new version without its last element | `WrongType`, `IndexOutOfBounds` (empty) |
//!
//! Updates are persistent with the identity rules of the collection
//! contract: the input never changes, untouched elements keep their
//! identity, and a no-op update returns the input object itself. Operand
//! kinds are checked before indices and keys. Occurrences are not
//! transparent here: `count` of an occurrence is `WrongType`. Any intrinsic
//! may also fail with [`VmErrorKind::OutOfMemory`](crate::VmErrorKind::OutOfMemory).
//!
//! # Builder intrinsics
//!
//! | Mnemonic | Stack | Effect | Errors |
//! |----------|-------|--------|--------|
//! | `new_vector_builder` | → b | a new open vector builder owned by the running task | – |
//! | `new_map_builder` | → b | a new open map builder owned by the running task | – |
//! | `builder_conj` | b v → b | append `v` in place (vector builders) | `WrongType`, lifecycle |
//! | `builder_assoc` | b key v → b | set `key` in place (vector: index `<= count`) | `WrongType`, `InvalidIndex`, `IndexOutOfBounds`, lifecycle |
//! | `freeze` | b → coll | a new persistent vector or map with the builder's contents; the builder becomes frozen | `WrongType`, lifecycle |
//!
//! "Lifecycle" errors are [`BuilderError`](crate::BuilderError)s:
//!
//! * [`BuilderError::NotOwner`](crate::BuilderError::NotOwner): the running
//!   task is not the builder's owner (cross-task mutation or freeze).
//! * [`BuilderError::Frozen`](crate::BuilderError::Frozen): use after
//!   `freeze`.
//! * [`BuilderError::OwnerFinished`](crate::BuilderError::OwnerFinished): the
//!   owning task has completed or failed, which released the builder.
//!
//! Ownership is the owning task's [`TaskId`](crate::TaskId), never an OS
//! thread: a task keeps using its builders after yielding and after being
//! resumed on another thread. The full lifecycle (create, mutate in place,
//! freeze, release on task termination), the order in which the checks are
//! made, and how builder contents are traced are documented in
//! `src/collections/builder.rs`.
//!
//! Builders are heap objects with identity; `===` compares them by identity
//! and they may be stored in locals, operands and even persistent
//! collections, but they are not shareable values: `Runtime::publish`
//! rejects any value that reaches one (`crate::value::share`).
//!
//! # Rooting
//!
//! Every intrinsic reads its operands in place and pops them only after its
//! result exists, so operands stay rooted by the task's frame while the
//! intrinsic allocates. Each call happens inside a run, a registered heap
//! mutator, so no collection can start mid-instruction.

use super::dispatch::Fault;
use super::frames::VmState;
use super::instr::Instr;
use crate::collections::builder::{self, BuildFault, BuilderKind};
use crate::collections::{self as coll, CollectionError};
use crate::heap::Heap;
use crate::value::Value;

impl From<CollectionError> for Fault {
    fn from(e: CollectionError) -> Self {
        match e {
            CollectionError::OutOfMemory => Fault::OutOfMemory("heap object table exhausted"),
            other => Fault::Collection(other),
        }
    }
}

impl From<BuildFault> for Fault {
    fn from(e: BuildFault) -> Self {
        match e {
            BuildFault::Collection(e) => e.into(),
            BuildFault::Builder(e) => Fault::Builder(e),
            BuildFault::Internal(m) => Fault::Internal(m),
        }
    }
}

/// The top `N` operands, oldest first, without popping them.
fn top<const N: usize>(ops: &[Value]) -> Result<[Value; N], Fault> {
    let start = ops
        .len()
        .checked_sub(N)
        .ok_or(Fault::Internal("operand stack underflow"))?;
    <[Value; N]>::try_from(&ops[start..]).map_err(|_| Fault::Internal("operand stack underflow"))
}

/// Replace the top `popped` operands with `result`.
fn replace(ops: &mut Vec<Value>, popped: usize, result: Value) {
    ops.truncate(ops.len().saturating_sub(popped));
    ops.push(result);
}

/// Execute one collection intrinsic or builder instruction.
pub(crate) fn step(state: &mut VmState, heap: &Heap, instr: Instr) -> Result<(), Fault> {
    let caller = state.diag.task;
    let ops = &mut state.frame.operands;
    match instr {
        Instr::NewVector(n) => {
            let n = usize::from(n);
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let vec = coll::new_vector(heap, &ops[start..])?;
            replace(ops, n, vec);
        }
        Instr::NewMap(n) => {
            let n = 2 * usize::from(n);
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let entries: Vec<(Value, Value)> = ops[start..]
                .chunks_exact(2)
                .map(|kv| (kv[0], kv[1]))
                .collect();
            let map = coll::new_map(heap, &entries)?;
            replace(ops, n, map);
        }
        Instr::Count => {
            let [c] = top(ops)?;
            let n = coll::count(heap, c)?;
            replace(ops, 1, Value::Number(n as f64));
        }
        Instr::Nth => {
            let [v, i] = top(ops)?;
            let found = coll::nth_at(heap, v, i)?;
            replace(ops, 2, found);
        }
        Instr::Get => {
            let [c, k] = top(ops)?;
            let found = coll::lookup(heap, c, k)?;
            replace(ops, 2, found);
        }
        Instr::GetOr => {
            let [c, k, d] = top(ops)?;
            let found = coll::get(heap, c, k)?.unwrap_or(d);
            replace(ops, 3, found);
        }
        Instr::Assoc => {
            let [c, k, v] = top(ops)?;
            let updated = coll::assoc(heap, c, k, v)?;
            replace(ops, 3, updated);
        }
        Instr::Dissoc => {
            let [m, k] = top(ops)?;
            let updated = coll::dissoc(heap, m, k)?;
            replace(ops, 2, updated);
        }
        Instr::Conj => {
            let [v, x] = top(ops)?;
            let updated = coll::conj(heap, v, x)?;
            replace(ops, 2, updated);
        }
        Instr::PopLast => {
            let [v] = top(ops)?;
            let updated = coll::pop(heap, v)?;
            replace(ops, 1, updated);
        }
        Instr::NewVectorBuilder | Instr::NewMapBuilder => {
            let kind = match instr {
                Instr::NewVectorBuilder => BuilderKind::Vector,
                _ => BuilderKind::Map,
            };
            let b = builder::new_builder(heap, caller, kind, &mut state.builders)?;
            state.frame.operands.push(b);
        }
        Instr::BuilderConj => {
            let [b, x] = top(ops)?;
            builder::conj(heap, caller, b, x)?;
            replace(ops, 2, b);
        }
        Instr::BuilderAssoc => {
            let [b, k, v] = top(ops)?;
            builder::assoc(heap, caller, b, k, v)?;
            replace(ops, 3, b);
        }
        Instr::Freeze => {
            let [b] = top(ops)?;
            let frozen = builder::freeze(heap, caller, b)?;
            replace(ops, 1, frozen);
        }
        _ => return Err(Fault::Internal("not a collection intrinsic")),
    }
    Ok(())
}

/// Early VM tests (hand-assembled bytecode). Cross-task use while the owner
/// is alive needs the owner's builder handle, which no public API can take
/// out of a suspended task yet, so it is tested here.
#[cfg(test)]
mod tests {
    use crate::heap::Rooted;
    use crate::host::NoHost;
    use crate::vm::{Assembler, Opcode};
    use crate::{Budget, BuilderError, Primitive, RunOutcome, Runtime, ValueView, VmErrorKind};

    #[test]
    fn another_task_cannot_mutate_or_freeze_a_live_owners_builder() {
        use Opcode::*;
        let num = Primitive::Number;
        // let b = vector_builder(); conj!(b, 1); /* yield */ conj!(b, 2); return persistent!(b)
        let mut asm = Assembler::new();
        asm.op(NewVectorBuilder).set_local(0);
        asm.get_local(0).constant(num(1.0)).op(BuilderConj).op(Pop);
        asm.get_local(0).constant(num(2.0)).op(BuilderConj);
        asm.op(Freeze).op(Return);
        let rt = Runtime::new();
        let owner = rt.spawn(&asm.finish("owner", 0, 1).unwrap(), &[]).unwrap();
        let first = owner.run_with(Budget::instructions(6), &mut NoHost);
        assert!(matches!(first, Ok(RunOutcome::Yielded)), "{first:?}");
        let (_, _, locals) = owner.saved_state().unwrap();
        let handle = Rooted::new(rt.heap(), locals[0]);

        for op in [BuilderConj, BuilderAssoc, Freeze] {
            let mut asm = Assembler::new();
            asm.get_local(0);
            match op {
                BuilderConj => asm.constant(num(9.0)),
                BuilderAssoc => asm.constant(num(0.0)).constant(num(9.0)),
                _ => &mut asm,
            };
            asm.op(op).op(Return);
            let intruder = rt
                .spawn(&asm.finish("intruder", 1, 1).unwrap(), &[&handle])
                .unwrap();
            let err = match intruder.run().unwrap() {
                RunOutcome::Failed(e) => e,
                other => panic!("expected Failed, got {other:?}"),
            };
            let expected = BuilderError::NotOwner {
                owner: owner.id(),
                caller: intruder.id(),
            };
            assert_eq!(err.kind(), VmErrorKind::Builder(expected));
            assert_eq!(err.diagnostic().opcode, Some(op));
        }

        // The owner continues; the rejected attempts changed nothing.
        let RunOutcome::Complete(v) = owner.run().unwrap() else {
            panic!("owner did not complete")
        };
        let items = rt.scope(|s| {
            let items = s.items(s.local(&v).unwrap()).unwrap();
            items.into_iter().map(|i| s.view(i)).collect::<Vec<_>>()
        });
        assert_eq!(items, [ValueView::Number(1.0), ValueView::Number(2.0)]);
    }

    #[test]
    fn an_owner_that_fails_releases_its_builders() {
        use Opcode::*;
        // let b = vector_builder(); conj!(b, {}); /* yield */ count(1) -> fails
        let mut asm = Assembler::new();
        asm.op(NewVectorBuilder).set_local(0);
        asm.get_local(0).op(NewObject).op(BuilderConj).op(Pop);
        asm.constant(Primitive::Number(1.0)).op(Count).op(Return);
        let rt = Runtime::new();
        let owner = rt.spawn(&asm.finish("owner", 0, 1).unwrap(), &[]).unwrap();
        let first = owner.run_with(Budget::instructions(6), &mut NoHost);
        assert!(matches!(first, Ok(RunOutcome::Yielded)), "{first:?}");
        let (_, _, locals) = owner.saved_state().unwrap();
        let handle = Rooted::new(rt.heap(), locals[0]);
        let RunOutcome::Failed(_) = owner.run().unwrap() else {
            panic!("owner did not fail")
        };

        // The released builder dropped its draft: only the builder survives.
        assert_eq!(rt.collect().unwrap().reclaimed, 1);
        let mut asm = Assembler::new();
        asm.get_local(0).op(Freeze).op(Return);
        let other = rt
            .spawn(&asm.finish("other", 1, 1).unwrap(), &[&handle])
            .unwrap();
        let RunOutcome::Failed(err) = other.run().unwrap() else {
            panic!("expected Failed")
        };
        let expected = BuilderError::OwnerFinished { owner: owner.id() };
        assert_eq!(err.kind(), VmErrorKind::Builder(expected));
    }
}
