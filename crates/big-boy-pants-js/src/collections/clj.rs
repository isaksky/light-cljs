//! ClojureScript core operations on native forms (light-cljs-477).
//!
//! The `__rt.clj*` intrinsics ([`crate::vm::intrinsics`], "Clojure core
//! intrinsics") are what a compile-time macro unit calls for `count`,
//! `first`, `get`, `assoc`, `contains?` and the other core operations
//! when light-cljs lowers them for this engine. Unlike the strict
//! `__rt.form*` operations of [`crate::collections::form`], they follow the
//! ClojureScript functions they replace:
//!
//! * **Occurrences are transparent.** A collection or index that arrives
//!   wrapped in an occurrence (a macro input with its provenance) is read
//!   through it. Elements, keys and values are returned as stored, so an
//!   input subtree keeps its occurrence (and its identity) when a macro
//!   returns it.
//! * **`nil` puns.** `(count nil)` is `0`, `(first nil)`, `(seq nil)` and
//!   `(get nil k)` are `nil`, `(assoc nil k v)` is `{k v}`, `(conj nil x)`
//!   is `(x)`, `(contains? nil k)` is `false`.
//! * **Sequence storage preserves language markers.** `seq`, `rest` and
//!   `next` use positional list bodies (or `nil`): a vector's elements
//!   (its `seq` in chunks, below), a map's entries as two-element
//!   vectors and a set's members (in trie order for hashed forms, source
//!   order for positional literals), and a string's
//!   UTF-16 code units as one-unit strings. A nonempty list is its own
//!   `seq` (the same value, occurrence included). The `rest` and `next` of
//!   a list or vector share its trie (light-cljs-502): O(1), allocating one
//!   list object and no trie node, so walking one with `next` is linear.
//!   Those of a map, set or string copy the remaining elements. The seq of a
//!   vector of more than 32 elements is a chunked cons sequence over its
//!   trie (32 elements a chunk, as the runtime's `ChunkedSeq`). Lazy and cons sequences (light-cljs-479,
//!   [`crate::collections::lazy`]) are read through, realizing what an
//!   operation needs: `first`, `rest` and `cons` realize one cell, `count`,
//!   `nth` as far as they read; `cons` and `conj` keep a lazy sequence as an
//!   unrealized tail. Every `concat` is lazy. Positional sequence views
//!   retain a non-IList tag through metadata and shared tails; plain lists
//!   and Cons cells alone satisfy the builtin IList marker. Predicates
//!   never realize lazy values or inspect sequence elements.
//! * **Unchanged updates return their input.** An `assoc` of an identical
//!   value, a `dissoc` or `disj` of an absent key, a `conj` of a present set
//!   member and `with-meta` of the same metadata return the operand itself
//!   (the occurrence, if it was one). A map seq entry's `assoc`, `conj`
//!   and `with-meta` instead produce ordinary vectors.
//!
//! A value of a kind the operation does not support (a number for `seq`, a
//! JavaScript object for `count`) fails with a catchable JavaScript
//! `TypeError` whose message names the operation and the kind,
//! `unsupported operation: native macro runtime seq on number`; `nth` past
//! the end without a default is a catchable `RangeError`
//! `index out of bounds`; invalid `keyword` and `symbol` operands are a
//! catchable plain `Error` with `keywordCore`'s or `symbolCore`'s message.
//! These are the diagnostics of the JavaScript adapters they replace. Allocation is charged to the heap's byte budget
//! like every form allocation: exhaustion is the engine's usual
//! `HeapLimit`/`OutOfMemory` failure.

use super::form::{self, FormBody, FormKind};
use super::{CollectionError, PVector, index_from_value, lazy};
use crate::heap::{Heap, HeapObject};
use crate::value::{JsString, Value};

/// Why a Clojure core operation failed.
#[derive(Debug)]
pub(crate) enum CljError {
    /// An allocation or form operation failed ([`CollectionError`]).
    Collection(CollectionError),
    /// The operand's kind is not supported by `op` (a `TypeError`).
    Unsupported {
        op: &'static str,
        found: &'static str,
    },
    /// `nth` beyond the end without a default (a `RangeError`).
    OutOfBounds,
    /// A constructor's operands have the wrong types (a plain `Error` with
    /// the message of the JavaScript function it replaces).
    Operands(&'static str),
    /// A callable collection received the wrong number of arguments.
    Arity(usize),
    /// The operation needs this unrealized lazy sequence realized first
    /// (light-cljs-479): not an error, a request the VM performs before
    /// executing the operation again ([`crate::collections::lazy`]).
    Realize(Value),
    /// A realized lazy sequence contains itself: it is infinite (a
    /// `RangeError`).
    Infinite,
}

impl From<CollectionError> for CljError {
    fn from(e: CollectionError) -> Self {
        CljError::Collection(e)
    }
}

impl From<form::SeqError> for CljError {
    fn from(e: form::SeqError) -> Self {
        match e {
            form::SeqError::Pending(v) => CljError::Realize(v),
            form::SeqError::Cycle => CljError::Infinite,
        }
    }
}

impl CljError {
    /// The JavaScript error message.
    pub(crate) fn message(&self) -> String {
        match self {
            CljError::Collection(e) => e.to_string(),
            CljError::Unsupported { op, found } => {
                format!("unsupported operation: native macro runtime {op} on {found}")
            }
            CljError::OutOfBounds => "index out of bounds".to_string(),
            CljError::Operands(message) => (*message).to_string(),
            CljError::Arity(count) => format!("Invalid arity: {count}"),
            CljError::Realize(_) => "lazy sequence realization pending".to_string(),
            CljError::Infinite => "infinite lazy sequence: it contains itself".to_string(),
        }
    }
}

type Result<T> = std::result::Result<T, CljError>;

/// What a value is, for these operations (occurrences unwrapped).
enum Shape {
    Nil,
    Str(JsString),
    Form(FormKind, std::sync::Arc<form::FormObject>),
    Other(&'static str),
}

/// `typeof` of a non-form value, as the JavaScript adapters reported it.
fn type_of(heap: &Heap, v: &Value) -> &'static str {
    match v {
        Value::Object(id) => heap
            .read(*id, |o| match o {
                HeapObject::Function(_) | HeapObject::BoundFunction(_) | HeapObject::Builtin(_) => {
                    "function"
                }
                _ => "object",
            })
            .unwrap_or("object"),
        other => other.type_name(),
    }
}

fn shape(heap: &Heap, v: &Value) -> Shape {
    let v = form::unwrap(heap, v.clone());
    match &v {
        Value::Null | Value::Undefined => Shape::Nil,
        Value::String(s) => Shape::Str(s.clone()),
        _ => match form::form_object(heap, &v) {
            Some(f) => Shape::Form(f.kind(), f),
            None => Shape::Other(type_of(heap, &v)),
        },
    }
}

fn unsupported(op: &'static str, shape: &Shape) -> CljError {
    let found = match shape {
        Shape::Nil => "nil",
        Shape::Str(_) => "string",
        Shape::Form(kind, _) => kind.name(),
        Shape::Other(name) => name,
    };
    CljError::Unsupported { op, found }
}

/// The one-code-unit string at `index`, charged to the heap's budget.
fn code_unit(heap: &Heap, s: &JsString, index: usize) -> Result<Option<Value>> {
    match s.code_units().get(index) {
        None => Ok(None),
        Some(&unit) => JsString::copy_units(heap, &[unit])
            .map(|s| Some(Value::String(s)))
            .ok_or(CljError::Collection(CollectionError::HeapLimit)),
    }
}

/// A map entry as the two-element form vector `seq` yields.
fn entry(heap: &Heap, key: &Value, value: &Value) -> Result<Value> {
    Ok(form::new_map_entry(heap, key, value)?)
}

/// The elements `seq` of `v` would hold, or `None` for `nil`. A lazy or
/// cons sequence is read through, realizing what it needs (light-cljs-479).
fn seq_items(heap: &Heap, op: &'static str, v: &Value) -> Result<Option<Vec<Value>>> {
    if matches!(shape(heap, v), Shape::Nil) {
        return Ok(None);
    }
    let mut out = Vec::new();
    let mut visited = lazy::Visited::default();
    let mut cur = v.clone();
    loop {
        let head = form::realized_head(heap, cur, &mut visited)?;
        if let Shape::Form(FormKind::Cons, f) = shape(heap, &head)
            && let FormBody::Cons(c) = f.body()
        {
            out.extend(c.elements());
            cur = c.rest.clone();
            continue;
        }
        out.extend(collection_items(heap, op, &head)?.unwrap_or_default());
        return Ok(Some(out));
    }
}

/// The elements of one collection, string or `nil` (not a lazy or cons
/// sequence), or `None` for `nil`.
fn collection_items(heap: &Heap, op: &'static str, v: &Value) -> Result<Option<Vec<Value>>> {
    let shape = shape(heap, v);
    Ok(Some(match &shape {
        Shape::Nil => return Ok(None),
        Shape::Str(s) => {
            let mut items = Vec::new();
            items
                .try_reserve_exact(s.len())
                .map_err(|_| CljError::Collection(CollectionError::HeapLimit))?;
            for index in 0..s.len() {
                items.push(code_unit(heap, s, index)?.expect("in bounds"));
            }
            items
        }
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => items.to_vec(),
            FormBody::Set(_) => form::items(heap, &form::unwrap(heap, v.clone()))?,
            FormBody::Map(map) => {
                let mut entries = Vec::with_capacity(map.len());
                for (k, v) in form::entries(heap, &form::unwrap(heap, v.clone()))? {
                    entries.push(entry(heap, &k, &v)?);
                }
                entries
            }
            FormBody::Struct(s) if s.info.record => {
                let mut entries = Vec::new();
                for (k, v) in form::record_entries(heap, s)? {
                    entries.push(entry(heap, &k, &v)?);
                }
                entries
            }
            // Read by `seq_items`, never here.
            FormBody::Lazy(_) | FormBody::Cons(_) => return Err(unsupported(op, &shape)),
            FormBody::CompilerLiteral { .. }
            | FormBody::Keyword(_)
            | FormBody::Symbol(_)
            | FormBody::Struct(_)
            | FormBody::Type(_)
            | FormBody::Reduced(_) => return Err(unsupported(op, &shape)),
        },
        Shape::Other(_) => return Err(unsupported(op, &shape)),
    }))
}

