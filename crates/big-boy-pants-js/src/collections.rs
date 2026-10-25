//! Persistent (shareable, immutable) vectors and maps, and syntax
//! occurrences (`docs/design.md` §§4–5).
//!
//! This is the project's own native collection contract. It does not claim
//! compatibility with Clojure's or any other collection library.
//!
//! # Kinds
//!
//! | Kind | Heap object | Contents |
//! |------|-------------|----------|
//! | [`PersistentKind::Vector`] | immutable | elements indexed `0..count` |
//! | [`PersistentKind::Map`] | immutable | key → value entries, unique keys |
//! | [`PersistentKind::Occurrence`] | immutable | one value plus its [`Provenance`] |
//!
//! Each vector, map or occurrence is one heap object with a stable identity
//! (like every heap object). Elements, keys and values are any values,
//! including other collections and plain objects; nested collections are
//! referenced by identity, never copied.
//!
//! # Operations (embedder API on [`Scope`])
//!
//! | Operation | Vector | Map | Errors |
//! |-----------|--------|-----|--------|
//! | [`Scope::new_vector`] / [`Scope::new_map`] | from locals | from entries (a later duplicate key wins) | – |
//! | [`Scope::count`] | element count | entry count | `WrongType` |
//! | [`Scope::nth`] | element at a `usize` index | – | `IndexOutOfBounds`, `WrongType` |
//! | [`Scope::get`] | element at an index key, `None` if out of bounds | value for a key, `None` if absent | `InvalidIndex` (vector), `WrongType` |
//! | [`Scope::assoc`] | replace at index `< count`, or append at `count` | add or replace a key | `InvalidIndex`, `IndexOutOfBounds`, `WrongType` |
//! | [`Scope::dissoc`] | – | remove a key (absent: no-op) | `WrongType` |
//! | [`Scope::conj`] / [`Scope::pop`] | append / remove the last element | – | `IndexOutOfBounds` (pop of empty), `WrongType` |
//! | [`Scope::items`] / [`Scope::entries`] | elements in order | entries in unspecified order | `WrongType` |
//! | [`Scope::new_occurrence`], [`Scope::occurrence_value`], [`Scope::provenance`] | – | – | `WrongType` |
//! | [`Scope::value_equals`] | structural | structural | never fails |
//!
//! * **Index contract.** A vector index given as a value (a key for
//!   [`Scope::get`] or [`Scope::assoc`]) must be a number with an integral
//!   value in `0 ..= 2^53 - 1` (`-0` counts as `0`); anything else is
//!   [`CollectionError::InvalidIndex`]. No string or other coercion is done.
//! * **Key contract.** Map keys may be any value and compare by
//!   *SameValueZero*: primitives by value (`NaN` equals `NaN`, `-0` equals and
//!   is stored as `+0`), heap objects — including vectors, maps and
//!   occurrences — **by identity**. Structurally equal but distinct
//!   collections are distinct keys. Hashing is a fixed private function of
//!   the key's bits or object identity: no user hashing, equality, getter or
//!   protocol code can run during a lookup or update.
//! * **Updates are persistent.** `assoc`, `dissoc`, `conj` and `pop` return a
//!   new collection object and never change their input. The new version
//!   shares every untouched trie node with the old one (only the
//!   root-to-change path is copied, O(log32 n)), and every element that was
//!   not replaced is the *same* value, so nested collections keep their
//!   identity. An update that changes nothing (`assoc` of an identical value
//!   — numbers compared by bits, everything else by `===` — or `dissoc` of an
//!   absent key) returns the original object itself.
//! * **Errors** never panic and never leave a partial result:
//!   [`CollectionError`] names what was wrong.
//!
//! VM code reaches the same operations through private intrinsics
//! ([`crate::vm::intrinsics`]), which call the `pub(crate)` functions of
//! this module on raw values; the `Scope` methods are thin wrappers over
//! them too. The strict `get` intrinsic adds [`CollectionError::MissingKey`].
//!
//! # Transient builders
//!
//! The one mutable collection kind: a task-owned vector or map builder,
//! mutated in place only by its owning task and frozen into a new persistent
//! value. Lifecycle, ownership (by task, not OS thread), errors
//! ([`BuilderError`]) and tracing are documented in `collections/builder.rs`
//! and summarized in [`crate::vm::intrinsics`]. Builders are not persistent
//! kinds: [`Scope::persistent_kind`] reports `None` for them.
//!
//! # Equality, identity, provenance
//!
//! Three separate notions, never conflated:
//!
//! * **Identity** ([`Scope::same_object`], JavaScript `===`): the same heap
//!   object. A vector is `===` only to itself.
//! * **Value equality** ([`Scope::value_equals`]): structural. Vectors are
//!   equal when they have the same count and pairwise value-equal elements;
//!   maps when they have the same keys (by the key contract) mapping to
//!   value-equal values; primitives by SameValueZero; other heap objects
//!   (plain, native) only by identity. Occurrences are transparent: an
//!   occurrence is value-equal to its value, so provenance never affects
//!   equality. The walk is iterative (no recursion limit), expands each pair
//!   of collections at most once (so heavily shared graphs cost time linear
//!   in their distinct object pairs, not their paths), and terminates
//!   because a persistent value can only contain values that existed before
//!   it, so persistent graphs are acyclic.
//! * **Provenance**: where a value occurred in source. An occurrence is its
//!   own heap object wrapping a value and a [`Provenance`]. Two occurrences
//!   of the equal scalar `1` at different locations are two objects with two
//!   provenances that compare value-equal; provenance is never attached to,
//!   or looked up by, the scalar itself.
//!
//! # Storage, tracing and locking boundary
//!
//! A collection heap object owns an immutable trie whose nodes are
//! independently owned, immutable `Arc` storage outside the object table:
//! nodes are not heap objects, have no identity, and are shared between
//! versions by `Arc`. Their lifetime is the lifetime of the heap objects (or
//! in-flight operations) holding them. The owning heap object's `Trace`
//! walks the trie and reports every heap reference among its elements, keys
//! and values, so the collector sees all edges (a node shared by several
//! versions is walked once per version). Nodes never hold `Rooted` handles.
//!
//! A read resolves the collection's identity under the heap's object-table
//! lock once per operation, copying out an `Arc` to the body; the trie walk
//! then runs with no lock held. No per-node lock exists. Updates allocate one
//! new heap object. Callers are registered mutators (an open scope or a run
//! guard), so no collection can run between an operation's allocation and
//! the result becoming a local.
//!
//! # Not supported (yet)
//!
//! * Embedder access to builders (they are created and used only by VM
//!   code), builders seeded from an existing persistent value, and in-place
//!   editing of map trie nodes (a map builder path-copies internally).
//! * Construction-time shareability restrictions: a collection may hold a
//!   plain object or a builder, and is then rejected only when published
//!   (`crate::value::share`; collections built from shareable parts are
//!   marked known shareable at allocation).
//! * Keywords, structural (value-based) map keys, sorted or insertion-ordered
//!   persistent maps, persistent sets/lists, slicing, iteration protocols and
//!   JavaScript property access on these persistent collections. (Keywords,
//!   symbols, lists and structural map keys exist as the separate native
//!   form values of [`form`], light-cljs-466.) Strings, symbols
//!   and BigInts are supported values/keys. JavaScript's mutable Map/Set
//!   are separate from these native persistent types.

