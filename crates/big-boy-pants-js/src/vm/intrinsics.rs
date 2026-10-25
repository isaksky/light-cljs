//! Collection and atom intrinsics, and task-owned transient builders
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
//! # Atom intrinsics
//!
//! Opcodes `0x33`–`0x35` operate on atoms ([`crate::atom`], which documents
//! the comparison, ordering and lifetime contract):
//!
//! | Mnemonic | Stack | Effect | Errors ([`AtomErrorKind`](crate::AtomErrorKind)) |
//! |----------|-------|--------|--------|
//! | `atom_new` | v → atom | a new shared (multi-writer) atom holding `v` | `NotShareable` |
//! | `atom_load` | atom → v | the current value, now rooted by the frame | `NotAnAtom` |
//! | `atom_cas` | atom e n → w ok | if the current value is SameValue to `e` (objects: identity), install `n`: `w` is `e` and `ok` is `true`; otherwise nothing changes, `w` is the current value and `ok` is `false` | `NotAnAtom`, `SingleWriter`, `NotShareable` (`n`) |
//!
//! A failed `atom_cas` is reported, not retried: the instruction never
//! re-runs code that computed `n`, so effects before it happen once. The
//! task fails with [`VmErrorKind::Atom`](crate::VmErrorKind::Atom) on an error.
//! Single-writer atoms are readable here; only their Rust
//! [`Publisher`](crate::Publisher) can change them.
//!
//! # Form intrinsics
//!
//! Opcodes `0xBF`–`0xC2` operate on native Clojure form values
//! ([`crate::collections::form`], which documents the value, equality,
//! hashing, metadata and publication contract and its supported boundary).
//! They are a separate kind from the SameValueZero collections above: a
//! form operation rejects a persistent vector or map, and the intrinsics
//! above reject forms. Keys are structural: an equal key finds the entry.
//! Occurrences are transparent only to `formEquals`, `formHash` and map
//! keys; the other operations take the occurrence itself (`formValue`
//! unwraps it).
//!
//! | `__rt` call | Instruction | Result | Errors ([`CollectionError`]) |
//! |-------------|-------------|--------|--------|
//! | `formList(x1, …, xn)`, `formVector(…)` | `new_form_list n`, `new_form_vector n` | new list or vector of the values, in order | – |
//! | `formMap(k1, v1, …)` | `new_form_map n` | new map; a later equal key replaces the value | – |
//! | `formKeyword(ns, name)`, `formSymbol(ns, name)` | `form_op` | new keyword or symbol; `ns` a string or `null`/`undefined`, `name` a string (exact UTF-16) | `WrongType` |
//! | `formKind(v)` | `form_op` | `"keyword"`, `"symbol"`, `"list"`, `"vector"`, `"map"`, `"occurrence"`, or `null` | – |
//! | `formNamespace(x)`, `formName(x)` | `form_op` | a keyword's or symbol's namespace (or `null`) and name | `WrongType` |
//! | `formCount(c)` | `form_op` | element or entry count | `WrongType` |
//! | `formNth(c, i)` | `form_op` | list or vector element at index `i` | `WrongType`, `InvalidIndex`, `IndexOutOfBounds` |
//! | `formGet(c, k, d)` | `form_op` | map value for an equal key, or vector element at index `k`; `d` when absent | `WrongType` (not a vector or map) |
//! | `formAssoc(c, k, v)` | `form_op` | new map with `k` set, or vector with index `k <= count` set | `WrongType`, `InvalidIndex`, `IndexOutOfBounds` |
//! | `formDissoc(m, k)` | `form_op` | new map without the entry for an equal key (absent: `m`) | `WrongType` |
//! | `formConj(c, x)` | `form_op` | vector: `x` appended; list: `x` prepended | `WrongType` |
//! | `formKeys(m)` | `form_op` | a new form vector of the map's keys (the stored key objects), in trie order | `WrongType` |
//! | `formEquals(a, b)`, `formHash(v)` | `form_op` | value equality (boolean) and the light-cljs runtime's 32-bit hash (number) | – |
//! | `formMeta(v)` | `form_op` | the form's metadata map, or `null` | – |
//! | `formWithMeta(v, m)` | `form_op` | a symbol, list, vector or map with metadata `m` (a form map or `null`); `v` itself for the same `m` | `WrongType` |
//! | `formValue(v)` | `form_op` | an occurrence's value (through nested occurrences), else `v` | – |
//!
//! Updates keep the input's metadata and follow the identity rules of
//! [`crate::collections::form`]. A `formEquals`, `formHash` or structural
//! lookup runs to completion inside its instruction, like the collection
//! intrinsics, reading only frozen form bodies; it never calls JavaScript.
//!
//! # Rooting
//!
//! Every intrinsic reads its operands in place and pops them only after its
//! result exists, so operands stay rooted by the task's frame while the
//! intrinsic allocates. Each call happens inside a run, a registered heap
//! mutator, so no collection can start mid-instruction.