/// Whether `v` (occurrences unwrapped, lazy forms not followed) is a lazy
/// or cons sequence.
fn is_lazy_seq(heap: &Heap, v: &Value) -> bool {
    matches!(
        form::kind(heap, &form::unwrap(heap, v.clone())),
        Some(FormKind::Lazy | FormKind::Cons)
    )
}

/// Builtin collection markers inspect the original value, never a lazy head.
/// User protocol membership is resolved by the task-local runtime adapter.
pub(crate) fn is_list(heap: &Heap, v: &Value) -> bool {
    form::form_object(heap, &form::unwrap(heap, v.clone())).is_some_and(|f| f.is_list())
}

pub(crate) fn is_seq(heap: &Heap, v: &Value) -> bool {
    matches!(
        shape(heap, v),
        Shape::Form(FormKind::List | FormKind::Lazy | FormKind::Cons, _)
    )
}

pub(crate) fn is_sequential(heap: &Heap, v: &Value) -> bool {
    match shape(heap, v) {
        Shape::Form(_, f) => f.sequential(),
        _ => false,
    }
}

pub(crate) fn is_coll(heap: &Heap, v: &Value) -> bool {
    match shape(heap, v) {
        Shape::Form(kind, f) => {
            is_sequential(heap, v)
                || matches!(kind, FormKind::Map | FormKind::Set)
                || f.record().is_some()
        }
        _ => false,
    }
}

pub(crate) fn is_vector(heap: &Heap, v: &Value) -> bool {
    matches!(shape(heap, v), Shape::Form(FormKind::Vector, _))
}

pub(crate) fn is_map_entry(heap: &Heap, v: &Value) -> bool {
    form::form_object(heap, &form::unwrap(heap, v.clone())).is_some_and(|f| f.is_map_entry())
}

/// Persistent stack observation does not realize other sequence kinds.
pub(crate) fn peek(heap: &Heap, v: &Value) -> Result<Value> {
    match shape(heap, v) {
        Shape::Nil => Ok(Value::Null),
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) if f.is_list() => Ok(items.get(0).unwrap_or(Value::Null)),
            FormBody::Vector(items) => Ok(items
                .len()
                .checked_sub(1)
                .and_then(|i| items.get(i))
                .unwrap_or(Value::Null)),
            FormBody::CompilerLiteral { tag, payload, .. } if tag == "queue" => {
                first(heap, payload)
            }
            _ => Err(CljError::Operands(
                "No protocol method IStack.-peek defined for this value",
            )),
        },
        _ => Err(CljError::Operands(
            "No protocol method IStack.-peek defined for this value",
        )),
    }
}

/// The head of `v` with realized lazy sequences followed, and its shape.
fn head(heap: &Heap, v: &Value) -> Result<(Value, Shape)> {
    let head = form::sequence_head(heap, v.clone(), &mut lazy::Visited::default())?;
    let shape = shape(heap, &head);
    Ok((head, shape))
}

/// The cons body of a shape, if it is one.
fn cons_body(shape: &Shape) -> Option<&lazy::ConsBody> {
    match shape {
        Shape::Form(_, f) => match f.body() {
            FormBody::Cons(c) => Some(c),
            _ => None,
        },
        _ => None,
    }
}

/// What a thunk's value is stored as: a sequence (`nil`, a list, or a lazy
/// or cons sequence) as it is, a collection or string as the list of its
/// seq (`nil` when empty), anything else as it is (reading it later fails
/// as `seq` of it does).
pub(crate) fn realized_seq(heap: &Heap, v: &Value) -> Result<Value> {
    match shape(heap, v) {
        Shape::Nil | Shape::Other(_) => Ok(v.clone()),
        Shape::Form(FormKind::List | FormKind::Lazy | FormKind::Cons, _) => Ok(v.clone()),
        Shape::Form(FormKind::Keyword | FormKind::Symbol | FormKind::Type, _) => Ok(v.clone()),
        Shape::Form(FormKind::Struct, f) if f.record().is_none() => Ok(v.clone()),
        Shape::Form(FormKind::Vector, f) => match f.body() {
            FormBody::Vector(items) => vector_seq(heap, items, v),
            _ => Ok(v.clone()),
        },
        _ => list_or_nil(
            heap,
            &collection_items(heap, "seq", v)?.unwrap_or_default(),
            copied_sequence_counted(heap, v),
        ),
    }
}

/// A new list of `items`, or `nil` when there are none.
fn list_or_nil(heap: &Heap, items: &[Value], counted: bool) -> Result<Value> {
    if items.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(form::new_sequence(heap, items, counted)?)
    }
}

/// Indexed, record and array/sorted-map views are counted; hash-map and set
/// views are not. This follows the source kind, never its elements.
fn copied_sequence_counted(heap: &Heap, value: &Value) -> bool {
    match shape(heap, value) {
        Shape::Form(_, f) => match f.body() {
            FormBody::Map(form::FormMap::Positional(_)) => true,
            FormBody::Map(_) | FormBody::Set(_) => false,
            _ => true,
        },
        _ => true,
    }
}

/// ClojureScript truth: everything but `nil` and `false`.
pub(crate) fn truth(heap: &Heap, v: &Value) -> bool {
    !matches!(
        form::unwrap(heap, v.clone()),
        Value::Null | Value::Undefined | Value::Bool(false)
    )
}

/// `zero?`: strict numeric zero, including negative zero; occurrences unwrap.
pub(crate) fn is_zero(heap: &Heap, v: &Value) -> bool {
    matches!(form::unwrap(heap, v.clone()), Value::Number(n) if n == 0.0)
}

/// `false?`: only boolean false, through an input occurrence.
pub(crate) fn is_false(heap: &Heap, v: &Value) -> bool {
    matches!(form::unwrap(heap, v.clone()), Value::Bool(false))
}

/// Builtin editable collections used by `into`'s reducer selection. A map
/// entry has a vector body but, like a record, is not editable. Read through
/// occurrences without realizing sequences or invoking user code.
pub(crate) fn is_editable(heap: &Heap, v: &Value) -> bool {
    match shape(heap, v) {
        Shape::Form(FormKind::Vector, f) => !f.is_map_entry(),
        Shape::Form(FormKind::Map | FormKind::Set, f) => !f.is_sorted(),
        _ => false,
    }
}

/// `reduced?`, transparent to input occurrences.
pub(crate) fn is_reduced(heap: &Heap, v: &Value) -> bool {
    form::kind(heap, &form::unwrap(heap, v.clone())) == Some(FormKind::Reduced)
}

/// Unwrap exactly one early-termination marker, preserving other operands
/// (including occurrences) by identity.
pub(crate) fn unreduced(heap: &Heap, v: &Value) -> Value {
    let unwrapped = form::unwrap(heap, v.clone());
    match form::form_object(heap, &unwrapped)
        .as_deref()
        .map(|f| f.body())
    {
        Some(FormBody::Reduced(value)) => value.clone(),
        _ => v.clone(),
    }
}

/// `count`: elements, entries, members or UTF-16 code units; `nil` is 0.
/// A finite integer range retains its count; other lazy or cons sequences
/// are realized to their end.
pub(crate) fn count(heap: &Heap, v: &Value) -> Result<Value> {
    if is_lazy_seq(heap, v) {
        let n =
            form::sequence_count(heap, v)?.ok_or_else(|| unsupported("count", &shape(heap, v)))?;
        return Ok(Value::Number(n));
    }
    let shape = shape(heap, v);
    let n = match &shape {
        Shape::Nil => 0,
        Shape::Str(s) => s.len(),
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => items.len(),
            FormBody::Map(map) | FormBody::Set(map) => map.len(),
            FormBody::Struct(s) if s.info.record => {
                s.info.fields.len() + s.ext_map(heap).map_or(0, |e| e.len())
            }
            FormBody::CompilerLiteral { .. }
            | FormBody::Keyword(_)
            | FormBody::Symbol(_)
            | FormBody::Struct(_)
            | FormBody::Type(_)
            | FormBody::Reduced(_)
            | FormBody::Lazy(_)
            | FormBody::Cons(_) => return Err(unsupported("count", &shape)),
        },
        Shape::Other(_) => return Err(unsupported("count", &shape)),
    };
    Ok(Value::Number(n as f64))
}

/// `seq`: `nil` when empty, a nonempty list itself, otherwise a new list.
/// A lazy sequence's seq is the seq of what it realized (realizing it
/// first); a cons sequence is its own.
pub(crate) fn seq(heap: &Heap, v: &Value) -> Result<Value> {
    let (head_value, head_shape) = head(heap, v)?;
    // A value that is not lazy keeps its occurrence.
    let own_is_lazy = form::kind(heap, &form::unwrap(heap, v.clone())) == Some(FormKind::Lazy);
    let own = if own_is_lazy { head_value } else { v.clone() };
    if let Shape::Form(FormKind::List, f) = &head_shape
        && let FormBody::List(items) = f.body()
    {
        return Ok(if items.len() == 0 { Value::Null } else { own });
    }
    if let Some(c) = cons_body(&head_shape) {
        // The seq of a lazy sequence (a `concat` is one) has no metadata,
        // as the runtime's seq of a `LazySeq` has none.
        let through_lazy = own_is_lazy || matches!(c.kind, lazy::ConsKind::Concat { .. });
        if through_lazy && c.kind.plain() != c.kind {
            return Ok(form::cons_sharing(
                heap,
                c.items.clone(),
                c.start,
                c.rest.clone(),
                c.kind.plain(),
                &form::unwrap(heap, own),
            )?);
        }
        return Ok(own);
    }
    // A vector's seq is chunked, as the runtime's `ChunkedSeq` is.
    if let Shape::Form(FormKind::Vector, f) = &head_shape
        && let FormBody::Vector(items) = f.body()
    {
        return vector_seq(heap, items, &own);
    }
    // A compiler queue is sequential, but its PersistentQueueSeq view is
    // uncounted and unchunked even when the payload spans many vector chunks.
    if let Shape::Form(FormKind::CompilerLiteral, f) = &head_shape
        && let FormBody::CompilerLiteral { tag, payload, .. } = f.body()
        && tag == "queue"
    {
        return list_or_nil(
            heap,
            &collection_items(heap, "seq", payload)?.unwrap_or_default(),
            false,
        );
    }
    match collection_items(heap, "seq", &own)? {
        None => Ok(Value::Null),
        Some(items) => list_or_nil(heap, &items, copied_sequence_counted(heap, &own)),
    }
}