pub(crate) mod builder;
pub mod form;
pub(crate) mod map;
mod vector;

use std::collections::HashSet;
use std::fmt;

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, Local, ObjectId, Scope};
use crate::value::Value;

pub use builder::BuilderError;
pub(crate) use builder::{Builder, OwnedBuilders};
pub use form::{FormKind, FormName};
pub(crate) use map::PMap;
pub(crate) use vector::PVector;

/// Where a value occurred in source text. Host-defined: `source` names a
/// source (file, buffer) in the host's own numbering; `line` and `column`
/// are the host's coordinates (1-based by convention).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct Provenance {
    pub source: u32,
    pub line: u32,
    pub column: u32,
}

/// A syntax occurrence: one value at one source location.
#[derive(Clone, Debug)]
pub(crate) struct Occurrence {
    pub(crate) value: Value,
    pub(crate) provenance: Provenance,
}

impl Trace for Occurrence {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        self.value.trace(visit);
    }
}

/// The persistent kinds of [`crate::collections`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum PersistentKind {
    Vector,
    Map,
    Occurrence,
}

impl PersistentKind {
    fn name(self) -> &'static str {
        match self {
            PersistentKind::Vector => "vector",
            PersistentKind::Map => "map",
            PersistentKind::Occurrence => "occurrence",
        }
    }
}

