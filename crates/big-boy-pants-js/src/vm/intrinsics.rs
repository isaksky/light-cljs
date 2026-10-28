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
//! `__rt.atomAux(atom)` (`form_op`) returns a stable shared bookkeeping atom,
//! initially holding null, separate from the parent's current value. Its
//! lifetime follows the parent through a traced edge; no global registry or
//! host root retains it. Concurrent calls attach exactly one auxiliary atom.
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
//! | `formKeyword(ns, name)`, `formSymbol(ns, name)` | `form_op` | new keyword or symbol; `ns` a string or `null`/`undefined`, `name` a string (exact UTF-16), whose spelling splits back into `ns` and `name` at its first `/` | `WrongType`, `AmbiguousName` |
//! | `formKind(v)` | `form_op` | `"keyword"`, `"symbol"`, `"list"`, `"vector"`, `"map"`, `"set"`, `"occurrence"`, or `null` | – |
//! | `formSequence(v)` | `form_op` | immutable non-IList sequence sharing a positional trie or cons body; no realization | `WrongType` |
//! | `formNamespace(x)`, `formName(x)` | `form_op` | a keyword's or symbol's namespace (or `null`) and name | `WrongType` |
//! | `formCount(c)` | `form_op` | element or entry count | `WrongType` |
//! | `formNth(c, i)` | `form_op` | list or vector element at index `i` | `WrongType`, `InvalidIndex`, `IndexOutOfBounds` |
//! | `formGet(c, k, d)` | `form_op` | map value for an equal key, or vector element at index `k`; `d` when absent | `WrongType` (not a vector or map) |
//! | `formAssoc(c, k, v)` | `form_op` | new map with `k` set, or vector with index `k <= count` set | `WrongType`, `InvalidIndex`, `IndexOutOfBounds` |
//! | `formDissoc(m, k)` | `form_op` | new map without the entry for an equal key (absent: `m`) | `WrongType` |
//! | `formConj(c, x)` | `form_op` | vector: `x` appended; list: `x` prepended | `WrongType` |
//! | `formKeys(m)` | `form_op` | a new form vector of the map's keys (the stored key objects), in trie order | `WrongType` |
//! | `formPrintItems(m)` | `form_op` | map/record entries or set members in legacy printer order, retaining key identity | wrong type |
//! | `formReaderCollection(m)` | `form_op` | map/set with legacy seq traversal retained through updates, sharing storage | wrong type |
//! | `formSortedMap(pairs)` | `form_op` | default-comparator sorted map from alternating operands; odd trailing key has nil value | wrong type; incomparable keys |
//! | `formEquals(a, b)`, `formHash(v)` | `form_op` | value equality (boolean) and the light-cljs runtime's 32-bit hash (number) | – |
//! | `formMeta(v)` | `form_op` | the form's metadata map, or `null` | – |
//! | `formWithMeta(v, m)` | `form_op` | a symbol, list, vector or map with metadata `m` (a form map or `null`); `v` itself for the same `m` | `WrongType` |
//! | `formValue(v)` | `form_op` | an occurrence's value (through nested occurrences), else `v` | – |
//!
//! | `formSet(x1, …, xn)` | `new_form_set n` | new set of the values; of equal members the first is kept (light-cljs-477) | – |
//! | `formLiteralMap(pairs)` | `form_op LiteralMap` | positional map from alternating operands in a form list/vector; retains equal keys, first-match lookup | wrong type; odd operand count |
//! | `formCompilerLiteral(tag, payload)` | `form_op CompilerLiteral` | explicit immutable compiler descriptor; payload and metadata traced and checked for publication | invalid tag or payload |
//! | `formLiteralPayload(value)` | `form_op LiteralPayload` | descriptor payload, preserving its identity | wrong type |
//! | `formLiteralSet(items)` | `form_op LiteralSet` | positional set from a form list/vector; retains equal members and source order | wrong type |
//! | `formArrayMap(pairs)` | `form_op ArrayMap` | insertion-ordered map from alternating operands; equal keys keep the first key and last value | wrong type; odd operand count |
//! | `formDisj(s, x)` | `form_op` | new set without the member equal to `x` (absent: `s`) | `WrongType` |
//!
//! | `structType(name, fields, record)` | `form_op` | a new struct type (light-cljs-478; [`crate::collections::form`], "Structs") | `WrongType` |
//! | `structNew(type, v1, …)` | `new_struct n` | a struct of `type` with these field values (missing: `undefined`) | `WrongType` |
//! | `structField(s, name)` | `form_op` | the field named `name` | `WrongType`, `MissingKey` |
//! | `structTypeOf(v)` | `form_op` | a struct's type, or `null` | – |
//! | `recordFromMap(type, m)` | `form_op` | `map->R`: fields from `m`'s field keywords, the rest as extension entries | `WrongType` |
//! | `cljDispatch(table, v)` | `form_op` | protocol dispatch: `v`'s implementation in `table`, its default, `null`, or `undefined` for a JavaScript object | `WrongType` |
//! | `cljProtocolLookup(snapshot, version, frontier, v)` | `form_op` | guarded protocol implementation with struct/object/default precedence; `undefined` for stale/missing snapshots, undeclared structs or host objects, for task-local slow resolution | `WrongType` |
//!
//! Sets are the fifth collection kind: `formCount`, `formGet` (the stored
//! member), `formConj` (absent members only), `formWithMeta`,
//! `formEquals` and `formHash` (`hashUnorderedColl` of the members) accept
//! them; `formKind` names them `"set"`.
//!
//! # Clojure core intrinsics
//!
//! `__rt.cljTruth(v)`, `cljCount(c)`, `cljSeq(c)`, `cljFirst(c)`,
//! `cljRest(c)`, `cljNext(c)`, `cljCons(x, c)`, `cljNth(c, i)`,
//! `cljNthOr(c, i, d)`, `cljGet(c, k, d)`, `cljContains(c, k)`,
//! `cljAssoc(c, k, v)`, `cljDissoc(m, k)`, `cljConj(c, x)`,
//! `cljDisj(s, x)`, `cljConcat(a, b)`, `cljMeta(v)` and `cljWithMeta(v, m)` are further
//! `form_op` selectors with the semantics of the ClojureScript functions of
//! those names on forms, strings and `nil`, which light-cljs lowers macro
//! code to (light-cljs-477): occurrences are transparent, `nil` puns, and
//! sequences are form lists. So are the flat lowerings
//! `cljConcatAll(colls)` (`concat` of the collections in the list `colls`)
//! and `cljZipMap(keys, values)` (a map of two equal-length lists, for a
//! hash-map literal that evaluates its keys first), and the constructors
//! `cljKeyword(x)`, `cljKeywordNs(ns, name)`, `cljSymbol(x)` and
//! `cljSymbolNs(ns, name)` (`keyword` and `symbol` of one or two
//! arguments). [`crate::collections::clj`] documents them.
//! Unlike the operations above, an unsupported operand kind raises a
//! catchable `TypeError`, `cljNth` past the end a catchable `RangeError` and
//! an invalid constructor operand a catchable plain `Error`
//! ([`crate::VmErrorKind::Error`]), the diagnostics of the JavaScript
//! functions they replace.
//!
//! Lazy and chunked sequences (light-cljs-479,
//! [`crate::collections::lazy`]) are `__rt.lazySeq(thunk)`, `cljRealized(s)`,
//! `cljList(c)` (every element of the seq, realized, as a list),
//! `cljChunkedSeq(s)`, `cljChunkFirst(s)`, `cljChunkRest(s)`,
//! `cljChunkNext(s)`, `cljChunkCons(chunk, rest)`, `cljRangeCons(chunk,
//! rest, endOrNil, step)` (a range's chunked sequence, with retained arithmetic for
//! finite integer ranges) and `cljRealizeAll(v)` (`v`
//! with every lazy sequence inside it replaced by a list). A form
//! instruction that needs an unrealized lazy sequence (a clj operation, or
//! equality or hashing inside any form instruction) does not complete: it
//! moves its operands into a `Realize` native frame, which realizes the
//! sequence (or parks the task while another task does) and then executes the
//! instruction again with the same operands.
//!
//! `cljReduced(v)`, `cljIsReduced(v)` and `cljUnreduced(v)` (light-cljs-507)
//! construct/test/unwrap an immutable native early-termination marker. Its
//! payload is traced and publication-checked, and its allocation uses the
//! ordinary persistent-form byte budget. Reduction callbacks and mutable
//! transducer state run on the task VM; these intrinsics run no user code.
//! `cljIsEditable(v)` tests the builtin editable collection kinds without
//! allocating or realizing a sequence; map entries and records are excluded.
//!
//! Updates keep the input's metadata and follow the identity rules of
//! [`crate::collections::form`]. A `formEquals`, `formHash` or structural
//! lookup runs to completion inside its instruction, like the collection
//! intrinsics, reading only frozen form bodies; it never calls JavaScript.
//!
//! # Private initializer slots
//!
//! `initializerSlot(unit, name)` creates private task-owned compiler storage;
//! `initializerSlotGet(slot, field)` reads unit/name/state/value/defined at
//! fields 0..4. `initializerSlotSet(slot, field, value)` writes state (0..3),
//! value (any JavaScript value), or defined (boolean) at fields 2/3/4 and
//! returns the value. Invalid slots/fields are TypeErrors; a foreign task's
//! access is ForeignObject. Slots are never publishable, trace all three
//! value edges and charge their fixed boxed body to the aggregate heap budget.
//! Their public compiler roots and getter properties remain ordinary objects.
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
use super::native::realize;
use crate::atom::{self, AtomError};
use crate::collections::builder::{self, BuildFault, BuilderKind};
use crate::collections::clj;
use crate::collections::form::{self, FormKind};
use crate::collections::lazy;
use crate::collections::{self as coll, CollectionError, PersistentKind};
use crate::heap::Heap;
use crate::value::Value;