/// The seq of the vector `source` of `items` (nonempty): a chunked sequence
/// over its trie, read 32 elements a chunk (`chunk-first`); O(1), copying
/// nothing.
fn vector_seq(heap: &Heap, items: &PVector, source: &Value) -> Result<Value> {
    if items.len() == 0 {
        return Ok(Value::Null);
    }
    let source = form::unwrap(heap, source.clone());
    // The runtime's seq of a vector of at most 32 elements is an unchunked
    // `IndexedSeq`.
    if items.len() <= CHUNK {
        return Ok(form::list_sharing(heap, items.clone(), &source)?);
    }
    Ok(form::cons_sharing(
        heap,
        items.clone(),
        0,
        Value::Null,
        lazy::ConsKind::ChunkedSeq,
        &source,
    )?)
}

/// `first`: the first element of the sequence, or `nil`.
pub(crate) fn first(heap: &Heap, v: &Value) -> Result<Value> {
    let (v, shape) = head(heap, v)?;
    match &shape {
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => {
                return Ok(items.get(0).unwrap_or(Value::Null));
            }
            FormBody::Cons(c) => return Ok(c.first()),
            FormBody::Set(_) | FormBody::Map(_) | FormBody::Struct(_) => {}
            FormBody::CompilerLiteral { .. }
            | FormBody::Keyword(_)
            | FormBody::Symbol(_)
            | FormBody::Type(_)
            | FormBody::Lazy(_)
            | FormBody::Reduced(_) => {
                return Err(unsupported("first", &shape));
            }
        },
        Shape::Str(s) => return Ok(code_unit(heap, s, 0)?.unwrap_or(Value::Null)),
        Shape::Nil => return Ok(Value::Null),
        Shape::Other(_) => return Err(unsupported("first", &shape)),
    }
    Ok(collection_items(heap, "first", &v)?
        .and_then(|items| items.into_iter().next())
        .unwrap_or(Value::Null))
}

/// The elements after the first, as `rest` and `next` see them, of a value
/// that is not a lazy or cons sequence.
enum Tail {
    /// Of a list or vector: its trie without the first element
    /// ([`PVector::drop_first`](super::PVector::drop_first)), and the
    /// (unwrapped) list or vector it shares it with.
    Shared(super::PVector, Value),
    /// Of a map, set, record or string: a copy.
    Copied(Vec<Value>),
}

fn tail(heap: &Heap, op: &'static str, v: &Value) -> Result<Tail> {
    if let Shape::Form(_, f) = shape(heap, v)
        && let FormBody::List(items) | FormBody::Vector(items) = f.body()
    {
        return Ok(Tail::Shared(
            items.drop_first(1),
            form::unwrap(heap, v.clone()),
        ));
    }
    Ok(Tail::Copied(
        collection_items(heap, op, v)?
            .map(|items| items.into_iter().skip(1).collect())
            .unwrap_or_default(),
    ))
}

/// The rest of a cons sequence: its next element on, or its tail (`()` for
/// none). The tail is returned as it is, so a lazy tail is not realized.
fn cons_rest(heap: &Heap, c: &lazy::ConsBody, source: &Value) -> Result<Value> {
    if c.len() > 1 {
        return Ok(form::cons_sharing(
            heap,
            c.items.clone(),
            c.start + 1,
            c.rest.clone(),
            c.kind.rest(),
            source,
        )?);
    }
    match &c.rest {
        Value::Null | Value::Undefined => Ok(form::new_list(heap, &[])?),
        rest => Ok(rest.clone()),
    }
}

/// `rest`: the elements after the first, as a list (empty, never `nil`);
/// for a lazy or cons sequence, the rest of its seq, whose lazy tail stays
/// unrealized.
pub(crate) fn rest(heap: &Heap, v: &Value) -> Result<Value> {
    let (v, shape) = head(heap, v)?;
    if let Some(c) = cons_body(&shape) {
        return cons_rest(heap, c, &v);
    }
    match tail(heap, "rest", &v)? {
        Tail::Shared(items, source) => Ok(form::list_sharing(heap, items, &source)?),
        Tail::Copied(items) => Ok(form::new_sequence(
            heap,
            &items,
            copied_sequence_counted(heap, &v),
        )?),
    }
}

/// `next`: the elements after the first, as a list, or `nil` for none; of
/// a lazy or cons sequence, the seq of its rest.
pub(crate) fn next(heap: &Heap, v: &Value) -> Result<Value> {
    let (v, shape) = head(heap, v)?;
    if let Some(c) = cons_body(&shape) {
        let rest = cons_rest(heap, c, &v)?;
        return seq(heap, &rest);
    }
    match tail(heap, "next", &v)? {
        Tail::Shared(items, _) if items.len() == 0 => Ok(Value::Null),
        Tail::Shared(items, source) => Ok(form::list_sharing(heap, items, &source)?),
        Tail::Copied(items) => list_or_nil(heap, &items, copied_sequence_counted(heap, &v)),
    }
}

/// `cons`: a new list of `x` followed by the elements of `coll`; onto a
/// lazy or cons sequence, a cons sequence whose tail is `coll`, unrealized.
pub(crate) fn cons(heap: &Heap, x: &Value, coll: &Value) -> Result<Value> {
    let rest = match shape(heap, coll) {
        Shape::Nil => return Ok(form::new_list(heap, std::slice::from_ref(x))?),
        Shape::Form(FormKind::List | FormKind::Cons | FormKind::Lazy, _) => coll.clone(),
        _ => seq(heap, coll)?,
    };
    let items = PVector::try_from_values(heap, std::slice::from_ref(x))?;
    Ok(form::new_cons(
        heap,
        items,
        0,
        rest,
        lazy::ConsKind::Cons,
        None,
    )?)
}

/// The concatenation of `colls`, always a LazySeq, including empty and
/// entirely finite inputs.
fn concat_onto(heap: &Heap, colls: &[Value]) -> Result<Value> {
    // Variadic concat traverses every operand, including the final one.
    // A terminal nil keeps that behavior distinct from unary/binary concat,
    // whose final operand is returned without rebuilding its sequence.
    let colls = if colls.len() > 2 {
        let mut operands = colls.to_vec();
        operands.push(Value::Null);
        form::new_list(heap, &operands)?
    } else {
        form::new_list(heap, colls)?
    };
    Ok(form::new_lazy_concat(heap, colls)?)
}

/// Realize a lazy `concat` of the list `colls`: the elements of the first
/// collection's first segment (a list's or vector's, a cons sequence's), as
/// a cons sequence (chunked when that collection's seq is) whose tail is
/// the lazy `concat` of the rest of that collection and the others; `nil`
/// when all are empty. Realizes only the first collection with elements.
pub(crate) fn concat_step(heap: &Heap, colls: &Value) -> Result<Value> {
    let colls = sequential_items(heap, "concat", colls)?;
    for (i, coll) in colls.iter().enumerate() {
        if i + 1 == colls.len() {
            return Ok(coll.clone());
        }
        let (head, shape) = head(heap, coll)?;
        let (items, start, tail, chunked) = match &shape {
            Shape::Nil => continue,
            Shape::Form(_, f) => match f.body() {
                FormBody::List(items) => (items.clone(), 0, Value::Null, false),
                FormBody::Vector(items) => (items.clone(), 0, Value::Null, items.len() > CHUNK),
                FormBody::Cons(c) => (c.items.clone(), c.start, c.rest.clone(), c.chunked()),
                _ => {
                    let items = collection_items(heap, "concat", &head)?.unwrap_or_default();
                    (
                        PVector::try_from_values(heap, &items)?,
                        0,
                        Value::Null,
                        false,
                    )
                }
            },
            _ => {
                let items = collection_items(heap, "concat", &head)?.unwrap_or_default();
                (
                    PVector::try_from_values(heap, &items)?,
                    0,
                    Value::Null,
                    false,
                )
            }
        };
        if start >= items.len() {
            continue;
        }
        let mut rest: Vec<Value> = Vec::new();
        if !matches!(tail, Value::Null | Value::Undefined) {
            rest.push(tail);
        }
        rest.extend(colls[i + 1..].iter().cloned());
        let rest = if rest.is_empty() {
            Value::Null
        } else {
            let rest = form::new_list(heap, &rest)?;
            form::new_lazy_concat(heap, rest)?
        };
        let kind = if chunked {
            lazy::ConsKind::ChunkedSeq
        } else {
            lazy::ConsKind::Cons
        };
        return Ok(form::new_cons(heap, items, start, rest, kind, None)?);
    }
    Ok(Value::Null)
}

/// `concat` of two collections: a lazy sequence of the elements of `a`
/// followed by those of `b` (`nil` contributes none).
pub(crate) fn concat(heap: &Heap, a: &Value, b: &Value) -> Result<Value> {
    concat_onto(heap, &[a.clone(), b.clone()])
}