/// Why a collection operation failed. See the contract in
/// [`crate::collections`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionError {
    /// The operand has the wrong kind (`found` uses the names `vector`,
    /// `map`, `occurrence`, `object`, `number`, ...).
    WrongType {
        expected: &'static str,
        found: &'static str,
    },
    /// A vector index value that is not an integral number in
    /// `0 ..= 2^53 - 1`.
    InvalidIndex,
    /// An index beyond the vector (for `pop`: the vector is empty).
    IndexOutOfBounds { index: usize, count: usize },
    /// The heap's object table is exhausted.
    OutOfMemory,
    /// A strict lookup (the `get` intrinsic) found no entry for the key.
    MissingKey,
}

impl fmt::Display for CollectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CollectionError::WrongType { expected, found } => {
                write!(f, "expected a {expected}, got {found}")
            }
            CollectionError::InvalidIndex => {
                f.write_str("a vector index must be a non-negative integral number")
            }
            CollectionError::IndexOutOfBounds { index, count } => {
                write!(f, "index {index} is out of bounds for count {count}")
            }
            CollectionError::OutOfMemory => f.write_str("heap object table exhausted"),
            CollectionError::MissingKey => f.write_str("key not found in map"),
        }
    }
}

impl std::error::Error for CollectionError {}

/// A collection body copied out of the heap (an `Arc` clone).
enum Body {
    Vector(PVector),
    Map(PMap),
    Occurrence(Occurrence),
    /// Not a persistent kind; the type name for diagnostics.
    Other(&'static str),
}

/// Resolve `v` to its body: persistent bodies are frozen and read without
/// any lock (`crate::heap::storage`).
fn body(heap: &Heap, v: &Value) -> Body {
    let Value::Object(id) = *v else {
        return Body::Other(v.type_name());
    };
    heap.read(id, |object| match object {
        HeapObject::Vector(vec) => Body::Vector(vec.clone()),
        HeapObject::Map(map) => Body::Map(map.clone()),
        HeapObject::Occurrence(occ) => Body::Occurrence(occ.clone()),
        other => Body::Other(object_name(other)),
    })
    // Callers hold a live value; a stale one is reported, not trusted.
    .unwrap_or(Body::Other("reclaimed object"))
}

/// The name of a heap object's kind in `WrongType` errors.
pub(crate) fn object_name(object: &HeapObject) -> &'static str {
    match object {
        HeapObject::Proxy(_) => "Proxy",
        HeapObject::Promise(_) => "Promise",
        HeapObject::Generator(_) => "generator",
        HeapObject::AsyncState(_) => "async activation",
        HeapObject::PromiseReaction(_) => "promise reaction",
        HeapObject::PromiseResolving(_) => "promise resolving functions",
        HeapObject::Plain(_) => "object",
        HeapObject::ModuleNamespace(_) => "module namespace",
        HeapObject::Native(_) => "native object",
        HeapObject::Vector(_) => PersistentKind::Vector.name(),
        HeapObject::Map(_) => PersistentKind::Map.name(),
        HeapObject::Occurrence(_) => PersistentKind::Occurrence.name(),
        HeapObject::Form(f) => f.kind().diagnostic_name(),
        HeapObject::Builder(b) => b.kind().name(),
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => "function",
        HeapObject::Cell(_) => "binding cell",
        HeapObject::Atom(_) => "atom",
        HeapObject::Builtin(_) => "built-in function",
        HeapObject::Array(_) => "array",
        HeapObject::JsMap(_) => "Map",
        HeapObject::JsSet(_) => "Set",
        HeapObject::WeakSet(_) => "WeakSet",
        HeapObject::WeakMap(_) => "WeakMap",
        HeapObject::WeakRef(_) => "WeakRef",
        HeapObject::FinalizationRegistry(_) => "FinalizationRegistry",
        HeapObject::DisposableStack(_) => "DisposableStack",

        HeapObject::ArrayBuffer(_) => "ArrayBuffer",
        HeapObject::TypedArray(_) => "typed array",
    }
}

