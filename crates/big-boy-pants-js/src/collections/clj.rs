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
//! * **Sequences are lists.** `seq`, `rest`, `next` and `cons` return form
//!   lists (or `nil`): a vector's elements, a map's entries as two-element
//!   vectors and a set's members (both in their trie order), and a string's
//!   UTF-16 code units as one-unit strings. A nonempty list is its own
//!   `seq` (the same value, occurrence included). There is no lazy or
//!   chunked sequence: `rest` copies the remaining elements.
//! * **Unchanged updates return their input.** An `assoc` of an identical
//!   value, a `dissoc` or `disj` of an absent key, a `conj` of a present set
//!   member and `with-meta` of the same metadata return the operand itself
//!   (the occurrence, if it was one).
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
use super::{CollectionError, index_from_value};
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
}

impl From<CollectionError> for CljError {
    fn from(e: CollectionError) -> Self {
        CljError::Collection(e)
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
    Ok(form::new_vector(heap, &[key.clone(), value.clone()])?)
}

/// The elements `seq` of `v` would hold, or `None` for `nil`.
fn seq_items(heap: &Heap, op: &'static str, v: &Value) -> Result<Option<Vec<Value>>> {
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
            FormBody::Set(set) => {
                let mut members = Vec::with_capacity(set.len());
                set.for_each(|k, _| members.push(k.clone()));
                members
            }
            FormBody::Map(map) => {
                let mut entries = Vec::with_capacity(map.len());
                for (k, v) in map.to_vec() {
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
            FormBody::Keyword(_)
            | FormBody::Symbol(_)
            | FormBody::Struct(_)
            | FormBody::Type(_) => return Err(unsupported(op, &shape)),
        },
        Shape::Other(_) => return Err(unsupported(op, &shape)),
    }))
}

/// A new list of `items`, or `nil` when there are none.
fn list_or_nil(heap: &Heap, items: &[Value]) -> Result<Value> {
    if items.is_empty() {
        Ok(Value::Null)
    } else {
        Ok(form::new_list(heap, items)?)
    }
}

/// ClojureScript truth: everything but `nil` and `false`.
pub(crate) fn truth(heap: &Heap, v: &Value) -> bool {
    !matches!(
        form::unwrap(heap, v.clone()),
        Value::Null | Value::Undefined | Value::Bool(false)
    )
}

/// `count`: elements, entries, members or UTF-16 code units; `nil` is 0.
pub(crate) fn count(heap: &Heap, v: &Value) -> Result<Value> {
    let shape = shape(heap, v);
    let n = match &shape {
        Shape::Nil => 0,
        Shape::Str(s) => s.len(),
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => items.len(),
            FormBody::Map(map) | FormBody::Set(map) => map.len(),
            FormBody::Struct(s) if s.info.record => {
                s.info.fields.len() + s.ext.as_ref().map_or(0, |e| e.len())
            }
            FormBody::Keyword(_)
            | FormBody::Symbol(_)
            | FormBody::Struct(_)
            | FormBody::Type(_) => return Err(unsupported("count", &shape)),
        },
        Shape::Other(_) => return Err(unsupported("count", &shape)),
    };
    Ok(Value::Number(n as f64))
}

/// `seq`: `nil` when empty, a nonempty list itself, otherwise a new list.
pub(crate) fn seq(heap: &Heap, v: &Value) -> Result<Value> {
    if let Shape::Form(FormKind::List, f) = shape(heap, v)
        && let FormBody::List(items) = f.body()
    {
        return Ok(if items.len() == 0 {
            Value::Null
        } else {
            v.clone()
        });
    }
    match seq_items(heap, "seq", v)? {
        None => Ok(Value::Null),
        Some(items) => list_or_nil(heap, &items),
    }
}

/// `first`: the first element of the sequence, or `nil`.
pub(crate) fn first(heap: &Heap, v: &Value) -> Result<Value> {
    let shape = shape(heap, v);
    match &shape {
        Shape::Form(_, f) => match f.body() {
            FormBody::List(items) | FormBody::Vector(items) => {
                return Ok(items.get(0).unwrap_or(Value::Null));
            }
            FormBody::Set(_) | FormBody::Map(_) | FormBody::Struct(_) => {}
            FormBody::Keyword(_) | FormBody::Symbol(_) | FormBody::Type(_) => {
                return Err(unsupported("first", &shape));
            }
        },
        Shape::Str(s) => return Ok(code_unit(heap, s, 0)?.unwrap_or(Value::Null)),
        Shape::Nil => return Ok(Value::Null),
        Shape::Other(_) => return Err(unsupported("first", &shape)),
    }
    Ok(seq_items(heap, "first", v)?
        .and_then(|items| items.into_iter().next())
        .unwrap_or(Value::Null))
}