/// `concat` of any number of collections, given as the elements of the
/// list `colls`: a lazy sequence of all their elements, in order (`nil`
/// contributes none). The lowering of `(concat a b c …)`, flat however many
/// operands there are.
pub(crate) fn concat_all(heap: &Heap, colls: &Value) -> Result<Value> {
    let colls = sequential_items(heap, "concat", colls)?;
    concat_onto(heap, &colls)
}

/// The elements of a list or vector (the operand lists of the flat
/// lowerings).
fn sequential_items(heap: &Heap, op: &'static str, v: &Value) -> Result<Vec<Value>> {
    let shape = shape(heap, v);
    match &shape {
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => Ok(items.to_vec()),
            _ => Err(unsupported(op, &shape)),
        },
        _ => Err(unsupported(op, &shape)),
    }
}

/// A map of `keys[i]` to `values[i]` (two lists of equal length), as
/// `formMap` of the alternating pairs: a later equal key replaces the
/// value. A hash-map literal evaluates all its keys before its values and
/// is lowered to this.
pub(crate) fn zip_map(heap: &Heap, keys: &Value, values: &Value) -> Result<Value> {
    let keys = sequential_items(heap, "hash-map", keys)?;
    let values = sequential_items(heap, "hash-map", values)?;
    if keys.len() != values.len() {
        return Err(CljError::Operands(
            "hash-map keys and values differ in count",
        ));
    }
    let entries: Vec<(Value, Value)> = keys.into_iter().zip(values).collect();
    Ok(form::new_map(heap, &entries)?)
}

/// What a constructor operand is: `None` for `nil`, a string, or the name
/// of a keyword or symbol.
enum NamePart {
    Nil,
    Str(JsString),
    Named(FormKind, form::FormName),
    Other,
}

fn name_part(heap: &Heap, v: &Value) -> NamePart {
    match shape(heap, v) {
        Shape::Nil => NamePart::Nil,
        Shape::Str(s) => NamePart::Str(s),
        Shape::Form(kind @ (FormKind::Keyword | FormKind::Symbol), f) => match f.body() {
            FormBody::Keyword(n) | FormBody::Symbol(n) => NamePart::Named(kind, n.clone()),
            _ => NamePart::Other,
        },
        _ => NamePart::Other,
    }
}

/// The position of the first `/` in `s`.
fn slash(s: &JsString) -> Option<usize> {
    s.code_units().iter().position(|&u| u == u16::from(b'/'))
}

fn substring(heap: &Heap, s: &JsString, range: std::ops::Range<usize>) -> Result<JsString> {
    JsString::copy_units(heap, &s.code_units()[range])
        .ok_or(CljError::Collection(CollectionError::HeapLimit))
}

fn named(
    heap: &Heap,
    kind: FormKind,
    namespace: Option<JsString>,
    name: JsString,
) -> Result<Value> {
    Ok(form::new_named(heap, kind, namespace, name)?)
}

/// `(keyword x)`: a keyword itself (as given); a symbol's namespace and
/// name as a keyword; a string's keyword (`"ns/name"` splits at its one
/// `/`, `"/"` is the name `/`); `nil` for anything else. A spelling with
/// more than one `/` has no native keyword (`AmbiguousName`).
pub(crate) fn keyword(heap: &Heap, x: &Value) -> Result<Value> {
    match name_part(heap, x) {
        NamePart::Named(FormKind::Keyword, _) => Ok(x.clone()),
        NamePart::Named(_, n) => named(heap, FormKind::Keyword, n.namespace, n.name),
        NamePart::Str(s) => {
            let units = s.code_units();
            let slashes = units.iter().filter(|&&u| u == u16::from(b'/')).count();
            match slash(&s) {
                Some(at) if slashes == 1 && units.len() > 1 => {
                    let ns = substring(heap, &s, 0..at)?;
                    let name = substring(heap, &s, at + 1..units.len())?;
                    named(heap, FormKind::Keyword, Some(ns), name)
                }
                _ => named(heap, FormKind::Keyword, None, s),
            }
        }
        NamePart::Nil | NamePart::Other => Ok(Value::Null),
    }
}

/// `(keyword ns name)`: keyword or symbol operands give their names; then
/// `ns` must be `nil` or a string and `name` a string.
pub(crate) fn keyword_ns(heap: &Heap, ns: &Value, name: &Value) -> Result<Value> {
    const MESSAGE: &str = "keyword expects strings or keywords and an optional nil namespace";
    let text = |part: NamePart| match part {
        NamePart::Named(_, n) => Some(Some(n.name)),
        NamePart::Str(s) => Some(Some(s)),
        NamePart::Nil => Some(None),
        NamePart::Other => None,
    };
    let (Some(ns), Some(Some(name))) = (text(name_part(heap, ns)), text(name_part(heap, name)))
    else {
        return Err(CljError::Operands(MESSAGE));
    };
    named(heap, FormKind::Keyword, ns, name)
}

/// `(symbol x)`: a symbol itself (as given); a keyword's or string's
/// spelling split at its first `/` (none, or a leading one: no
/// namespace). Anything else fails as "no conversion to symbol".
pub(crate) fn symbol(heap: &Heap, x: &Value) -> Result<Value> {
    let s = match name_part(heap, x) {
        NamePart::Named(FormKind::Symbol, _) => return Ok(x.clone()),
        // A keyword converts through its spelling, as a string does: `:/x`
        // (namespace "", name "x") is the unqualified `/x`, which has no
        // native symbol.
        NamePart::Named(_, n) => {
            let mut units = Vec::new();
            if let Some(ns) = &n.namespace {
                units.extend_from_slice(ns.code_units());
                units.push(u16::from(b'/'));
            }
            units.extend_from_slice(n.name.code_units());
            JsString::copy_units(heap, &units)
                .ok_or(CljError::Collection(CollectionError::HeapLimit))?
        }
        NamePart::Str(s) => s,
        NamePart::Nil | NamePart::Other => {
            return Err(CljError::Operands("no conversion to symbol"));
        }
    };
    match slash(&s) {
        Some(at) if at >= 1 => {
            let ns = substring(heap, &s, 0..at)?;
            let name = substring(heap, &s, at + 1..s.len())?;
            named(heap, FormKind::Symbol, Some(ns), name)
        }
        _ => named(heap, FormKind::Symbol, None, s),
    }
}

/// `(symbol ns name)`: `ns` `nil` or a string, `name` a string.
pub(crate) fn symbol_ns(heap: &Heap, ns: &Value, name: &Value) -> Result<Value> {
    const MESSAGE: &str = "symbol expects strings and an optional nil namespace";
    let ns = match name_part(heap, ns) {
        NamePart::Nil => None,
        NamePart::Str(s) => Some(s),
        _ => return Err(CljError::Operands(MESSAGE)),
    };
    let NamePart::Str(name) = name_part(heap, name) else {
        return Err(CljError::Operands(MESSAGE));
    };
    named(heap, FormKind::Symbol, ns, name)
}

/// `nth`: the element at index `i` of a list, vector or string; for `nil`,
/// `default` (`nil` without one). Past the end: `default`, or `RangeError`
/// without one.
pub(crate) fn nth(heap: &Heap, coll: &Value, i: &Value, default: Option<&Value>) -> Result<Value> {
    let shape = shape(heap, coll);
    let index = index_from_value(&form::unwrap(heap, i.clone())).ok();
    if is_lazy_seq(heap, coll) {
        let found = match index {
            Some(index) => nth_of_sequence(heap, coll, index)?,
            None => None,
        };
        return match (found, default) {
            (Some(v), _) => Ok(v),
            (None, Some(d)) => Ok(d.clone()),
            (None, None) => Err(CljError::OutOfBounds),
        };
    }
    let found = match &shape {
        Shape::Nil => return Ok(default.cloned().unwrap_or(Value::Null)),
        Shape::Str(s) => match index {
            Some(index) => code_unit(heap, s, index)?,
            None => None,
        },
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => index.and_then(|i| items.get(i)),
            _ => return Err(unsupported("nth", &shape)),
        },
        Shape::Other(_) => return Err(unsupported("nth", &shape)),
    };
    match (found, default) {
        (Some(v), _) => Ok(v),
        (None, Some(d)) => Ok(d.clone()),
        (None, None) => Err(CljError::OutOfBounds),
    }
}

/// The element at `index` of a lazy or cons sequence, realizing only as far
/// as it.
fn nth_of_sequence(heap: &Heap, coll: &Value, index: usize) -> Result<Option<Value>> {
    let mut visited = lazy::Visited::default();
    let mut cur = coll.clone();
    let mut skip = index;
    loop {
        let head = form::realized_head(heap, cur, &mut visited)?;
        let shape = shape(heap, &head);
        if let Some(c) = cons_body(&shape) {
            if skip < c.len() {
                return Ok(c.items.get(c.start + skip));
            }
            skip -= c.len();
            cur = c.rest.clone();
            continue;
        }
        let items = collection_items(heap, "nth", &head)?.unwrap_or_default();
        return Ok(items.get(skip).cloned());
    }
}

/// `get`: a map value by an equal key, a set's stored member, a vector
/// element or a string's code unit by index; `default` otherwise,
/// including for `nil` and for values that are not collections.
pub(crate) fn get(heap: &Heap, coll: &Value, key: &Value, default: &Value) -> Result<Value> {
    Ok(match shape(heap, coll) {
        Shape::Form(_, f) => match f.body() {
            FormBody::Struct(s) if s.info.record => form::record_get(heap, s, key)?,
            FormBody::Map(_) | FormBody::Set(_) => {
                let coll = form::unwrap(heap, coll.clone());
                form::get(heap, &coll, key)?
            }
            FormBody::Vector(items) => index_from_value(&form::unwrap(heap, key.clone()))
                .ok()
                .and_then(|i| items.get(i)),
            _ => None,
        },
        Shape::Str(s) => match index_from_value(&form::unwrap(heap, key.clone())) {
            Ok(i) => code_unit(heap, &s, i)?,
            Err(_) => None,
        },
        Shape::Nil | Shape::Other(_) => None,
    }
    .unwrap_or_else(|| default.clone()))
}