fn found(b: &Body) -> &'static str {
    match b {
        Body::Vector(_) => PersistentKind::Vector.name(),
        Body::Map(_) => PersistentKind::Map.name(),
        Body::Occurrence(_) => PersistentKind::Occurrence.name(),
        Body::Other(name) => name,
    }
}

fn wrong(expected: &'static str, b: &Body) -> CollectionError {
    CollectionError::WrongType {
        expected,
        found: found(b),
    }
}

fn vector_body(heap: &Heap, v: &Value) -> Result<PVector, CollectionError> {
    match body(heap, v) {
        Body::Vector(vec) => Ok(vec),
        other => Err(wrong("vector", &other)),
    }
}

fn map_body(heap: &Heap, v: &Value) -> Result<PMap, CollectionError> {
    match body(heap, v) {
        Body::Map(map) => Ok(map),
        other => Err(wrong("map", &other)),
    }
}

/// Allocate a persistent collection object. `parts` are the values it was
/// derived from, for the known-shareable bit (`Heap::alloc_persistent`):
/// `None` for "all of its contents".
fn alloc(
    heap: &Heap,
    object: HeapObject,
    parts: Option<&[Value]>,
) -> Result<Value, CollectionError> {
    heap.alloc_persistent(object, parts)
        .map(Value::Object)
        .ok_or(CollectionError::OutOfMemory)
}

/// Largest index value accepted: 2^53 - 1 (`Number.MAX_SAFE_INTEGER`).
const MAX_INDEX: f64 = 9_007_199_254_740_991.0;

/// The vector index named by `key`. See the index contract.
pub(crate) fn index_from_value(key: &Value) -> Result<usize, CollectionError> {
    match *key {
        Value::Number(n) if n.fract() == 0.0 && (0.0..=MAX_INDEX).contains(&n) => {
            // In range and integral; saturates on 32-bit targets (and then
            // fails as out of bounds, since no vector can be that long).
            Ok(n as usize)
        }
        _ => Err(CollectionError::InvalidIndex),
    }
}

/// The persistent kind of `v`, if any.
pub(crate) fn kind_of(heap: &Heap, v: &Value) -> Option<PersistentKind> {
    match body(heap, v) {
        Body::Vector(_) => Some(PersistentKind::Vector),
        Body::Map(_) => Some(PersistentKind::Map),
        Body::Occurrence(_) => Some(PersistentKind::Occurrence),
        Body::Other(_) => None,
    }
}

pub(crate) fn new_vector(heap: &Heap, items: &[Value]) -> Result<Value, CollectionError> {
    alloc(
        heap,
        HeapObject::Vector(PVector::from_values(items)),
        Some(items),
    )
}

pub(crate) fn new_map(heap: &Heap, entries: &[(Value, Value)]) -> Result<Value, CollectionError> {
    let map = entries.iter().fold(PMap::default(), |m, (k, v)| {
        m.assoc(k.clone(), v.clone()).unwrap_or(m)
    });
    alloc(heap, HeapObject::Map(map), None)
}

pub(crate) fn new_occurrence(
    heap: &Heap,
    value: Value,
    provenance: Provenance,
) -> Result<Value, CollectionError> {
    alloc(
        heap,
        HeapObject::Occurrence(Occurrence {
            value: value.clone(),
            provenance,
        }),
        Some(&[value]),
    )
}

pub(crate) fn count(heap: &Heap, coll: Value) -> Result<usize, CollectionError> {
    match body(heap, &coll) {
        Body::Vector(vec) => Ok(vec.len()),
        Body::Map(map) => Ok(map.len()),
        other => Err(wrong("vector or map", &other)),
    }
}

/// The element count of a vector (`WrongType` for anything else).
pub(crate) fn vector_count(heap: &Heap, vec: Value) -> Result<usize, CollectionError> {
    vector_body(heap, &vec).map(|vec| vec.len())
}