/// The elements after the first, as `rest` and `next` see them.
fn tail(heap: &Heap, op: &'static str, v: &Value) -> Result<Vec<Value>> {
    if let Shape::Form(_, f) = shape(heap, v)
        && let FormBody::List(items) | FormBody::Vector(items) = f.body()
    {
        let items = items.to_vec();
        return Ok(items.get(1..).map(<[Value]>::to_vec).unwrap_or_default());
    }
    Ok(seq_items(heap, op, v)?
        .map(|items| items.into_iter().skip(1).collect())
        .unwrap_or_default())
}

/// `rest`: the elements after the first, as a list (empty, never `nil`).
pub(crate) fn rest(heap: &Heap, v: &Value) -> Result<Value> {
    let items = tail(heap, "rest", v)?;
    Ok(form::new_list(heap, &items)?)
}

/// `next`: the elements after the first, as a list, or `nil` for none.
pub(crate) fn next(heap: &Heap, v: &Value) -> Result<Value> {
    let items = tail(heap, "next", v)?;
    list_or_nil(heap, &items)
}

/// `cons`: a new list of `x` followed by the elements of `coll`.
pub(crate) fn cons(heap: &Heap, x: &Value, coll: &Value) -> Result<Value> {
    let mut items = vec![x.clone()];
    items.extend(seq_items(heap, "cons", coll)?.unwrap_or_default());
    Ok(form::new_list(heap, &items)?)
}

/// `concat` of two collections: a new list of the elements of `a` and then
/// those of `b` (empty when both are; `nil` contributes none).
pub(crate) fn concat(heap: &Heap, a: &Value, b: &Value) -> Result<Value> {
    let mut items = seq_items(heap, "concat", a)?.unwrap_or_default();
    items.extend(seq_items(heap, "concat", b)?.unwrap_or_default());
    Ok(form::new_list(heap, &items)?)
}

/// `concat` of any number of collections, given as the elements of the
/// list `colls`: one new list of all their elements, in order (`nil`
/// contributes none). The lowering of `(concat a b c …)`, flat however many
/// operands there are.
pub(crate) fn concat_all(heap: &Heap, colls: &Value) -> Result<Value> {
    let mut items = Vec::new();
    for coll in sequential_items(heap, "concat", colls)? {
        items.extend(seq_items(heap, "concat", &coll)?.unwrap_or_default());
    }
    Ok(form::new_list(heap, &items)?)
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

/// `get`: a map value by an equal key, a set's stored member, a vector
/// element or a string's code unit by index; `default` otherwise,
/// including for `nil` and for values that are not collections.
pub(crate) fn get(heap: &Heap, coll: &Value, key: &Value, default: &Value) -> Result<Value> {
    Ok(match shape(heap, coll) {
        Shape::Form(_, f) => match f.body() {
            FormBody::Struct(s) if s.info.record => form::record_get(heap, s, key),
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

/// `contains?`: a map key, a set member, or a vector or string index.
pub(crate) fn contains(heap: &Heap, coll: &Value, key: &Value) -> Result<bool> {
    Ok(match shape(heap, coll) {
        Shape::Form(_, f) => match f.body() {
            FormBody::Struct(s) if s.info.record => form::record_get(heap, s, key).is_some(),
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
        Shape::Nil => return Ok(form::new_map(heap, &[(key.clone(), value.clone())])?),
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
    let shape = shape(heap, coll);
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
                Some(FormBody::Map(entries)) => {
                    let mut map = target.clone();
                    for (k, v) in entries.to_vec() {
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
                    FormBody::List(_) => key_string(8),
                    FormBody::Vector(_) => key_string(9),
                    FormBody::Map(_) => key_string(10),
                    FormBody::Set(_) => key_string(11),
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

/// A sequential view used by tests: the elements of a list or vector.
#[cfg(test)]
fn elements(heap: &Heap, v: &Value) -> Vec<Value> {
    match form::form_object(heap, &form::unwrap(heap, v.clone()))
        .as_deref()
        .map(form::FormObject::body)
    {
        Some(FormBody::List(items) | FormBody::Vector(items)) => items.to_vec(),
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
        assert_eq!(
            elements(&heap, &concat_all(&heap, &colls).unwrap()),
            vec![n(1.0), n(2.0), n(3.0), s("x")]
        );
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