/// Invoke a native callable collection. Functions are called by the VM-side
/// JavaScript adapter instead. A vector's call ignores a default and throws
/// outside its bounds, as the legacy runtime's fixed `call` method does.
/// A map entry accepts one index or an index and fallback. Preserve the
/// argument count so an explicit nil/undefined fallback is still supplied.
pub(crate) fn invoke(
    heap: &Heap,
    f: &Value,
    arg: &Value,
    default: &Value,
    argument_count: usize,
) -> Result<Value> {
    let shape = shape(heap, f);
    match &shape {
        Shape::Form(FormKind::Keyword | FormKind::Symbol, _) => get(heap, arg, f, default),
        Shape::Form(FormKind::Map | FormKind::Set, _) => get(heap, f, arg, default),
        Shape::Form(FormKind::Vector, vector) => {
            let FormBody::Vector(items) = vector.body() else {
                unreachable!("a vector shape has a vector body");
            };
            let index = form::unwrap(heap, arg.clone());
            if vector.is_map_entry() {
                if argument_count != 1 && argument_count != 2 {
                    return Err(CljError::Arity(argument_count));
                }
                // Entries require exact numeric 0/1, even for non-number
                // operands. A rejected index returns the supplied fallback
                // unchanged, including its occurrence and nil/undefined value.
                return match index {
                    Value::Number(0.0) => Ok(items.get(0).expect("entry key")),
                    Value::Number(1.0) => Ok(items.get(1).expect("entry value")),
                    _ if argument_count == 2 => Ok(default.clone()),
                    _ => Err(CljError::Operands("Index out of bounds")),
                };
            }
            let Value::Number(index) = index else {
                return Err(CljError::Operands("Key must be integer"));
            };
            // The legacy callable checks numeric bounds before its bitwise
            // indexing truncates a fractional index. Keep nth/get's index
            // contract separate, including rejection of negative fractions.
            if index >= 0.0 && index < items.len() as f64 {
                Ok(items.get(index as usize).expect("index within bounds"))
            } else {
                Err(CljError::Operands("Index out of bounds"))
            }
        }
        _ => Err(unsupported("invoke", &shape)),
    }
}

/// `contains?`: a map key, a set member, or a vector or string index.
pub(crate) fn contains(heap: &Heap, coll: &Value, key: &Value) -> Result<bool> {
    Ok(match shape(heap, coll) {
        Shape::Form(_, f) => match f.body() {
            FormBody::Struct(s) if s.info.record => form::record_get(heap, s, key)?.is_some(),
            FormBody::Map(_) | FormBody::Set(_) => {
                let coll = form::unwrap(heap, coll.clone());
                form::get(heap, &coll, key)?.is_some()
            }
            FormBody::Vector(items) => {
                index_from_value(&form::unwrap(heap, key.clone())).is_ok_and(|i| i < items.len())
            }
            _ => false,
        },
        Shape::Str(s) => {
            index_from_value(&form::unwrap(heap, key.clone())).is_ok_and(|i| i < s.len())
        }
        Shape::Nil | Shape::Other(_) => false,
    })
}

/// `coll` itself when `updated` is the value it wraps, else `updated`.
fn same_or(heap: &Heap, coll: &Value, updated: Value) -> Value {
    if form::unwrap(heap, coll.clone()).same_object(&updated) {
        coll.clone()
    } else {
        updated
    }
}

/// `assoc` of one key: a map entry (the key stored as given), or a vector
/// index `<= count`; `nil` becomes a one-entry map.
pub(crate) fn assoc(heap: &Heap, coll: &Value, key: &Value, value: &Value) -> Result<Value> {
    let shape = shape(heap, coll);
    let target = form::unwrap(heap, coll.clone());
    let updated = match &shape {
        Shape::Nil => return Ok(form::new_literal_map(heap, &[key.clone(), value.clone()])?),
        Shape::Form(FormKind::Map, _) => form::assoc(heap, &target, key.clone(), value.clone())?,
        Shape::Form(FormKind::Struct, f) if f.record().is_some() => {
            form::record_assoc(heap, &target, key.clone(), value.clone())?
        }
        Shape::Form(FormKind::Vector, _) => {
            let index = form::unwrap(heap, key.clone());
            form::assoc(heap, &target, index, value.clone())?
        }
        _ => return Err(unsupported("assoc", &shape)),
    };
    Ok(same_or(heap, coll, updated))
}

/// `dissoc` of one key from a map; `nil` stays `nil`.
pub(crate) fn dissoc(heap: &Heap, coll: &Value, key: &Value) -> Result<Value> {
    let shape = shape(heap, coll);
    match &shape {
        Shape::Nil => Ok(Value::Null),
        Shape::Form(FormKind::Map, _) => {
            let target = form::unwrap(heap, coll.clone());
            let updated = form::dissoc(heap, &target, key)?;
            Ok(same_or(heap, coll, updated))
        }
        Shape::Form(FormKind::Struct, f) if f.record().is_some() => {
            let target = form::unwrap(heap, coll.clone());
            let updated = form::record_dissoc(heap, &target, key)?;
            Ok(same_or(heap, coll, updated))
        }
        _ => Err(unsupported("dissoc", &shape)),
    }
}

/// `disj` of one member from a set; `nil` stays `nil`.
pub(crate) fn disj(heap: &Heap, coll: &Value, member: &Value) -> Result<Value> {
    let shape = shape(heap, coll);
    match &shape {
        Shape::Nil => Ok(Value::Null),
        Shape::Form(FormKind::Set, _) => {
            let target = form::unwrap(heap, coll.clone());
            let updated = form::disj(heap, &target, member)?;
            Ok(same_or(heap, coll, updated))
        }
        _ => Err(unsupported("disj", &shape)),
    }
}

/// `conj` of one value: a vector appends, a list prepends, a set adds, a
/// map adds a `[key value]` vector's entry or merges a map; `nil` becomes
/// the list `(x)`.
pub(crate) fn conj(heap: &Heap, coll: &Value, x: &Value) -> Result<Value> {
    // Onto a lazy or cons sequence: `cons`, as `LazySeq`'s `-conj` does.
    if is_lazy_seq(heap, coll) {
        return cons(heap, x, coll);
    }
    let shape = shape(heap, coll);
    if matches!(&shape, Shape::Form(FormKind::List, f) if !f.is_list()) {
        return cons(heap, x, coll);
    }
    let target = form::unwrap(heap, coll.clone());
    let updated = match &shape {
        Shape::Nil => return Ok(form::new_list(heap, std::slice::from_ref(x))?),
        Shape::Form(FormKind::List | FormKind::Vector | FormKind::Set, _) => {
            form::conj(heap, &target, x.clone())?
        }
        Shape::Form(FormKind::Map | FormKind::Struct, f)
            if f.kind() == FormKind::Map || f.record().is_some() =>
        {
            let put = |coll: &Value, k: Value, v: Value| {
                if f.kind() == FormKind::Map {
                    form::assoc(heap, coll, k, v)
                } else {
                    form::record_assoc(heap, coll, k, v)
                }
            };
            let item = form::unwrap(heap, x.clone());
            match form::form_object(heap, &item)
                .as_deref()
                .map(form::FormObject::body)
            {
                Some(FormBody::Vector(pair)) if pair.len() == 2 => put(
                    &target,
                    pair.get(0).expect("pair"),
                    pair.get(1).expect("pair"),
                )?,
                Some(FormBody::Map(_)) => {
                    let mut map = target.clone();
                    for (k, v) in form::entries(heap, &item)? {
                        map = put(&map, k, v)?;
                    }
                    map
                }
                Some(FormBody::Struct(s)) if s.info.record => {
                    let mut map = target.clone();
                    for (k, v) in form::record_entries(heap, s)? {
                        map = put(&map, k, v)?;
                    }
                    map
                }
                _ => return Err(unsupported("conj of a map entry", &self::shape(heap, x))),
            }
        }
        _ => return Err(unsupported("conj", &shape)),
    };
    Ok(same_or(heap, coll, updated))
}

/// The dispatch keys of the values that are not structs or JavaScript
/// objects, in [`dispatch_key`] order.
const KEY_NAMES: [&str; 12] = [
    "null",
    "boolean",
    "number",
    "string",
    "bigint",
    "js-symbol",
    "keyword",
    "symbol",
    "list",
    "vector",
    "map",
    "set",
];

fn key_string(index: usize) -> Value {
    static KEYS: std::sync::OnceLock<Vec<JsString>> = std::sync::OnceLock::new();
    let keys = KEYS.get_or_init(|| KEY_NAMES.iter().map(|k| JsString::from(*k)).collect());
    Value::String(keys[index].clone())
}