use super::dispatch::Fault;
use super::frames::VmState;
use super::instr::{FormOp, Instr};
use crate::atom::{self, AtomError};
use crate::collections::builder::{self, BuildFault, BuilderKind};
use crate::collections::form::{self, FormKind};
use crate::collections::{self as coll, CollectionError, PersistentKind};
use crate::heap::Heap;
use crate::value::{JsString, Value};

impl From<CollectionError> for Fault {
    fn from(e: CollectionError) -> Self {
        match e {
            CollectionError::OutOfMemory => Fault::OutOfMemory("heap object table exhausted"),
            other => Fault::Collection(other),
        }
    }
}

impl From<AtomError> for Fault {
    fn from(e: AtomError) -> Self {
        match e {
            AtomError::OutOfMemory => Fault::OutOfMemory("heap object table exhausted"),
            other => Fault::Atom(other.kind(), other.to_string()),
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
    Ok(std::array::from_fn(|i| ops[start + i].clone()))
}

/// Execute one collection intrinsic or builder instruction.
pub(crate) fn step(state: &mut VmState, heap: &Heap, instr: Instr) -> Result<(), Fault> {
    let caller = state.diag.task;
    match instr {
        Instr::NewVector(n) => {
            let n = usize::from(n);
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let vec = coll::new_vector(heap, &ops[start..])?;
            state.replace(n, vec)?;
        }
        Instr::NewMap(n) => {
            let n = 2 * usize::from(n);
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let entries: Vec<(Value, Value)> = ops[start..]
                .chunks_exact(2)
                .map(|kv| (kv[0].clone(), kv[1].clone()))
                .collect();
            let map = coll::new_map(heap, &entries)?;
            state.replace(n, map)?;
        }
        Instr::Count => {
            let [c] = top(state.operands())?;
            let n = coll::count(heap, c)?;
            state.replace(1, Value::Number(n as f64))?;
        }
        Instr::Nth => {
            let [v, i] = top(state.operands())?;
            let found = coll::nth_at(heap, v, i)?;
            state.replace(2, found)?;
        }
        Instr::Get => {
            let [c, k] = top(state.operands())?;
            let found = coll::lookup(heap, c, k)?;
            state.replace(2, found)?;
        }
        Instr::GetOr => {
            let [c, k, d] = top(state.operands())?;
            let found = coll::get(heap, c, k)?.unwrap_or(d);
            state.replace(3, found)?;
        }
        Instr::Assoc => {
            let [c, k, v] = top(state.operands())?;
            let updated = coll::assoc(heap, c, k, v)?;
            state.replace(3, updated)?;
        }
        Instr::Dissoc => {
            let [m, k] = top(state.operands())?;
            let updated = coll::dissoc(heap, m, k)?;
            state.replace(2, updated)?;
        }
        Instr::Conj => {
            let [v, x] = top(state.operands())?;
            let updated = coll::conj(heap, v, x)?;
            state.replace(2, updated)?;
        }
        Instr::PopLast => {
            let [v] = top(state.operands())?;
            let updated = coll::pop(heap, v)?;
            state.replace(1, updated)?;
        }
        Instr::NewVectorBuilder | Instr::NewMapBuilder => {
            let kind = match instr {
                Instr::NewVectorBuilder => BuilderKind::Vector,
                _ => BuilderKind::Map,
            };
            let b = builder::new_builder(heap, caller, kind, &mut state.builders)?;
            state.push(b);
        }
        Instr::BuilderConj => {
            let [b, x] = top(state.operands())?;
            builder::conj(heap, caller, b.clone(), x)?;
            state.replace(2, b)?;
        }
        Instr::BuilderAssoc => {
            let [b, k, v] = top(state.operands())?;
            builder::assoc(heap, caller, b.clone(), k, v)?;
            state.replace(3, b)?;
        }
        Instr::Freeze => {
            let [b] = top(state.operands())?;
            let frozen = builder::freeze(heap, caller, b)?;
            state.replace(1, frozen)?;
        }
        // Operands stay on the stack (rooted) until the result replaces them;
        // this run is a registered mutator, so the result stays live until
        // pushed.
        Instr::AtomNew => {
            let [v] = top(state.operands())?;
            let a = atom::new_atom(heap, v, false, |a| a)?;
            state.replace(1, a)?;
        }
        Instr::AtomLoad => {
            let [a] = top(state.operands())?;
            let v = atom::load(heap, a, |v| v)?;
            state.replace(1, v)?;
        }
        Instr::AtomCas => {
            let [a, expected, new] = top(state.operands())?;
            let (witness, ok) = match atom::compare_and_set(heap, a, expected.clone(), new, |v| v)?
            {
                Ok(()) => (expected, true),
                Err(current) => (current, false),
            };
            state.replace(3, witness)?;
            state.push(Value::Bool(ok));
        }
        Instr::NewFormList(n) | Instr::NewFormVector(n) => {
            let n = usize::from(n);
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let made = match instr {
                Instr::NewFormList(_) => form::new_list(heap, &ops[start..])?,
                _ => form::new_vector(heap, &ops[start..])?,
            };
            state.replace(n, made)?;
        }
        Instr::NewFormMap(n) => {
            let n = 2 * usize::from(n);
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let entries: Vec<(Value, Value)> = ops[start..]
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[k, v]| (k.clone(), v.clone()))
                .collect();
            let map = form::new_map(heap, &entries)?;
            state.replace(n, map)?;
        }
        Instr::FormOp(op) => {
            let n = op.arity();
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(n)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let args = ops[start..].to_vec();
            let result = form_op(heap, op, args)?;
            state.replace(n, result)?;
        }
        _ => return Err(Fault::Internal("not a collection intrinsic")),
    }
    Ok(())
}

/// One `FormOp` on its operands (oldest first, exactly [`FormOp::arity`]).
fn form_op(heap: &Heap, op: FormOp, args: Vec<Value>) -> Result<Value, CollectionError> {
    let mut args = args.into_iter();
    let mut arg = || args.next().unwrap_or(Value::Undefined);
    let string = |s: &str| Value::String(JsString::from(s));
    Ok(match op {
        FormOp::Keyword => form::new_named_from_values(heap, FormKind::Keyword, arg(), arg())?,
        FormOp::Symbol => form::new_named_from_values(heap, FormKind::Symbol, arg(), arg())?,
        FormOp::Kind => {
            let v = arg();
            match form::kind(heap, &v) {
                Some(kind) => string(kind.name()),
                None if coll::kind_of(heap, &v) == Some(PersistentKind::Occurrence) => {
                    string("occurrence")
                }
                None => Value::Null,
            }
        }
        FormOp::Namespace => form::name(heap, &arg())?
            .namespace
            .map_or(Value::Null, Value::String),
        FormOp::Name => Value::String(form::name(heap, &arg())?.name),
        FormOp::Count => Value::Number(form::count(heap, &arg())? as f64),
        FormOp::Nth => {
            let (c, i) = (arg(), arg());
            form::nth_at(heap, &c, &i)?
        }
        FormOp::Get => {
            let (c, k, d) = (arg(), arg(), arg());
            form::get(heap, &c, &k)?.unwrap_or(d)
        }
        FormOp::Assoc => {
            let (c, k, v) = (arg(), arg(), arg());
            form::assoc(heap, &c, k, v)?
        }
        FormOp::Dissoc => {
            let (m, k) = (arg(), arg());
            form::dissoc(heap, &m, &k)?
        }
        FormOp::Conj => {
            let (c, v) = (arg(), arg());
            form::conj(heap, &c, v)?
        }
        FormOp::Equals => {
            let (a, b) = (arg(), arg());
            Value::Bool(form::equals(heap, &a, &b))
        }
        FormOp::Hash => Value::Number(f64::from(form::hash(heap, &arg()))),
        FormOp::Meta => form::meta(heap, &arg()).unwrap_or(Value::Null),
        FormOp::WithMeta => {
            let (v, m) = (arg(), arg());
            form::with_meta(heap, &v, &m)?
        }
        FormOp::Value => form::unwrap(heap, arg()),
        FormOp::Keys => {
            let keys: Vec<Value> = form::entries(heap, &arg())?
                .into_iter()
                .map(|(k, _)| k)
                .collect();
            form::new_vector(heap, &keys)?
        }
    })
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
        let handle = Rooted::new(rt.heap(), locals[0].clone());

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
        let handle = Rooted::new(rt.heap(), locals[0].clone());
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
