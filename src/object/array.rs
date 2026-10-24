//! Array objects (clj-quickjs-118): task-owned exotic objects whose
//! array-index properties live in element storage beside an ordinary
//! object's named properties, with an automatically maintained `length`.
//!
//! The decision record (why task-owned element storage rather than the
//! persistent vector of [`crate::collections`], the alternatives and their
//! consequences) is `docs/architecture.md`, "Array objects".
//!
//! # Representation
//!
//! An array is [`HeapObject::Array`](crate::heap::HeapObject) holding an
//! [`ArrayObject`]:
//!
//! * **`object`**, an [`OrdinaryObject`]: the owner task, the
//!   `[[Prototype]]`, the `[[Extensible]]` flag and every own property whose
//!   key is not an array index (`a.name = 1`). Ownership and the
//!   foreign-task rules are exactly those of ordinary objects.
//! * **Elements**, the own properties whose key is an array index (the
//!   canonical numeric string of an integer `0 ..= 2^32 - 2`, see
//!   [`array_index`]), as writable, enumerable, configurable data
//!   properties (the only kind an array element can be until the property
//!   descriptor pellets):
//!   - a **dense** vector for indices `0 .. dense.len()`, where `None` is a
//!     hole (an absent property, not an `undefined` value);
//!   - a **sparse** ordered map for indices at or beyond `dense.len()`.
//!
//!   A write at index `i` goes to the dense vector when `i` is inside it, or
//!   when the sparse map is empty and `i` is at most [`MAX_DENSE_GAP`] past
//!   its end (the gap fills with holes); every other write goes to the
//!   sparse map. So the dense part only grows while the sparse part is
//!   empty, and every sparse index is at or beyond `dense.len()`: elements
//!   enumerate in ascending index order by reading the dense part, then the
//!   sparse part. A literal, or appending one index at a time, stays dense;
//!   `a[4294967294] = 1` on an empty array allocates one sparse entry.
//! * **`length`**, a `u32`: one more than the greatest index present, or
//!   more (holes at the end: `[ , ]` has length 1 and no elements). It is
//!   an own data property that is not enumerable, writable and not
//!   configurable; it is not stored in `object`. Writing an index at or
//!   beyond `length` sets `length` to that index plus one.
//!
//! Reads and writes take only the heap's object-table lock, as for
//! ordinary objects (`crate::object`).

use std::collections::BTreeMap;

use super::{OrdinaryObject, SetOutcome};
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::TaskId;
use crate::value::{JsString, Value};

/// The greatest array index, `2^32 - 2` (ECMAScript "array index"; QuickJS
/// `JS_ATOM_MAX_INT` covers the same canonical integer keys).
pub(crate) const MAX_INDEX: u32 = u32::MAX - 1;

/// How far past the end of the dense part a write may land and still grow
/// it (filling the gap with holes), while the sparse part is empty.
pub(crate) const MAX_DENSE_GAP: u32 = 1024;

/// The array index a property name denotes: its canonical numeric string
/// (no sign, no leading zero, no exponent or fraction) of an integer
/// `0 ..= 2^32 - 2`. `"01"`, `"1.0"`, `"-0"` and `"4294967295"` are
/// ordinary names.
pub(crate) fn array_index(name: &JsString) -> Option<u32> {
    let units = name.code_units();
    let (&first, rest) = units.split_first()?;
    if first == u16::from(b'0') {
        return rest.is_empty().then_some(0);
    }
    // At most 10 digits: 4294967294 has 10.
    if units.len() > 10 {
        return None;
    }
    let mut n: u64 = 0;
    for &u in units {
        let digit = u.checked_sub(u16::from(b'0')).filter(|d| *d <= 9)?;
        n = n * 10 + u64::from(digit);
    }
    u32::try_from(n).ok().filter(|&i| i <= MAX_INDEX)
}

/// The array index a number denotes as a property key: an integer
/// `0 ..= 2^32 - 2` (`-0` included, whose ToString is `"0"`).
pub(crate) fn number_index(n: f64) -> Option<u32> {
    if n >= 0.0 && n <= f64::from(MAX_INDEX) && n.fract() == 0.0 {
        Some(n as u32)
    } else {
        None
    }
}