impl From<CollectionError> for Fault {
    fn from(e: CollectionError) -> Self {
        match e {
            CollectionError::OutOfMemory => Fault::OutOfMemory("heap object table exhausted"),
            CollectionError::CannotCompare(message) => Fault::Error(message),
            CollectionError::CannotCompareValues(values) => Fault::Compare(values),
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
pub(crate) fn step(
    state: &mut VmState,
    heap: &Heap,
    host: &mut dyn crate::Host,
    instr: Instr,
) -> Result<(), Fault> {
    let caller = state.diag.task;
    match instr {
        Instr::FormOp(
            op
            @ (FormOp::InitializerSlot | FormOp::InitializerSlotGet | FormOp::InitializerSlotSet),
        ) => {
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(op.arity())
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let value = super::initializer::operation(heap, caller, op, &ops[start..])?;
            state.replace(op.arity(), value)?;
        }
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
        Instr::NewFormList(_)
        | Instr::NewFormVector(_)
        | Instr::NewFormSet(_)
        | Instr::NewStruct(_)
        | Instr::NewFormMap(_)
        | Instr::FormOp(_) => return form_step(state, heap, host, instr),
        _ => return Err(Fault::Internal("not a collection intrinsic")),
    }
    Ok(())
}

/// Execute one form instruction. Form equality and hashing may meet an
/// unrealized lazy sequence, and the clj operations read sequences through:
/// then the instruction realizes it and executes again instead of
/// completing (light-cljs-479; [`crate::collections::lazy`]), whatever
/// result or failure it computed past the request.
fn form_step(
    state: &mut VmState,
    heap: &Heap,
    host: &mut dyn crate::Host,
    instr: Instr,
) -> Result<(), Fault> {
    if instr == Instr::FormOp(FormOp::CljCoreResolver) {
        let [resolver] = top(state.operands())?;
        super::dispatch::callee(heap, &resolver)?;
        state.core_resolver = Some(resolver);
        return state.replace(1, Value::Null);
    }
    let n = match instr {
        Instr::NewFormList(n) | Instr::NewFormVector(n) | Instr::NewFormSet(n) => usize::from(n),
        Instr::NewStruct(n) => usize::from(n),
        Instr::NewFormMap(n) => 2 * usize::from(n),
        Instr::FormOp(op) => op.arity(),
        _ => return Err(Fault::Internal("not a form intrinsic")),
    };
    let ops = state.operands();
    let start = ops
        .len()
        .checked_sub(n)
        .ok_or(Fault::Internal("operand stack underflow"))?;
    let ops = &ops[start..];
    // A request left by an embedder's comparison is not this instruction's.
    let _ = lazy::take_request();
    let _ = lazy::take_cycle();
    let made = form_value(heap, host, instr, ops);
    if lazy::take_cycle() {
        let _ = lazy::take_request();
        return Err(Fault::RangeError(
            "infinite lazy sequence: it contains itself".to_string(),
        ));
    }
    if let Some(pending) = lazy::take_request() {
        return realize::start(heap, state, instr, n, pending);
    }
    state.replace(n, made?)
}

/// The value of one form instruction on its operands `ops`.
fn form_value(
    heap: &Heap,
    host: &mut dyn crate::Host,
    instr: Instr,
    ops: &[Value],
) -> Result<Value, Fault> {
    Ok(match instr {
        Instr::NewFormList(_) => form::new_list(heap, ops)?,
        Instr::NewFormSet(_) => form::new_set(heap, ops)?,
        Instr::NewFormVector(_) => form::new_vector(heap, ops)?,
        Instr::NewStruct(_) => {
            let (ty, fields) = match ops.split_first() {
                Some((ty, fields)) => (ty.clone(), fields),
                None => (Value::Undefined, &[][..]),
            };
            form::new_struct(heap, &ty, fields)?
        }
        Instr::NewFormMap(_) => {
            let entries: Vec<(Value, Value)> = ops
                .as_chunks::<2>()
                .0
                .iter()
                .map(|[k, v]| (k.clone(), v.clone()))
                .collect();
            form::new_map(heap, &entries)?
        }
        Instr::FormOp(op) => form_op(heap, host, op, ops.to_vec())?,
        _ => return Err(Fault::Internal("not a form intrinsic")),
    })
}

impl From<clj::CljError> for Fault {
    fn from(e: clj::CljError) -> Self {
        match e {
            clj::CljError::Collection(e) => e.into(),
            clj::CljError::OutOfBounds => Fault::RangeError(e.message()),
            clj::CljError::Unsupported { .. } => Fault::TypeError(e.message()),
            // `keywordCore`/`symbolCore` throw a plain `Error`.
            clj::CljError::Operands(_) => Fault::Error(e.message()),
            clj::CljError::Arity(_) => Fault::Error(e.message()),
            clj::CljError::Infinite => Fault::RangeError(e.message()),
            // Intercepted by `form_op`, which starts the realization.
            clj::CljError::Realize(_) => Fault::Internal("unhandled lazy realization request"),
        }
    }
}

/// One `FormOp` on its operands (oldest first, exactly [`FormOp::arity`]).
/// An operation that needs a lazy sequence realized records the request
/// ([`crate::collections::lazy::request`]) and returns a placeholder.
fn form_op(
    heap: &Heap,
    host: &mut dyn crate::Host,
    op: FormOp,
    args: Vec<Value>,
) -> Result<Value, Fault> {
    if op == FormOp::AtomAux {
        return atom::auxiliary(heap, args[0].clone(), |v| v).map_err(Fault::from);
    }
    match clj_op(heap, op, &args) {
        Ok(Some(result)) => Ok(result),
        Ok(None) => Ok(strict_form_op(heap, host, op, args)?),
        Err(clj::CljError::Realize(lazy)) => {
            crate::collections::lazy::request(lazy);
            Ok(Value::Undefined)
        }
        Err(e) => Err(e.into()),
    }
}

/// A `__rt.clj*` or lazy sequence operation; `None` for a strict one.
fn clj_op(heap: &Heap, op: FormOp, args: &[Value]) -> Result<Option<Value>, clj::CljError> {
    let a = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    Ok(match op {
        FormOp::CljTruth => Some(Value::Bool(clj::truth(heap, &a(0)))),
        FormOp::CljIsZero => Some(Value::Bool(clj::is_zero(heap, &a(0)))),
        FormOp::CljIsFalse => Some(Value::Bool(clj::is_false(heap, &a(0)))),
        FormOp::CljReduced => Some(form::new_reduced(heap, a(0))?),
        FormOp::CljIsReduced => Some(Value::Bool(clj::is_reduced(heap, &a(0)))),
        FormOp::CljUnreduced => Some(clj::unreduced(heap, &a(0))),
        FormOp::CljIsEditable => Some(Value::Bool(clj::is_editable(heap, &a(0)))),
        FormOp::CljIsList => Some(Value::Bool(clj::is_list(heap, &a(0)))),
        FormOp::CljIsSeq => Some(Value::Bool(clj::is_seq(heap, &a(0)))),
        FormOp::CljIsSequential => Some(Value::Bool(clj::is_sequential(heap, &a(0)))),
        FormOp::CljIsColl => Some(Value::Bool(clj::is_coll(heap, &a(0)))),
        FormOp::CljIsVector => Some(Value::Bool(clj::is_vector(heap, &a(0)))),
        FormOp::CljIsMapEntry => Some(Value::Bool(clj::is_map_entry(heap, &a(0)))),
        FormOp::CljPeek => Some(clj::peek(heap, &a(0))?),
        FormOp::CljInvoke => Some(clj::invoke(
            heap,
            &a(0),
            &a(1),
            &a(2),
            crate::collections::index_from_value(&a(3))?,
        )?),
        FormOp::CljCallable => Some(Value::Bool(matches!(
            form::kind(heap, &form::unwrap(heap, a(0))),
            Some(
                form::FormKind::Vector
                    | form::FormKind::Map
                    | form::FormKind::Set
                    | form::FormKind::Keyword
                    | form::FormKind::Symbol
            )
        ))),
        FormOp::CljCount => Some(clj::count(heap, &a(0))?),
        FormOp::CljSeq => Some(clj::seq(heap, &a(0))?),
        FormOp::CljFirst => Some(clj::first(heap, &a(0))?),
        FormOp::CljRest => Some(clj::rest(heap, &a(0))?),
        FormOp::CljNext => Some(clj::next(heap, &a(0))?),
        FormOp::CljCons => Some(clj::cons(heap, &a(0), &a(1))?),
        FormOp::CljNth => Some(clj::nth(heap, &a(0), &a(1), None)?),
        FormOp::CljNthOr => Some(clj::nth(heap, &a(0), &a(1), Some(&a(2)))?),
        FormOp::CljGet => Some(clj::get(heap, &a(0), &a(1), &a(2))?),
        FormOp::CljGet2 => Some(clj::get(heap, &a(0), &a(1), &Value::Null)?),
        FormOp::CljContains => Some(Value::Bool(clj::contains(heap, &a(0), &a(1))?)),
        FormOp::CljAssoc => Some(clj::assoc(heap, &a(0), &a(1), &a(2))?),
        FormOp::CljDissoc => Some(clj::dissoc(heap, &a(0), &a(1))?),
        FormOp::CljConj => Some(clj::conj(heap, &a(0), &a(1))?),
        FormOp::CljDisj => Some(clj::disj(heap, &a(0), &a(1))?),
        FormOp::CljMeta => Some(clj::meta(heap, &a(0))),
        FormOp::CljWithMeta => Some(clj::with_meta(heap, &a(0), &a(1))?),
        FormOp::CljWithExportedMeta => {
            let (value, meta) = (a(0), form::unwrap(heap, a(1)));
            let updated = clj::with_meta(heap, &value, &meta)?;
            Some(if matches!(meta, Value::Null | Value::Undefined) {
                form::with_exported_meta(heap, &form::unwrap(heap, updated), Value::Null)?
            } else {
                updated
            })
        }
        FormOp::CljConcat => Some(clj::concat(heap, &a(0), &a(1))?),
        FormOp::CljConcatAll => Some(clj::concat_all(heap, &a(0))?),
        FormOp::CljZipMap => Some(clj::zip_map(heap, &a(0), &a(1))?),
        FormOp::CljKeyword => Some(clj::keyword(heap, &a(0))?),
        FormOp::CljKeywordNs => Some(clj::keyword_ns(heap, &a(0), &a(1))?),
        FormOp::CljSymbol => Some(clj::symbol(heap, &a(0))?),
        FormOp::CljSymbolNs => Some(clj::symbol_ns(heap, &a(0), &a(1))?),
        FormOp::CljDispatch => Some(clj::dispatch(heap, &a(0), &a(1))?),
        FormOp::CljProtocolLookup => Some(clj::protocol_lookup(heap, &a(0), &a(1), &a(2), &a(3))?),
        FormOp::CljIsAtom => Some(Value::Bool(atom::is_atom(heap, &a(0)))),
        FormOp::LazySeq => Some(clj::lazy_seq(heap, &a(0))?),
        FormOp::CljRealized => Some(clj::realized(heap, &a(0))?),
        FormOp::CljList => Some(clj::to_list(heap, &a(0))?),
        FormOp::CljChunkedSeq => Some(Value::Bool(clj::chunked_seq(heap, &a(0)))),
        FormOp::CljChunkFirst => Some(clj::chunk_first(heap, &a(0))?),
        FormOp::CljChunkRest => Some(clj::chunk_rest(heap, &a(0))?),
        FormOp::CljChunkNext => Some(clj::chunk_next(heap, &a(0))?),
        FormOp::CljChunkCons => Some(clj::chunk_cons(heap, &a(0), &a(1), false, None)?),
        FormOp::CljRangeCons => {
            let count = match (form::unwrap(heap, a(2)), form::unwrap(heap, a(3))) {
                (Value::Number(end), Value::Number(step)) if step != 0.0 => {
                    Some(crate::collections::lazy::RangeCount { end, step })
                }
                (Value::Null | Value::Undefined, _) => None,
                _ => {
                    return Err(clj::CljError::Operands(
                        "counted range requires a numeric end and nonzero step",
                    ));
                }
            };
            Some(clj::chunk_cons(heap, &a(0), &a(1), true, count)?)
        }
        FormOp::CljRealizeAll => Some(clj::realize_all(heap, &a(0))?),
        _ => None,
    })
}

/// A strict `__rt.form*` operation ([`crate::collections::form`]).
fn strict_form_op(
    heap: &Heap,
    host: &mut dyn crate::Host,
    op: FormOp,
    args: Vec<Value>,
) -> Result<Value, CollectionError> {
    use crate::value::string::FixedText;
    static RECORD: FixedText = FixedText::new("record");
    static OCCURRENCE: FixedText = FixedText::new("occurrence");
    let mut args = args.into_iter();
    let mut arg = || args.next().unwrap_or(Value::Undefined);
    Ok(match op {
        FormOp::Keyword => form::new_named_from_values(heap, FormKind::Keyword, arg(), arg())?,
        FormOp::Symbol => form::new_named_from_values(heap, FormKind::Symbol, arg(), arg())?,
        FormOp::CompilerLiteral => {
            form::new_compiler_literal_with_zone(heap, &arg(), &arg(), || {
                super::date::zone(heap, host)
            })?
        }
        FormOp::LiteralPayload => form::compiler_literal(heap, &arg())?.1,
        FormOp::LiteralMap => form::literal_from_sequence(heap, &arg(), false)?,
        FormOp::LiteralSet => form::literal_from_sequence(heap, &arg(), true)?,
        FormOp::ArrayMap => form::array_map_from_sequence(heap, &arg())?,
        FormOp::Sequence => form::sequence_view(heap, &arg())?,
        FormOp::SortedMap => form::sorted_map_from_sequence(heap, &arg())?,
        FormOp::Kind => {
            let v = arg();
            match form::kind(heap, &v) {
                // A record is `"record"`, a deftype instance `"struct"`.
                Some(FormKind::Struct) if form::is_record(heap, &v) => Value::String(RECORD.get()),
                Some(kind) => Value::String(kind.name_string()),
                None if coll::kind_of(heap, &v) == Some(PersistentKind::Occurrence) => {
                    Value::String(OCCURRENCE.get())
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
            Value::Bool(form::try_equals(heap, &a, &b)?)
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
        FormOp::PrintItems => form::print_items(heap, &arg())?,
        FormOp::ReaderCollection => form::reader_collection(heap, &arg())?,
        FormOp::Disj => {
            let (s, m) = (arg(), arg());
            form::disj(heap, &s, &m)?
        }
        FormOp::StructType => {
            let (name, fields, record) = (arg(), arg(), arg());
            form::new_type(heap, &name, &fields, &record)?
        }
        FormOp::StructField => {
            let (s, name) = (arg(), arg());
            form::struct_field(heap, &s, &name)?
        }
        FormOp::StructTypeOf => form::type_of(heap, &arg()),
        FormOp::RecordFromMap => {
            let (ty, map) = (arg(), arg());
            form::record_from_map(heap, &ty, &map)?
        }
        // Dispatched by `form_op` before reaching here.
        FormOp::CljTruth
        | FormOp::InitializerSlot
        | FormOp::InitializerSlotGet
        | FormOp::InitializerSlotSet
        | FormOp::AtomAux
        | FormOp::CljCoreResolver
        | FormOp::CljGet2
        | FormOp::CljIsZero
        | FormOp::CljIsFalse
        | FormOp::CljReduced
        | FormOp::CljIsReduced
        | FormOp::CljUnreduced
        | FormOp::CljIsEditable
        | FormOp::CljIsList
        | FormOp::CljIsSeq
        | FormOp::CljIsSequential
        | FormOp::CljIsColl
        | FormOp::CljIsVector
        | FormOp::CljIsMapEntry
        | FormOp::CljPeek
        | FormOp::CljInvoke
        | FormOp::CljCallable
        | FormOp::CljCallMember
        | FormOp::CljReadRoot
        | FormOp::CljCount
        | FormOp::CljSeq
        | FormOp::CljFirst
        | FormOp::CljRest
        | FormOp::CljNext
        | FormOp::CljCons
        | FormOp::CljNth
        | FormOp::CljNthOr
        | FormOp::CljGet
        | FormOp::CljContains
        | FormOp::CljAssoc
        | FormOp::CljDissoc
        | FormOp::CljConj
        | FormOp::CljDisj
        | FormOp::CljMeta
        | FormOp::CljWithMeta
        | FormOp::CljWithExportedMeta
        | FormOp::CljConcat
        | FormOp::CljConcatAll
        | FormOp::CljZipMap
        | FormOp::CljKeyword
        | FormOp::CljKeywordNs
        | FormOp::CljSymbol
        | FormOp::CljDispatch
        | FormOp::CljProtocolLookup
        | FormOp::CljIsAtom
        | FormOp::LazySeq
        | FormOp::CljRealized
        | FormOp::CljList
        | FormOp::CljChunkedSeq
        | FormOp::CljChunkFirst
        | FormOp::CljChunkRest
        | FormOp::CljChunkNext
        | FormOp::CljChunkCons
        | FormOp::CljRealizeAll
        | FormOp::CljRangeCons
        | FormOp::CljSymbolNs => Value::Undefined,
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
    fn static_names_form_kind_results_share_bodies_without_byte_growth() {
        use super::{FormKind, FormOp, strict_form_op};
        use crate::collections::{Provenance, form, new_occurrence};
        use crate::heap::Heap;
        use crate::value::{JsString, Value};

        let heap = Heap::with_byte_limit(64 * 1024);
        let vector = form::new_vector(&heap, &[]).unwrap();
        let ty = form::new_type(
            &heap,
            &Value::String(JsString::from("R")),
            &vector,
            &Value::Bool(true),
        )
        .unwrap();
        let record = form::new_struct(&heap, &ty, &[]).unwrap();
        let occurrence = new_occurrence(&heap, Value::Number(1.0), Provenance::default()).unwrap();
        for (value, expected) in [
            (vector, FormKind::Vector.name()),
            (ty, FormKind::Type.name()),
            (record, "record"),
            (occurrence, "occurrence"),
        ] {
            let Value::String(first) =
                strict_form_op(&heap, &mut NoHost, FormOp::Kind, vec![value.clone()]).unwrap()
            else {
                panic!("expected kind string")
            };
            assert_eq!(first, expected);
            let used = heap.byte_budget().used();
            for _ in 0..100 {
                let Value::String(next) =
                    strict_form_op(&heap, &mut NoHost, FormOp::Kind, vec![value.clone()]).unwrap()
                else {
                    panic!("expected kind string")
                };
                assert!(first.same_body(&next));
            }
            assert_eq!(heap.byte_budget().used(), used);
        }
    }

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