/// What a protocol table keys `v`'s implementation by (light-cljs-478,
/// "Structs" in [`crate::collections::form`]): a struct's type (its
/// identity), a struct type's `"type"`, a native form's kind name, a
/// primitive's (`"null"` for `nil`, `"boolean"`, `"number"`, `"string"`),
/// `"function"`, `"array"`; `None` for any other JavaScript object, whose
/// type the caller knows. Occurrences are transparent.
pub(crate) fn dispatch_key(heap: &Heap, v: &Value) -> Option<Value> {
    let v = form::unwrap(heap, v.clone());
    Some(match &v {
        Value::Null | Value::Undefined => key_string(0),
        Value::Bool(_) => key_string(1),
        Value::Number(_) => key_string(2),
        Value::String(_) => key_string(3),
        Value::BigInt(_) => key_string(4),
        Value::Symbol(_) => key_string(5),
        Value::Object(id) => {
            if let Some(f) = form::form_object(heap, &v) {
                return Some(match f.body() {
                    FormBody::Struct(s) => s.ty.clone(),
                    FormBody::Type(_) => Value::String(JsString::from("type")),
                    FormBody::Keyword(_) => key_string(6),
                    FormBody::Symbol(_) => key_string(7),
                    // `()` is `EmptyList`, apart from `List`.
                    FormBody::List(items) if items.len() == 0 => {
                        Value::String(JsString::from("empty-list"))
                    }
                    FormBody::List(_) if f.is_list() => key_string(8),
                    FormBody::List(_) => Value::String(JsString::from(if f.sequence_counted() {
                        "sequence"
                    } else {
                        "uncounted-sequence"
                    })),
                    FormBody::Vector(_) if f.is_map_entry() => {
                        Value::String(JsString::from("map-entry"))
                    }
                    FormBody::Vector(_) => key_string(9),
                    FormBody::Map(_) => key_string(10),
                    FormBody::Set(_) => key_string(11),
                    FormBody::Lazy(_) => Value::String(JsString::from("lazy-seq")),
                    FormBody::Cons(_) => Value::String(JsString::from(if f.is_list() {
                        "cons"
                    } else if !f.sequence_counted() {
                        "uncounted-sequence"
                    } else {
                        "sequence"
                    })),
                    FormBody::Reduced(_) => Value::String(JsString::from("reduced")),
                    FormBody::CompilerLiteral { .. } => return None,
                });
            }
            return heap
                .read(*id, |o| match o {
                    HeapObject::Function(_)
                    | HeapObject::BoundFunction(_)
                    | HeapObject::Builtin(_) => Some(Value::String(JsString::from("function"))),
                    HeapObject::Array(_) => Some(Value::String(JsString::from("array"))),
                    _ => None,
                })
                .flatten();
        }
    })
}

/// A protocol method's implementation for `v` in `table`, a SameValueZero
/// persistent map ([`crate::collections`]) from dispatch keys
/// ([`dispatch_key`]) to implementations, whose `"_"` entry is the
/// `default` one: the entry for `v`'s key, else the default, else `nil`.
/// `undefined` for a JavaScript object [`dispatch_key`] has no key for,
/// which the caller resolves. A lookup by identity and string: no user
/// equality, hashing or property access.
pub(crate) fn dispatch(heap: &Heap, table: &Value, v: &Value) -> Result<Value> {
    let Some(key) = dispatch_key(heap, v) else {
        return Ok(Value::Undefined);
    };
    if let Some(found) = super::get(heap, table.clone(), key)? {
        return Ok(found);
    }
    Ok(super::get(heap, table.clone(), Value::String(JsString::from("_")))?.unwrap_or(Value::Null))
}

/// The common task-local protocol lookup. A snapshot is the persistent vector
/// [version, frontier, table, declared-types, direct-struct-only?], or null
/// before the first build. Its version/frontier must match before any lookup.
/// The optional direct mode selects only a struct's own entry: record builtins
/// precede object/default extensions. Other values stay on the caller's slow
/// path, which knows the core collection and host-type rules.
/// `declared` is a SameValueZero map of
/// the struct types whose declarations this task has installed. An unknown
/// struct must return to JavaScript to demand its declaration before resolving
/// any extension bundles. Host objects likewise keep their property/Promise
/// resolution there. Arrays also need that Promise probe when the table has
/// a Promise extension: their mutable prototype can inherit Promise.prototype.
/// This leaf reads only persistent maps and native values;
/// it cannot call user code, allocate adapters, or retain a heap borrow.
pub(crate) fn protocol_lookup(
    heap: &Heap,
    snapshot: &Value,
    version: &Value,
    frontier: &Value,
    v: &Value,
) -> Result<Value> {
    if matches!(snapshot, Value::Null | Value::Undefined) {
        return Ok(Value::Undefined);
    }
    let snapshot = super::vector_body(heap, snapshot)?;
    if snapshot.get(0).as_ref() != Some(version) || snapshot.get(1).as_ref() != Some(frontier) {
        return Ok(Value::Undefined);
    }
    let Some(key) = dispatch_key(heap, v) else {
        return Ok(Value::Undefined);
    };
    let is_struct = matches!(key, Value::Object(_));
    let direct = snapshot.get(4) == Some(Value::Bool(true));
    if direct && !is_struct {
        return Ok(Value::Undefined);
    }
    let table = snapshot.get(2).ok_or(CollectionError::IndexOutOfBounds {
        index: 2,
        count: snapshot.len(),
    })?;
    if matches!(&key, Value::String(name) if name == "array")
        && super::get(heap, table.clone(), Value::String("js/Promise".into()))?.is_some()
    {
        return Ok(Value::Undefined);
    }
    let declared = snapshot.get(3).ok_or(CollectionError::IndexOutOfBounds {
        index: 3,
        count: snapshot.len(),
    })?;
    if is_struct && super::get(heap, declared, key.clone())?.is_none() {
        return Ok(Value::Undefined);
    }
    if let Some(found) = super::get(heap, table.clone(), key)? {
        return Ok(found);
    }
    if direct {
        return Ok(Value::Null);
    }
    if is_struct
        && let Some(found) = super::get(heap, table.clone(), Value::String("object".into()))?
    {
        return Ok(found);
    }
    Ok(super::get(heap, table.clone(), Value::String("_".into()))?.unwrap_or(Value::Null))
}

/// `meta`: a form's metadata map, or `nil`.
pub(crate) fn meta(heap: &Heap, v: &Value) -> Value {
    form::meta(heap, &form::unwrap(heap, v.clone())).unwrap_or(Value::Null)
}

/// `with-meta`: a symbol, list, vector, map or set with metadata `m` (a
/// form map or `nil`); `v` itself when its metadata is already `m`.
pub(crate) fn with_meta(heap: &Heap, v: &Value, m: &Value) -> Result<Value> {
    let target = form::unwrap(heap, v.clone());
    let m = form::unwrap(heap, m.clone());
    let updated = form::with_meta(heap, &target, &m)?;
    Ok(same_or(heap, v, updated))
}

// ---- Lazy and chunked sequences (light-cljs-479) ----

/// The elements of a chunk: ClojureScript's `ArrayChunk` size.
const CHUNK: usize = 32;

/// `(lazy-seq …)`: a new lazy sequence realized by calling `thunk`.
pub(crate) fn lazy_seq(heap: &Heap, thunk: &Value) -> Result<Value> {
    Ok(form::new_lazy(heap, thunk.clone())?)
}

/// `realized?` of a lazy sequence.
pub(crate) fn realized(heap: &Heap, v: &Value) -> Result<Value> {
    let shape = shape(heap, v);
    match &shape {
        Shape::Form(_, f) => match f.lazy_cell() {
            Some(cell) => Ok(Value::Bool(cell.realized().is_some())),
            None => Err(unsupported("realized?", &shape)),
        },
        _ => Err(unsupported("realized?", &shape)),
    }
}

/// Every element of `v`'s seq, realized, as a new list (`()` for none):
/// what the runtime's array helpers read.
pub(crate) fn to_list(heap: &Heap, v: &Value) -> Result<Value> {
    let items = seq_items(heap, "seq", v)?.unwrap_or_default();
    Ok(form::new_list(heap, &items)?)
}

/// `chunked-seq?`: a vector's seq (read in chunks of 32), a finite range or
/// a sequence `chunk-cons` built. Lists and lazy sequences are not.
pub(crate) fn chunked_seq(heap: &Heap, v: &Value) -> bool {
    match shape(heap, v) {
        Shape::Form(_, f) => match f.body() {
            FormBody::Cons(c) => c.chunked(),
            _ => false,
        },
        _ => false,
    }
}

/// Where the chunk of a chunked cons sequence that starts at its first
/// element ends: a `chunk-cons` chunk is all of its elements; a vector's
/// seq (or a range's) is read in the 32-element chunks of its trie's leaves.
fn chunk_end(c: &lazy::ConsBody) -> usize {
    match c.kind {
        lazy::ConsKind::ChunkedSeq => c.items.len().min((c.start / CHUNK + 1) * CHUNK),
        _ => c.items.len(),
    }
}

/// `chunk-first` of a chunked seq: its first chunk as a form vector.
pub(crate) fn chunk_first(heap: &Heap, v: &Value) -> Result<Value> {
    let shape = shape(heap, v);
    if let Shape::Form(_, f) = &shape {
        match f.body() {
            FormBody::Cons(c) if c.chunked() => {
                let chunk: Vec<Value> = (c.start..chunk_end(c))
                    .filter_map(|i| c.items.get(i))
                    .collect();
                return Ok(form::new_vector(heap, &chunk)?);
            }
            _ => {}
        }
    }
    Err(unsupported("chunk-first", &shape))
}

/// `chunk-rest` of a chunked seq: what follows its first chunk (`()` for
/// nothing), unrealized.
pub(crate) fn chunk_rest(heap: &Heap, v: &Value) -> Result<Value> {
    let shape = shape(heap, v);
    if let Shape::Form(_, f) = &shape {
        match f.body() {
            FormBody::Cons(c) if c.chunked() => {
                let end = chunk_end(c);
                if end < c.items.len() {
                    let source = form::unwrap(heap, v.clone());
                    return Ok(form::cons_sharing(
                        heap,
                        c.items.clone(),
                        end,
                        c.rest.clone(),
                        c.kind,
                        &source,
                    )?);
                }
                return match &c.rest {
                    Value::Null | Value::Undefined => Ok(form::new_list(heap, &[])?),
                    rest => Ok(rest.clone()),
                };
            }
            _ => {}
        }
    }
    Err(unsupported("chunk-rest", &shape))
}

/// `chunk-next`: the seq of `chunk-rest`.
pub(crate) fn chunk_next(heap: &Heap, v: &Value) -> Result<Value> {
    let rest = chunk_rest(heap, v)?;
    seq(heap, &rest)
}