pub(crate) fn nth(heap: &Heap, vec: Value, index: usize) -> Result<Value, CollectionError> {
    let vec = vector_body(heap, &vec)?;
    vec.get(index).ok_or(CollectionError::IndexOutOfBounds {
        index,
        count: vec.len(),
    })
}

/// The element of a vector at an index *value* (the `nth` intrinsic): the
/// operand kind is checked before the index.
pub(crate) fn nth_at(heap: &Heap, vec: Value, key: Value) -> Result<Value, CollectionError> {
    let vec = vector_body(heap, &vec)?;
    let index = index_from_value(&key)?;
    vec.get(index).ok_or(CollectionError::IndexOutOfBounds {
        index,
        count: vec.len(),
    })
}

/// Strict lookup (the `get` intrinsic): like [`get`], but an absent entry
/// is an error: `IndexOutOfBounds` for a vector, `MissingKey` for a map.
pub(crate) fn lookup(heap: &Heap, coll: Value, key: Value) -> Result<Value, CollectionError> {
    match body(heap, &coll) {
        Body::Map(map) => map.get(&key).ok_or(CollectionError::MissingKey),
        Body::Vector(_) => nth_at(heap, coll, key),
        other => Err(wrong("vector or map", &other)),
    }
}

pub(crate) fn get(heap: &Heap, coll: Value, key: Value) -> Result<Option<Value>, CollectionError> {
    match body(heap, &coll) {
        Body::Vector(vec) => Ok(vec.get(index_from_value(&key)?)),
        Body::Map(map) => Ok(map.get(&key)),
        other => Err(wrong("vector or map", &other)),
    }
}

pub(crate) fn assoc(
    heap: &Heap,
    coll: Value,
    key: Value,
    value: Value,
) -> Result<Value, CollectionError> {
    match body(heap, &coll) {
        Body::Vector(vec) => {
            let index = index_from_value(&key)?;
            let count = vec.len();
            if index == count {
                return alloc(
                    heap,
                    HeapObject::Vector(vec.push(value.clone())),
                    Some(&[coll, value]),
                );
            }
            let old = vec
                .get(index)
                .ok_or(CollectionError::IndexOutOfBounds { index, count })?;
            if identical(&old, &value) {
                return Ok(coll);
            }
            let updated = vec
                .set(index, value.clone())
                .ok_or(CollectionError::IndexOutOfBounds { index, count })?;
            alloc(heap, HeapObject::Vector(updated), Some(&[coll, value]))
        }
        Body::Map(map) => match map.assoc(key.clone(), value.clone()) {
            Some(updated) => alloc(heap, HeapObject::Map(updated), Some(&[coll, key, value])),
            None => Ok(coll),
        },
        other => Err(wrong("vector or map", &other)),
    }
}

/// "Nothing changed" for a vector slot: numbers by bits, else `===`.
fn identical(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => x.to_bits() == y.to_bits(),
        _ => a.strict_equals(b),
    }
}

pub(crate) fn dissoc(heap: &Heap, map: Value, key: Value) -> Result<Value, CollectionError> {
    match map_body(heap, &map)?.dissoc(&key) {
        Some(updated) => alloc(heap, HeapObject::Map(updated), Some(&[map])),
        None => Ok(map),
    }
}

pub(crate) fn conj(heap: &Heap, vec: Value, value: Value) -> Result<Value, CollectionError> {
    let updated = vector_body(heap, &vec)?.push(value.clone());
    alloc(heap, HeapObject::Vector(updated), Some(&[vec, value]))
}

pub(crate) fn pop(heap: &Heap, vec: Value) -> Result<Value, CollectionError> {
    let updated = vector_body(heap, &vec)?
        .pop()
        .ok_or(CollectionError::IndexOutOfBounds { index: 0, count: 0 })?;
    alloc(heap, HeapObject::Vector(updated), Some(&[vec]))
}

pub(crate) fn items(heap: &Heap, vec: Value) -> Result<Vec<Value>, CollectionError> {
    Ok(vector_body(heap, &vec)?.to_vec())
}

pub(crate) fn entries(heap: &Heap, map: Value) -> Result<Vec<(Value, Value)>, CollectionError> {
    Ok(map_body(heap, &map)?.to_vec())
}