/// Storage of an array object. See the module docs.
#[derive(Debug)]
pub(crate) struct ArrayObject {
    object: OrdinaryObject,
    dense: Vec<Option<Value>>,
    sparse: BTreeMap<u32, Value>,
    length: u32,
}

impl ArrayObject {
    /// An empty array (length 0) with a null prototype, owned by `owner`.
    pub(crate) fn new(owner: Option<TaskId>) -> ArrayObject {
        ArrayObject {
            object: OrdinaryObject::new(owner),
            dense: Vec::new(),
            sparse: BTreeMap::new(),
            length: 0,
        }
    }

    /// An empty array owned by `owner` inheriting from `prototype`
    /// (`Array.prototype` for an array literal: ArrayCreate).
    pub(crate) fn with_prototype(owner: TaskId, prototype: ObjectId) -> ArrayObject {
        ArrayObject {
            object: OrdinaryObject::with_prototype(owner, prototype),
            ..ArrayObject::new(Some(owner))
        }
    }

    /// The ordinary part: owner, prototype, extensibility and the own
    /// properties that are not elements.
    pub(crate) fn object(&self) -> &OrdinaryObject {
        &self.object
    }

    /// See [`Self::object`].
    pub(crate) fn object_mut(&mut self) -> &mut OrdinaryObject {
        &mut self.object
    }

    /// The `length` property.
    pub(crate) fn length(&self) -> u32 {
        self.length
    }

    /// The element at `index`, if present (`None` for a hole or beyond the
    /// end).
    pub(crate) fn get(&self, index: u32) -> Option<&Value> {
        match self.dense.get(index as usize) {
            Some(slot) => slot.as_ref(),
            None => self.sparse.get(&index),
        }
    }

    /// The elements present, in ascending index order.
    pub(crate) fn elements(&self) -> impl Iterator<Item = (u32, &Value)> {
        let dense = self
            .dense
            .iter()
            .enumerate()
            .filter_map(|(i, v)| v.as_ref().map(|v| (i as u32, v)));
        dense.chain(self.sparse.iter().map(|(&i, v)| (i, v)))
    }

    /// Store `value` at `index` (present or not), keeping the storage
    /// invariant of the module docs, and extend `length` past it.
    fn store(&mut self, index: u32, value: Value) {
        let end = self.dense.len();
        if let Some(slot) = self.dense.get_mut(index as usize) {
            *slot = Some(value);
        } else if self.sparse.is_empty() && index as usize - end <= MAX_DENSE_GAP as usize {
            self.dense.resize(index as usize, None);
            self.dense.push(Some(value));
        } else {
            self.sparse.insert(index, value);
        }
        if index >= self.length {
            // `index <= MAX_INDEX`, so this cannot overflow.
            self.length = index + 1;
        }
    }

    /// OrdinarySet of element `index` on this array as the receiver, for
    /// the cases that do not depend on the prototype chain (as
    /// [`OrdinaryObject::set`]): update a present element, or add one to an
    /// extensible array with a null prototype, extending `length`.
    pub(crate) fn set(&mut self, index: u32, value: Value) -> SetOutcome {
        debug_assert!(index <= MAX_INDEX);
        if self.get(index).is_none() {
            if self.object.prototype().is_some() {
                return SetOutcome::Inherited;
            }
            if !self.object.extensible() {
                return SetOutcome::NotExtensible;
            }
        }
        self.store(index, value);
        SetOutcome::Done
    }

    /// The end of OrdinarySet once the prototype chain allowed the write of
    /// an element this array does not have ([`SetOutcome::Inherited`], as
    /// [`OrdinaryObject::add`]): store it, extending `length`. False
    /// (nothing changed) if the array is not extensible.
    pub(crate) fn add(&mut self, index: u32, value: Value) -> bool {
        debug_assert!(index <= MAX_INDEX && self.get(index).is_none());
        if !self.object.extensible() {
            return false;
        }
        self.store(index, value);
        true
    }