/// `chunk-cons`: the elements of `chunk` (a vector a `chunk` call froze, or
/// a form vector or list) followed by `rest`, unrealized; `rest` itself for
/// an empty chunk. A `range` chunk (`range`) is a finite range's sequence,
/// which the runtime's `IntegerRange` stands for: chunked, without metadata.
pub(crate) fn chunk_cons(
    heap: &Heap,
    chunk: &Value,
    rest: &Value,
    range: bool,
    count: Option<lazy::RangeCount>,
) -> Result<Value> {
    let chunk = form::unwrap(heap, chunk.clone());
    let items = match &chunk {
        Value::Object(id) => heap
            .read(*id, |o| match o {
                HeapObject::Vector(items) => Some(items.clone()),
                HeapObject::Form(f) => match f.body() {
                    FormBody::List(items) | FormBody::Vector(items) => Some(items.clone()),
                    _ => None,
                },
                _ => None,
            })
            .flatten(),
        _ => None,
    };
    let Some(items) = items else {
        return Err(unsupported("chunk-cons", &shape(heap, &chunk)));
    };
    if items.len() == 0 {
        return Ok(rest.clone());
    }
    // The tail of a sequence cell is `nil`, a list, or a lazy or cons
    // sequence.
    let rest = match shape(heap, rest) {
        Shape::Nil => Value::Null,
        Shape::Form(FormKind::List | FormKind::Lazy | FormKind::Cons, _) => rest.clone(),
        _ => to_list(heap, rest)?,
    };
    let kind = if range {
        lazy::ConsKind::ChunkedSeq
    } else {
        lazy::ConsKind::Chunked
    };
    Ok(form::new_cons(heap, items, 0, rest, kind, count)?)
}

/// `v` with every lazy and cons sequence inside it (elements, keys,
/// values and metadata, at any depth) replaced by a list of its realized
/// elements, realizing them; parts without one keep their identity. A
/// macro's result is converted so before the bridge exports it, which
/// reads finite forms only. An explicit stack: no native recursion.
pub(crate) fn realize_all(heap: &Heap, v: &Value) -> Result<Value> {
    #[derive(Clone)]
    enum Rebuild {
        /// A lazy or cons sequence; whether it exports with metadata
        /// (`nil` when none was given), as the light-cljs runtime's
        /// `LazySeq` and `ChunkedCons` do and its `Cons` does not.
        Seq(bool),
        /// Another form (a symbol): only its metadata.
        Meta,
        List,
        Vector,
        Set(bool),
        Map(bool),
        SortedMap,
        CompilerLiteral(JsString),
    }
    enum Work {
        Enter(Value),
        Exit {
            id: crate::heap::ObjectId,
            original: Value,
            rebuild: Rebuild,
            children: Vec<Value>,
            meta: Option<Value>,
        },
    }
    // Only objects are ever replaced: a primitive result is its input.
    let kept = |a: &Value, b: &Value| !matches!(a, Value::Object(_)) || a.same_object(b);
    let mut memo: std::collections::HashMap<crate::heap::ObjectId, Value> =
        std::collections::HashMap::new();
    let mut work = vec![Work::Enter(v.clone())];
    let mut results: Vec<Value> = Vec::new();
    // Forms being converted: one met again inside itself is a lazy
    // sequence that contains itself.
    let mut active: std::collections::HashSet<crate::heap::ObjectId> =
        std::collections::HashSet::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Enter(v) => {
                let Value::Object(id) = v else {
                    results.push(v);
                    continue;
                };
                if let Some(done) = memo.get(&id) {
                    results.push(done.clone());
                    continue;
                }
                let Some(f) = form::form_object(heap, &v) else {
                    results.push(v);
                    continue;
                };
                let (rebuild, children) = match f.body() {
                    FormBody::Lazy(_) => (
                        Rebuild::Seq(true),
                        seq_items(heap, "seq", &v)?.unwrap_or_default(),
                    ),
                    FormBody::Cons(c) => (
                        Rebuild::Seq(c.has_metadata()),
                        seq_items(heap, "seq", &v)?.unwrap_or_default(),
                    ),
                    FormBody::List(items) => (Rebuild::List, items.to_vec()),
                    FormBody::Vector(items) => (Rebuild::Vector, items.to_vec()),
                    FormBody::CompilerLiteral { tag, payload, .. } => {
                        (Rebuild::CompilerLiteral(tag.clone()), vec![payload.clone()])
                    }
                    FormBody::Set(set) => {
                        let mut members = Vec::with_capacity(set.len());
                        set.for_each(|k, _| members.push(k.clone()));
                        (Rebuild::Set(set.positional()), members)
                    }
                    FormBody::Map(map) => (
                        if f.is_sorted() {
                            Rebuild::SortedMap
                        } else {
                            Rebuild::Map(map.positional())
                        },
                        map.to_vec().into_iter().flat_map(|(k, v)| [k, v]).collect(),
                    ),
                    _ if f.meta().is_some() => (Rebuild::Meta, Vec::new()),
                    _ => {
                        results.push(v);
                        continue;
                    }
                };
                if !active.insert(id) {
                    return Err(CljError::Infinite);
                }
                let meta = f.meta().cloned();
                // Popped last-first: the children in order, then the
                // metadata, then the rebuild.
                let enter_meta = meta.clone().map(Work::Enter);
                let enter_children: Vec<Work> =
                    children.iter().rev().cloned().map(Work::Enter).collect();
                work.push(Work::Exit {
                    id,
                    original: v,
                    rebuild,
                    children,
                    meta,
                });
                work.extend(enter_meta);
                work.extend(enter_children);
            }
            Work::Exit {
                id,
                original,
                rebuild,
                children,
                meta,
            } => {
                let new_meta = match meta {
                    Some(_) => results.pop(),
                    None => None,
                };
                active.remove(&id);
                let new_children = results.split_off(results.len() - children.len());
                let unchanged = !matches!(rebuild, Rebuild::Seq(_))
                    && meta
                        .as_ref()
                        .zip(new_meta.as_ref())
                        .is_none_or(|(a, b)| kept(a, b))
                    && children.iter().zip(&new_children).all(|(a, b)| kept(a, b));
                let result = if unchanged {
                    original
                } else {
                    let rebuilt = match rebuild {
                        Rebuild::Meta => original.clone(),
                        Rebuild::Seq(_) | Rebuild::List => form::new_list(heap, &new_children)?,
                        Rebuild::Vector => form::new_vector(heap, &new_children)?,
                        Rebuild::CompilerLiteral(ref tag) => form::new_compiler_literal(
                            heap,
                            &Value::String(tag.clone()),
                            &new_children[0],
                        )?,
                        Rebuild::Set(true) => form::new_literal_set(heap, &new_children)?,
                        Rebuild::Set(false) => form::new_set(heap, &new_children)?,
                        Rebuild::Map(true) => form::new_literal_map(heap, &new_children)?,
                        Rebuild::SortedMap => {
                            let pairs = form::new_vector(heap, &new_children)?;
                            form::sorted_map_from_sequence(heap, &pairs)?
                        }
                        Rebuild::Map(false) => {
                            let entries: Vec<(Value, Value)> = new_children
                                .as_chunks::<2>()
                                .0
                                .iter()
                                .map(|[k, v]| (k.clone(), v.clone()))
                                .collect();
                            form::new_map(heap, &entries)?
                        }
                    };
                    let explicit_nil = matches!(rebuild, Rebuild::Seq(true));
                    match new_meta.or(explicit_nil.then_some(Value::Null)) {
                        Some(m) => form::with_exported_meta(heap, &rebuilt, m)?,
                        None => rebuilt,
                    }
                };
                memo.insert(id, result.clone());
                results.push(result);
            }
        }
    }
    Ok(results.pop().unwrap_or(Value::Null))
}