pub(crate) fn occurrence(heap: &Heap, occ: Value) -> Result<Occurrence, CollectionError> {
    match body(heap, &occ) {
        Body::Occurrence(o) => Ok(o),
        other => Err(wrong("occurrence", &other)),
    }
}

/// Structural value equality. See the module docs.
pub(crate) fn value_equals(heap: &Heap, a: Value, b: Value) -> bool {
    value_equals_counted(heap, a, b, &mut 0)
}

/// [`value_equals`], counting the collection pairs whose children it expands.
///
/// Each pair of collection identities is expanded at most once: a pair met
/// again (through structural sharing) is either still being compared or
/// already found equal, since any difference ends the walk. Work is
/// therefore bounded by the number of distinct object pairs, not by the
/// number of paths through a shared graph.
fn value_equals_counted(heap: &Heap, a: Value, b: Value, expanded: &mut usize) -> bool {
    let mut seen: HashSet<(ObjectId, ObjectId)> = HashSet::new();
    // Resolve occurrences (transparent for equality) to the underlying value
    // and its body.
    let resolve = |mut v: Value| loop {
        match body(heap, &v) {
            Body::Occurrence(o) => v = o.value,
            b => return (v, b),
        }
    };
    let mut work = vec![(a, b)];
    while let Some((a, b)) = work.pop() {
        if a.same_object(&b) {
            continue;
        }
        let ((a, body_a), (b, body_b)) = (resolve(a), resolve(b));
        if let (Value::Object(x), Value::Object(y)) = (&a, &b) {
            let (x, y) = (*x, *y);
            let collections = matches!(
                (&body_a, &body_b),
                (Body::Vector(_), Body::Vector(_)) | (Body::Map(_), Body::Map(_))
            );
            if x == y || (collections && !seen.insert((x, y))) {
                continue;
            }
        }
        match (body_a, body_b) {
            (Body::Vector(x), Body::Vector(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                *expanded += 1;
                work.extend(x.to_vec().into_iter().zip(y.to_vec()));
            }
            (Body::Map(x), Body::Map(y)) => {
                if x.len() != y.len() {
                    return false;
                }
                *expanded += 1;
                let mut missing = false;
                x.for_each(|k, v| match y.get(k) {
                    Some(w) => work.push((v.clone(), w)),
                    None => missing = true,
                });
                if missing {
                    return false;
                }
            }
            // Primitives by SameValueZero; other heap objects by identity.
            _ => {
                if !map::key_eq(&a, &b) {
                    return false;
                }
            }
        }
    }
    true
}

/// The embedder API. Every method works on locals of this scope; results are
/// new locals. See the module docs for the contract.
impl<'s> Scope<'s> {
    fn values(&self, locals: &[Local<'s>]) -> Vec<Value> {
        locals.iter().map(|l| self.local_value(*l)).collect()
    }

    fn result(&self, r: Result<Value, CollectionError>) -> Result<Local<'s>, CollectionError> {
        r.map(|v| self.new_local(v))
    }

    /// A new vector of `items`, in order.
    pub fn new_vector(&self, items: &[Local<'s>]) -> Result<Local<'s>, CollectionError> {
        self.result(new_vector(self.heap(), &self.values(items)))
    }

    /// A new map of `entries`; a later duplicate key replaces an earlier one.
    pub fn new_map(
        &self,
        entries: &[(Local<'s>, Local<'s>)],
    ) -> Result<Local<'s>, CollectionError> {
        let entries: Vec<(Value, Value)> = entries
            .iter()
            .map(|(k, v)| (self.local_value(*k), self.local_value(*v)))
            .collect();
        self.result(new_map(self.heap(), &entries))
    }

    /// A new occurrence of `value` at `at`.
    pub fn new_occurrence(
        &self,
        value: Local<'s>,
        at: Provenance,
    ) -> Result<Local<'s>, CollectionError> {
        self.result(new_occurrence(self.heap(), self.local_value(value), at))
    }

    /// The persistent kind of a value, or `None` for primitives and other
    /// objects.
    pub fn persistent_kind(&self, v: Local<'s>) -> Option<PersistentKind> {
        kind_of(self.heap(), &self.local_value(v))
    }

    /// Element count of a vector or entry count of a map.
    pub fn count(&self, coll: Local<'s>) -> Result<usize, CollectionError> {
        count(self.heap(), self.local_value(coll))
    }

    /// The element of a vector at `index`.
    pub fn nth(&self, vec: Local<'s>, index: usize) -> Result<Local<'s>, CollectionError> {
        self.result(nth(self.heap(), self.local_value(vec), index))
    }

    /// Lookup: a vector element by index value, or a map value by key;
    /// `None` when out of bounds or absent.
    pub fn get(
        &self,
        coll: Local<'s>,
        key: Local<'s>,
    ) -> Result<Option<Local<'s>>, CollectionError> {
        let found = get(self.heap(), self.local_value(coll), self.local_value(key))?;
        Ok(found.map(|v| self.new_local(v)))
    }

    /// A new version with `key` set to `value` (vectors: index `<= count`).
    pub fn assoc(
        &self,
        coll: Local<'s>,
        key: Local<'s>,
        value: Local<'s>,
    ) -> Result<Local<'s>, CollectionError> {
        let (c, k, v) = (
            self.local_value(coll),
            self.local_value(key),
            self.local_value(value),
        );
        self.result(assoc(self.heap(), c, k, v))
    }

    /// A new version of a map without `key`.
    pub fn dissoc(&self, map: Local<'s>, key: Local<'s>) -> Result<Local<'s>, CollectionError> {
        let (m, k) = (self.local_value(map), self.local_value(key));
        self.result(dissoc(self.heap(), m, k))
    }

    /// A new version of a vector with `value` appended.
    pub fn conj(&self, vec: Local<'s>, value: Local<'s>) -> Result<Local<'s>, CollectionError> {
        let (c, v) = (self.local_value(vec), self.local_value(value));
        self.result(conj(self.heap(), c, v))
    }

    /// A new version of a vector without its last element.
    pub fn pop(&self, vec: Local<'s>) -> Result<Local<'s>, CollectionError> {
        self.result(pop(self.heap(), self.local_value(vec)))
    }

    /// The elements of a vector, in order.
    pub fn items(&self, vec: Local<'s>) -> Result<Vec<Local<'s>>, CollectionError> {
        let values = items(self.heap(), self.local_value(vec))?;
        Ok(self.new_locals(values))
    }

    /// The entries of a map, in unspecified (but deterministic) order.
    pub fn entries(&self, map: Local<'s>) -> Result<Vec<(Local<'s>, Local<'s>)>, CollectionError> {
        let pairs = entries(self.heap(), self.local_value(map))?;
        let locals = self.new_locals(pairs.into_iter().flat_map(|(k, v)| [k, v]));
        Ok(locals.chunks(2).map(|kv| (kv[0], kv[1])).collect())
    }

    /// The value an occurrence wraps.
    pub fn occurrence_value(&self, occ: Local<'s>) -> Result<Local<'s>, CollectionError> {
        let o = occurrence(self.heap(), self.local_value(occ))?;
        Ok(self.new_local(o.value))
    }

    /// Where an occurrence occurred.
    pub fn provenance(&self, occ: Local<'s>) -> Result<Provenance, CollectionError> {
        Ok(occurrence(self.heap(), self.local_value(occ))?.provenance)
    }

    /// Structural value equality (not identity; provenance is ignored).
    pub fn value_equals(&self, a: Local<'s>, b: Local<'s>) -> bool {
        value_equals(self.heap(), self.local_value(a), self.local_value(b))
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use crate::heap::ObjectKind;

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    #[test]
    fn index_values_follow_the_index_contract() {
        assert_eq!(index_from_value(&n(0.0)), Ok(0));
        assert_eq!(index_from_value(&n(-0.0)), Ok(0));
        assert_eq!(index_from_value(&n(7.0)), Ok(7));
        assert_eq!(index_from_value(&n(MAX_INDEX)), Ok(MAX_INDEX as usize));
        for bad in [
            n(-1.0),
            n(1.5),
            n(f64::NAN),
            n(f64::INFINITY),
            n(MAX_INDEX + 1.0),
            Value::Null,
            Value::Bool(true),
            Value::Object(ObjectId::from_index(0)),
        ] {
            assert_eq!(index_from_value(&bad), Err(CollectionError::InvalidIndex));
        }
    }

    #[test]
    fn value_equality_expands_each_shared_pair_once() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        // a_i = [a_(i-1), a_(i-1)] (and b_i likewise, built independently):
        // 60 levels, 2^60 paths, 120 vectors. Occurrences wrap one side.
        let leaf = |x| new_occurrence(&heap, n(x), Provenance::default()).unwrap();
        let (mut a, mut b) = (n(1.0), leaf(1.0));
        for _ in 0..60 {
            a = new_vector(&heap, &[a.clone(), a]).unwrap();
            let shared = new_vector(&heap, &[b.clone(), b]).unwrap();
            b = new_occurrence(&heap, shared, Provenance::default()).unwrap();
        }
        let mut expanded = 0;
        assert!(value_equals_counted(&heap, a.clone(), b, &mut expanded));
        assert_eq!(expanded, 60, "one expansion per distinct pair");
        // A difference at the bottom is still found.
        let mut c = n(2.0);
        for _ in 0..60 {
            c = new_vector(&heap, &[c.clone(), c]).unwrap();
        }
        let mut expanded = 0;
        assert!(!value_equals_counted(&heap, a, c, &mut expanded));
        assert!(expanded <= 60);
    }

    #[test]
    fn versions_still_share_trie_nodes_after_collection() {
        let heap = Arc::new(Heap::default());
        let (old, new) = {
            let scope = Scope::new(&heap);
            let items: Vec<Local<'_>> = (0..2000)
                .map(|i| scope.primitive(crate::Primitive::Number(f64::from(i))))
                .collect();
            let old = scope.new_vector(&items).unwrap();
            let k = scope.primitive(crate::Primitive::Number(1500.0));
            let x = scope.new_object();
            let new = scope.assoc(old, k, x).unwrap();
            (scope.persist(old), scope.persist(new))
        };
        // Everything allocated is reachable from the two roots.
        assert_eq!(heap.collect().unwrap().reclaimed, 0);
        let nodes = |r: &crate::Rooted| {
            let Value::Object(id) = r.value_in(&heap).unwrap() else {
                panic!("not an object")
            };
            heap.read(id, |o| match o {
                HeapObject::Vector(v) => v.node_addresses(),
                other => panic!("not a vector: {other:?}"),
            })
            .unwrap()
        };
        let old_nodes: std::collections::HashSet<usize> = nodes(&old).into_iter().collect();
        let fresh = nodes(&new)
            .into_iter()
            .filter(|a| !old_nodes.contains(a))
            .count();
        assert_eq!(fresh, 3, "only the root-to-leaf path differs");
        assert_eq!(
            nth(&heap, old.value_in(&heap).unwrap(), 1500),
            Ok(Value::Number(1500.0))
        );
    }

    #[test]
    fn collection_objects_report_their_kinds_and_trace_their_edges() {
        let heap = Arc::new(Heap::default());
        let scope = Scope::new(&heap);
        let leaf = scope.new_object();
        let vec = scope.new_vector(&[leaf, leaf]).unwrap();
        let key = scope.new_object();
        let map = scope.new_map(&[(key, vec)]).unwrap();
        let occ = scope
            .new_occurrence(
                map,
                Provenance {
                    source: 1,
                    line: 2,
                    column: 3,
                },
            )
            .unwrap();
        let id = |l| match scope.raw(l) {
            Value::Object(id) => id,
            other => panic!("not an object: {other:?}"),
        };
        assert_eq!(heap.kind(id(vec)), Some(ObjectKind::Vector));
        assert_eq!(heap.kind(id(map)), Some(ObjectKind::Map));
        assert_eq!(heap.kind(id(occ)), Some(ObjectKind::Occurrence));
        let edges = |l| {
            let mut out = Vec::new();
            heap.read(id(l), |o| o.trace(&mut |e| out.push(e))).unwrap();
            out
        };
        assert_eq!(edges(vec), vec![id(leaf).into(), id(leaf).into()]);
        assert_eq!(edges(map), vec![id(key).into(), id(vec).into()]);
        assert_eq!(edges(occ), vec![id(map).into()]);
    }
}
