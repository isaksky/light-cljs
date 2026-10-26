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
//! `index out of bounds`. These are the diagnostics of the JavaScript
//! adapters they replace. Allocation is charged to the heap's byte budget
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
            FormBody::Keyword(_) | FormBody::Symbol(_) => return Err(unsupported(op, &shape)),
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
            FormBody::Keyword(_) | FormBody::Symbol(_) => return Err(unsupported("count", &shape)),
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
            FormBody::Set(_) | FormBody::Map(_) => {}
            FormBody::Keyword(_) | FormBody::Symbol(_) => return Err(unsupported("first", &shape)),
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

/// `nth`: the element at index `i` of a list, vector or string; `nil` for
/// `nil`. Past the end: `default`, or `RangeError` without one.
pub(crate) fn nth(heap: &Heap, coll: &Value, i: &Value, default: Option<&Value>) -> Result<Value> {
    let shape = shape(heap, coll);
    let index = index_from_value(&form::unwrap(heap, i.clone())).ok();
    let found = match &shape {
        Shape::Nil => return Ok(Value::Null),
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
        Shape::Form(FormKind::Map, _) => {
            let item = form::unwrap(heap, x.clone());
            match form::form_object(heap, &item)
                .as_deref()
                .map(form::FormObject::body)
            {
                Some(FormBody::Vector(pair)) if pair.len() == 2 => form::assoc(
                    heap,
                    &target,
                    pair.get(0).expect("pair"),
                    pair.get(1).expect("pair"),
                )?,
                Some(FormBody::Map(entries)) => {
                    let mut map = target.clone();
                    for (k, v) in entries.to_vec() {
                        map = form::assoc(heap, &map, k, v)?;
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