    /// Append an element (`Some`) or a hole (`None`) at index `length`, as
    /// an array literal does (CreateDataProperty, then the literal's final
    /// `length`). False, with nothing changed, when `length` is already
    /// `2^32 - 1` (no further index exists).
    pub(crate) fn push(&mut self, element: Option<Value>) -> bool {
        let index = self.length;
        if index > MAX_INDEX {
            return false;
        }
        match element {
            Some(value) => self.store(index, value),
            None => {
                if self.sparse.is_empty() && self.dense.len() == index as usize {
                    self.dense.push(None);
                }
                self.length = index + 1;
            }
        }
        true
    }
}

/// The collector traces every element, then the ordinary part (named
/// property values and the prototype).
impl Trace for ArrayObject {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        for (_, value) in self.elements() {
            value.trace(visit);
        }
        self.object.trace(visit);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    #[test]
    fn array_indices_are_canonical_integers_below_two_to_the_32_minus_one() {
        for (text, index) in [
            ("0", Some(0)),
            ("7", Some(7)),
            ("4294967294", Some(MAX_INDEX)),
            ("4294967295", None),
            ("99999999999", None),
            ("01", None),
            ("00", None),
            ("-0", None),
            ("1.0", None),
            ("1e3", None),
            (" 1", None),
            ("", None),
            ("length", None),
        ] {
            assert_eq!(array_index(&JsString::from(text)), index, "{text:?}");
        }
        for (x, index) in [
            (0.0, Some(0)),
            (-0.0, Some(0)),
            (3.0, Some(3)),
            (4294967294.0, Some(MAX_INDEX)),
            (4294967295.0, None),
            (1.5, None),
            (-1.0, None),
            (f64::NAN, None),
            (f64::INFINITY, None),
        ] {
            assert_eq!(number_index(x), index, "{x}");
        }
    }

    #[test]
    fn writes_keep_length_and_choose_dense_or_sparse_storage() {
        let mut a = ArrayObject::new(None);
        assert!(a.push(Some(n(0.0))));
        assert!(a.push(None));
        assert!(a.push(Some(n(2.0))));
        assert!(a.push(None));
        assert_eq!(a.length(), 4);
        assert_eq!((a.dense.len(), a.sparse.len()), (4, 0));
        assert_eq!(a.get(1), None, "a hole is absent");
        assert_eq!(a.get(2), Some(&n(2.0)));
        // Within the gap limit the dense part grows over holes.
        assert_eq!(a.set(10, n(10.0)), SetOutcome::Done);
        assert_eq!((a.length(), a.dense.len()), (11, 11));
        // Far past the end: sparse, and later writes at the end stay sparse.
        assert_eq!(a.set(MAX_INDEX, n(-1.0)), SetOutcome::Done);
        assert_eq!(a.length(), u32::MAX);
        assert_eq!(a.set(11, n(11.0)), SetOutcome::Done);
        assert_eq!((a.dense.len(), a.sparse.len()), (11, 2));
        // Overwriting below the length changes nothing else.
        assert_eq!(a.set(1, n(1.0)), SetOutcome::Done);
        assert_eq!(a.length(), u32::MAX);
        let order: Vec<u32> = a.elements().map(|(i, _)| i).collect();
        assert_eq!(order, vec![0, 1, 2, 10, 11, MAX_INDEX]);
        // No index exists past 2^32 - 2.
        assert!(!a.push(Some(n(0.0))));
        assert_eq!(a.length(), u32::MAX);
    }

    #[test]
    fn absent_elements_defer_to_a_prototype() {
        let mut a = ArrayObject::new(None);
        a.push(Some(n(0.0)));
        a.object.set_prototype(Some(ObjectId::from_index(0)));
        assert_eq!(a.set(0, n(1.0)), SetOutcome::Done, "present: own write");
        assert_eq!(a.set(1, n(1.0)), SetOutcome::Inherited);
        assert_eq!(a.length(), 1);
    }
}