/// A sequential view used by tests: the elements of a list or vector.
#[cfg(test)]
fn elements(heap: &Heap, v: &Value) -> Vec<Value> {
    match form::form_object(heap, &form::unwrap(heap, v.clone()))
        .as_deref()
        .map(form::FormObject::body)
    {
        Some(FormBody::List(items) | FormBody::Vector(items)) => items.to_vec(),
        Some(FormBody::Cons(_)) => seq_items(heap, "test sequence", v).unwrap().unwrap(),
        _ => panic!("not sequential"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collections::{Provenance, new_occurrence};
    use crate::heap::Scope;
    use std::sync::Arc;

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    #[test]
    fn nil_puns_and_occurrences_are_transparent() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        assert_eq!(count(&heap, &Value::Null).unwrap(), n(0.0));
        assert_eq!(first(&heap, &Value::Null).unwrap(), Value::Null);
        assert_eq!(seq(&heap, &Value::Null).unwrap(), Value::Null);
        assert!(!contains(&heap, &Value::Null, &n(0.0)).unwrap());
        let v = form::new_vector(&heap, &[n(1.0), n(2.0)]).unwrap();
        let occ = new_occurrence(&heap, v.clone(), Provenance::default()).unwrap();
        assert_eq!(count(&heap, &occ).unwrap(), n(2.0));
        assert_eq!(first(&heap, &occ).unwrap(), n(1.0));
        assert_eq!(elements(&heap, &rest(&heap, &occ).unwrap()), vec![n(2.0)]);
        assert_eq!(
            next(&heap, &rest(&heap, &occ).unwrap()).unwrap(),
            Value::Null
        );
        let index = new_occurrence(&heap, n(1.0), Provenance::default()).unwrap();
        assert_eq!(get(&heap, &occ, &index, &Value::Null).unwrap(), n(2.0));
        assert_eq!(nth(&heap, &occ, &index, None).unwrap(), n(2.0));
        assert!(matches!(
            nth(&heap, &occ, &n(5.0), None),
            Err(CljError::OutOfBounds)
        ));
        // An unchanged update returns the occurrence itself.
        assert!(
            assoc(&heap, &occ, &n(0.0), &n(1.0))
                .unwrap()
                .same_object(&occ)
        );
        let list = form::new_list(&heap, &[n(1.0)]).unwrap();
        let occ = new_occurrence(&heap, list, Provenance::default()).unwrap();
        assert!(seq(&heap, &occ).unwrap().same_object(&occ));
    }

    fn trie(heap: &Heap, v: &Value) -> Vec<usize> {
        match form::form_object(heap, &form::unwrap(heap, v.clone()))
            .as_deref()
            .map(form::FormObject::body)
        {
            Some(FormBody::List(items) | FormBody::Vector(items)) => items.node_addresses(),
            Some(FormBody::Cons(c)) => c.items.node_addresses(),
            _ => panic!("not sequential"),
        }
    }

    /// `seq`, `rest` and `next` of a vector or list share its trie and are
    /// lists equal to (and hashing like) a copy of the remaining elements;
    /// each allocates one list object, whatever the length.
    #[test]
    fn seq_rest_and_next_share_the_trie() {
        let heap = Arc::new(Heap::with_byte_limit(usize::MAX / 2));
        let _scope = Scope::new(&heap);
        let items: Vec<Value> = (0..1100).map(|i| n(f64::from(i))).collect();
        let v = form::new_vector(&heap, &items).unwrap();
        let occ = new_occurrence(&heap, v.clone(), Provenance::default()).unwrap();
        let shared = trie(&heap, &v);
        let mut s = seq(&heap, &occ).unwrap();
        let mut walked = Vec::new();
        while s != Value::Null {
            // A vector's seq is a chunked cons sequence over its trie
            // (light-cljs-479).
            assert_eq!(form::kind(&heap, &s), Some(FormKind::Cons));
            assert_eq!(trie(&heap, &s), shared);
            walked.push(first(&heap, &s).unwrap());
            s = next(&heap, &s).unwrap();
        }
        assert_eq!(walked, items);
        let mut r = occ.clone();
        for k in 1..=1100 {
            r = rest(&heap, &r).unwrap();
            if [1, 31, 32, 33, 1024, 1099, 1100].contains(&k) {
                let copy = form::new_list(&heap, &items[k..]).unwrap();
                assert!(form::equals(&heap, &r, &copy), "k = {k}");
                assert_eq!(form::hash(&heap, &r), form::hash(&heap, &copy), "k = {k}");
                assert_eq!(count(&heap, &r).unwrap(), n((1100 - k) as f64));
                assert_eq!(elements(&heap, &r), &items[k..]);
                assert_eq!(meta(&heap, &r), Value::Null);
                assert_eq!(
                    nth(&heap, &r, &n(0.0), Some(&Value::Null)).unwrap(),
                    items.get(k).cloned().unwrap_or(Value::Null)
                );
            }
        }
        assert_eq!(next(&heap, &r).unwrap(), Value::Null);
        assert_eq!(count(&heap, &rest(&heap, &r).unwrap()).unwrap(), n(0.0));
        // Updates of a shared tail see only its elements.
        let tail = rest(&heap, &rest(&heap, &v).unwrap()).unwrap();
        let more = conj(&heap, &tail, &n(-1.0)).unwrap();
        assert_eq!(elements(&heap, &more)[..2], [n(-1.0), n(2.0)]);
        assert_eq!(count(&heap, &more).unwrap(), n(1099.0));
        let joined = cons(&heap, &n(-1.0), &tail).unwrap();
        assert!(form::equals(&heap, &joined, &more));
        // A tail costs its list object only, however long the remainder.
        let budget = heap.byte_budget();
        let short = form::new_vector(&heap, &items[..2]).unwrap();
        let cost = |coll: &Value| {
            let before = budget.used();
            let tail = rest(&heap, coll).unwrap();
            (budget.used() - before, tail)
        };
        let (long_cost, _long) = cost(&v);
        let (short_cost, _short) = cost(&short);
        assert_eq!(long_cost, short_cost);
    }

    /// A shared tail traces its own elements only: once the vector it came
    /// from is gone, its hidden first element can be collected.
    #[test]
    fn a_shared_tail_keeps_its_elements_and_not_the_hidden_ones() {
        let heap = Arc::new(Heap::default());
        let (a, b, tail) = {
            let _scope = Scope::new(&heap);
            let a = form::new_list(&heap, &[n(1.0)]).unwrap();
            let b = form::new_list(&heap, &[n(2.0)]).unwrap();
            let v = form::new_vector(&heap, &[a.clone(), b.clone()]).unwrap();
            let tail = crate::heap::Rooted::new(&heap, next(&heap, &v).unwrap());
            (a, b, tail)
        };
        heap.collect().unwrap();
        let id = |v: &Value| match v {
            Value::Object(id) => *id,
            _ => panic!("not an object"),
        };
        assert!(!heap.is_live(id(&a)));
        assert!(heap.is_live(id(&b)));
        let _scope = Scope::new(&heap);
        let tail = tail.value_in(&heap).unwrap();
        assert!(first(&heap, &tail).unwrap().same_object(&b));
    }

    #[test]
    fn nth_nil_honors_default() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let missing =
            form::new_named(&heap, FormKind::Keyword, None, JsString::from("missing")).unwrap();
        let got = nth(&heap, &Value::Null, &n(0.0), Some(&missing)).unwrap();
        assert!(got.same_object(&missing));
        assert_eq!(
            nth(&heap, &Value::Undefined, &n(3.0), Some(&n(7.0))).unwrap(),
            n(7.0)
        );
        // Without a default, `(nth nil i)` is nil.
        assert_eq!(
            nth(&heap, &Value::Null, &n(0.0), None).unwrap(),
            Value::Null
        );
    }

    #[test]
    fn named_constructors_concat_all_and_zip_map() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let s = |t: &str| Value::String(JsString::from(t));
        let spelled = |v: &Value| form::name(&heap, v).unwrap().spelling();
        let occ = new_occurrence(&heap, s("a/b"), Provenance::default()).unwrap();
        let k = keyword(&heap, &occ).unwrap();
        assert_eq!(
            (form::kind(&heap, &k), spelled(&k)),
            (Some(FormKind::Keyword), "a/b".into())
        );
        assert!(keyword(&heap, &k).unwrap().same_object(&k));
        assert_eq!(keyword(&heap, &n(1.0)).unwrap(), Value::Null);
        assert_eq!(spelled(&keyword(&heap, &s("/")).unwrap()), "/");
        let sym = symbol(&heap, &k).unwrap();
        assert_eq!(
            (form::kind(&heap, &sym), spelled(&sym)),
            (Some(FormKind::Symbol), "a/b".into())
        );
        assert_eq!(spelled(&symbol(&heap, &s("p/q/r")).unwrap()), "p/q/r");
        // A keyword converts through its spelling: `:/x` spells the
        // unqualified `/x`, which has no native symbol.
        let slash_x = keyword_ns(&heap, &s(""), &s("x")).unwrap();
        assert!(matches!(
            symbol(&heap, &slash_x),
            Err(CljError::Collection(CollectionError::AmbiguousName))
        ));
        assert_eq!(spelled(&keyword_ns(&heap, &sym, &s("n")).unwrap()), "b/n");
        assert_eq!(
            spelled(&symbol_ns(&heap, &Value::Null, &s("n")).unwrap()),
            "n"
        );
        assert_eq!(
            symbol(&heap, &n(1.0)).unwrap_err().message(),
            "no conversion to symbol"
        );
        assert_eq!(
            symbol_ns(&heap, &k, &s("n")).unwrap_err().message(),
            "symbol expects strings and an optional nil namespace"
        );
        assert!(keyword_ns(&heap, &Value::Null, &n(1.0)).is_err());
        let a = form::new_vector(&heap, &[n(1.0)]).unwrap();
        let b = form::new_list(&heap, &[n(2.0), n(3.0)]).unwrap();
        let colls = form::new_list(&heap, &[a, Value::Null, b, s("x")]).unwrap();
        // Concat is a LazySeq even when every input is finite. Its element
        // assertion runs through VM realization in the forms integration test.
        let concatenated = concat_all(&heap, &colls).unwrap();
        assert_eq!(form::kind(&heap, &concatenated), Some(FormKind::Lazy));
        assert!(!is_list(&heap, &concatenated));
        let keys = form::new_list(&heap, &[n(1.0), n(1.0)]).unwrap();
        let values = form::new_list(&heap, &[n(2.0), n(3.0)]).unwrap();
        let m = zip_map(&heap, &keys, &values).unwrap();
        assert_eq!(count(&heap, &m).unwrap(), n(1.0));
        assert_eq!(get(&heap, &m, &n(1.0), &Value::Null).unwrap(), n(3.0));
    }

    #[test]
    fn sets_add_remove_and_test_members() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let set = form::new_set(&heap, &[n(1.0), n(2.0), n(1.0)]).unwrap();
        assert_eq!(count(&heap, &set).unwrap(), n(2.0));
        assert!(contains(&heap, &set, &n(2.0)).unwrap());
        assert!(conj(&heap, &set, &n(2.0)).unwrap().same_object(&set));
        let fewer = disj(&heap, &set, &n(1.0)).unwrap();
        assert!(!contains(&heap, &fewer, &n(1.0)).unwrap());
        assert!(disj(&heap, &fewer, &n(7.0)).unwrap().same_object(&fewer));
        let more = conj(&heap, &fewer, &n(3.0)).unwrap();
        let expected = form::new_set(&heap, &[n(3.0), n(2.0)]).unwrap();
        assert!(form::equals(&heap, &more, &expected));
        assert_eq!(form::hash(&heap, &more), form::hash(&heap, &expected));
        assert_eq!(get(&heap, &more, &n(3.0), &Value::Null).unwrap(), n(3.0));
        assert_eq!(get(&heap, &more, &n(4.0), &n(9.0)).unwrap(), n(9.0));
    }

    #[test]
    fn unsupported_kinds_name_the_operation() {
        let heap = Arc::new(Heap::default());
        let _scope = Scope::new(&heap);
        let e = seq(&heap, &n(1.0)).unwrap_err();
        assert_eq!(
            e.message(),
            "unsupported operation: native macro runtime seq on number"
        );
        let k = form::new_named(&heap, FormKind::Keyword, None, JsString::from("a")).unwrap();
        assert_eq!(
            count(&heap, &k).unwrap_err().message(),
            "unsupported operation: native macro runtime count on keyword"
        );
    }
}
