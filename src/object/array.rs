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
//!   [`array_index`]), as data properties, writable, enumerable and
//!   configurable unless `Object.defineProperty` gave them other attributes
//!   (clj-quickjs-119; kept in a sorted map of the exceptions, empty for a
//!   literal). `Object.freeze` (clj-quickjs-122) sets a `frozen` flag that
//!   makes the default non-writable and non-configurable instead, so a
//!   frozen array needs no entry per element:
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
//!   an own data property that is not enumerable, writable (until
//!   `Object.defineProperty` makes it read-only, clj-quickjs-119) and not
//!   configurable; it is not stored in `object`. Writing an index at or
//!   beyond `length` sets `length` to that index plus one; a read-only
//!   `length` rejects that write.
//!
//! Reads and writes take only the heap's object-table lock, as for
//! ordinary objects (`crate::object`).

use std::collections::BTreeMap;

use super::{
    DefineOutcome, DeleteOutcome, OrdinaryObject, Property, PropertyDescriptor, PropertyFlags,
    SetOutcome, validate_and_apply,
};
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
    /// The attributes of the elements whose attributes differ from the
    /// default ([`ArrayObject::default_flags`]: writable, enumerable and
    /// configurable, or [`FROZEN_ELEMENT`] once frozen), from
    /// `Object.defineProperty` (clj-quickjs-119).
    attributes: BTreeMap<u32, PropertyFlags>,
    /// Accessor elements keep an undefined presence slot in dense/sparse storage.
    accessors: BTreeMap<u32, (Value, Value)>,
    /// The `writable` attribute of `length` (`Object.defineProperty`).
    length_writable: bool,
    /// Set by `Object.freeze` (clj-quickjs-122): every element is then
    /// non-writable and non-configurable without a per-element entry, so
    /// freezing a large array allocates nothing. Never cleared: a frozen
    /// array is not extensible and its elements cannot be reconfigured.
    frozen: bool,
}

/// The attributes of an element of a frozen array that was enumerable
/// (every element not given other attributes by `Object.defineProperty`).
pub(crate) const FROZEN_ELEMENT: PropertyFlags = PropertyFlags {
    writable: false,
    enumerable: true,
    configurable: false,
};

impl ArrayObject {
    /// An empty array (length 0) with a null prototype, owned by `owner`.
    pub(crate) fn new(owner: Option<TaskId>) -> ArrayObject {
        ArrayObject {
            object: OrdinaryObject::new(owner),
            dense: Vec::new(),
            sparse: BTreeMap::new(),
            length: 0,
            attributes: BTreeMap::new(),
            accessors: BTreeMap::new(),
            length_writable: true,
            frozen: false,
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

    /// ArrayCreate(length): all indices are holes. Numeric lengths never
    /// allocate dense slots or sparse entries, even at 2^32 - 1.
    pub(crate) fn with_length(owner: TaskId, prototype: ObjectId, length: u32) -> ArrayObject {
        ArrayObject {
            length,
            ..Self::with_prototype(owner, prototype)
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
        if let Some((_, setter)) = self.accessors.get(&index) {
            return SetOutcome::Setter(setter.clone());
        }
        if self.get(index).is_none() {
            if self.object.prototype().is_some() {
                return SetOutcome::Inherited;
            }
            return self.add(index, value);
        }
        if !self.element_flags(index).writable {
            return SetOutcome::ReadOnly;
        }
        self.store(index, value);
        SetOutcome::Done
    }

    /// The end of OrdinarySet once the prototype chain allowed the write of
    /// an element this array does not have ([`SetOutcome::Inherited`], as
    /// [`OrdinaryObject::add`]): store it as a writable, enumerable,
    /// configurable element, extending `length`. Nothing changes if the
    /// array is not extensible (`NotExtensible`) or the index is at or
    /// beyond a read-only `length` (`ReadOnly`, ArrayDefineOwnProperty).
    pub(crate) fn add(&mut self, index: u32, value: Value) -> SetOutcome {
        debug_assert!(index <= MAX_INDEX && self.get(index).is_none());
        if index >= self.length && !self.length_writable {
            return SetOutcome::ReadOnly;
        }
        if !self.object.extensible() {
            return SetOutcome::NotExtensible;
        }
        self.store(index, value);
        SetOutcome::Done
    }

    /// The attributes of an element `attributes` has no entry for.
    fn default_flags(&self) -> PropertyFlags {
        if self.frozen {
            FROZEN_ELEMENT
        } else {
            PropertyFlags::DATA
        }
    }

    /// The attributes of a present element.
    fn element_flags(&self, index: u32) -> PropertyFlags {
        self.attributes
            .get(&index)
            .copied()
            .unwrap_or_else(|| self.default_flags())
    }

    /// SetIntegrityLevel(frozen) (`Object.freeze`, clj-quickjs-122): the
    /// ordinary part frozen ([`OrdinaryObject::freeze`]), `length` read-only
    /// and every element non-writable and non-configurable (`enumerable`
    /// kept).
    pub(crate) fn freeze(&mut self) {
        self.object.freeze();
        self.length_writable = false;
        self.frozen = true;
        self.attributes.retain(|_, flags| {
            flags.writable = false;
            flags.configurable = false;
            *flags != FROZEN_ELEMENT
        });
    }

    /// TestIntegrityLevel(frozen) (`Object.isFrozen`, clj-quickjs-122): the
    /// ordinary part frozen, `length` read-only and no element writable or
    /// configurable (also when `Object.defineProperty` made them so one by
    /// one).
    pub(crate) fn is_frozen(&self) -> bool {
        self.object.is_frozen()
            && !self.length_writable
            && (self.frozen
                || self.elements().all(|(i, _)| {
                    let flags = self.element_flags(i);
                    !flags.writable && !flags.configurable
                }))
    }

    /// The element `index` as a property (value and attributes), if present.
    pub(crate) fn element(&self, index: u32) -> Option<Property> {
        self.get(index).map(|value| Property {
            accessor: self.accessors.get(&index).cloned(),
            value: value.clone(),
            flags: self.element_flags(index),
        })
    }

    /// The present elements' indices with their `enumerable` attribute, in
    /// ascending index order (own-key enumeration, clj-quickjs-123,
    /// `crate::vm::keys`).
    pub(crate) fn element_keys(&self) -> impl Iterator<Item = (u32, bool)> + '_ {
        self.elements()
            .map(|(index, _)| (index, self.element_flags(index).enumerable))
    }

    /// `[[Delete]]` of the element `index` (OrdinaryDelete on an array
    /// index, clj-quickjs-124): a configurable element becomes a hole (a
    /// `None` slot in the dense part, or no sparse entry), so the storage
    /// invariant of the module docs holds and `length` is unchanged. An
    /// element that is not configurable stays.
    pub(crate) fn delete_element(&mut self, index: u32) -> DeleteOutcome {
        if self.get(index).is_none() {
            return DeleteOutcome::Absent;
        }
        if !self.element_flags(index).configurable {
            return DeleteOutcome::NonConfigurable;
        }
        match self.dense.get_mut(index as usize) {
            Some(slot) => *slot = None,
            None => {
                self.sparse.remove(&index);
            }
        }
        self.attributes.remove(&index);
        self.accessors.remove(&index);
        DeleteOutcome::Deleted
    }

    /// The `length` property: writable (unless made read-only), not
    /// enumerable, not configurable.
    pub(crate) fn length_property(&self) -> Property {
        Property {
            accessor: None,
            value: Value::Number(f64::from(self.length)),
            flags: PropertyFlags {
                writable: self.length_writable,
                enumerable: false,
                configurable: false,
            },
        }
    }

    /// True while `length` is writable.
    pub(crate) fn length_writable(&self) -> bool {
        self.length_writable
    }

    /// `[[DefineOwnProperty]]` of the element `index` (ArrayDefineOwnProperty
    /// for an array index, clj-quickjs-119): a new element at or beyond a
    /// read-only `length`, or on a non-extensible array, is rejected;
    /// otherwise [`validate_and_apply`] decides, and a new element extends
    /// `length`.
    pub(crate) fn define_element(
        &mut self,
        index: u32,
        desc: &PropertyDescriptor,
    ) -> DefineOutcome {
        debug_assert!(index <= MAX_INDEX);
        let current = self.element(index);
        if current.is_none() {
            if index >= self.length && !self.length_writable {
                return DefineOutcome::Rejected("the array's length is not writable");
            }
            if !self.object.extensible() {
                return DefineOutcome::Rejected("the object is not extensible");
            }
        }
        let property = match validate_and_apply(current.as_ref(), desc) {
            Ok(property) => property,
            Err(outcome) => return outcome,
        };
        self.store(index, property.value);
        if let Some(accessor) = property.accessor {
            self.accessors.insert(index, accessor);
        } else {
            self.accessors.remove(&index);
        }
        if property.flags == self.default_flags() {
            self.attributes.remove(&index);
        } else {
            self.attributes.insert(index, property.flags);
        }
        DefineOutcome::Done
    }

    /// ArraySetLength (clj-quickjs-154). The VM has already converted any
    /// value to a valid u32 number. Validate the attributes before deleting
    /// elements, and apply writable:false even when a deletion is blocked.
    pub(crate) fn define_length(&mut self, desc: &PropertyDescriptor) -> DefineOutcome {
        let current = self.length_property();
        let property = match validate_and_apply(Some(&current), desc) {
            Ok(property) => property,
            Err(outcome) => return outcome,
        };
        let Value::Number(length) = property.value else {
            unreachable!("array length must be normalized before definition");
        };
        debug_assert!(length >= 0.0 && length <= f64::from(u32::MAX) && length.fract() == 0.0);
        let length = length as u32;
        let outcome = if length < self.length {
            self.truncate(length)
        } else {
            self.length = length;
            DefineOutcome::Done
        };
        self.length_writable = property.flags.writable;
        outcome
    }

    /// Delete from the highest present index down. Sparse gaps never cost
    /// work; the atomic operation is bounded by stored elements/dense slots,
    /// not by the numeric length. Drop every removed value and attribute so
    /// neither the collector nor later growth can see stale elements.
    fn truncate(&mut self, length: u32) -> DefineOutcome {
        while let Some((&index, _)) = self.sparse.last_key_value() {
            if index < length {
                break;
            }
            if self.delete_element(index) == DeleteOutcome::NonConfigurable {
                self.length = index + 1;
                return DefineOutcome::Rejected("an array element is not configurable");
            }
        }
        let mut end = self.dense.len();
        while end > length as usize {
            let index = (end - 1) as u32;
            if self.delete_element(index) == DeleteOutcome::NonConfigurable {
                self.dense.truncate(end);
                self.length = index + 1;
                return DefineOutcome::Rejected("an array element is not configurable");
            }
            end -= 1;
        }
        self.dense.truncate(end);
        self.length = length;
        DefineOutcome::Done
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
        for (get, set) in self.accessors.values() {
            get.trace(visit);
            set.trace(visit);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn n(x: f64) -> Value {
        Value::Number(x)
    }

    #[test]
    fn constructor_length_allocates_no_elements() {
        let heap = crate::heap::Heap::default();
        let task = TaskId::from_raw(1);
        let prototype = crate::realm::intrinsics(&heap).unwrap().array_prototype;
        let array = ArrayObject::with_length(task, prototype, u32::MAX);
        assert_eq!(array.length(), u32::MAX);
        assert!(array.dense.is_empty());
        assert!(array.sparse.is_empty());
        assert_eq!(array.object().owner(), Some(task));
        assert_eq!(array.object().prototype(), Some(prototype));
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

    /// Freezing a large array allocates no per-element attributes, keeps
    /// the attributes `defineProperty` gave, and blocks every change.
    #[test]
    fn freezing_needs_no_per_element_attributes() {
        let mut a = ArrayObject::new(None);
        for i in 0..1000 {
            a.push(Some(n(f64::from(i))));
        }
        let hidden = PropertyDescriptor {
            enumerable: Some(false),
            ..PropertyDescriptor::default()
        };
        assert_eq!(a.define_element(3, &hidden), DefineOutcome::Done);
        assert!(!a.is_frozen());
        a.freeze();
        assert!(a.is_frozen());
        assert_eq!(a.attributes.len(), 1, "only the non-enumerable element");
        assert_eq!(a.element(0).unwrap().flags, FROZEN_ELEMENT);
        let three = a.element(3).unwrap().flags;
        assert!(!three.writable && !three.enumerable && !three.configurable);
        assert_eq!(a.set(0, n(-1.0)), SetOutcome::ReadOnly);
        assert_eq!(a.set(1000, n(-1.0)), SetOutcome::ReadOnly);
        assert!(!a.length_property().flags.writable);
        // A same-value define is accepted and adds no entry.
        let same = PropertyDescriptor {
            value: Some(n(0.0)),
            ..PropertyDescriptor::default()
        };
        assert_eq!(a.define_element(0, &same), DefineOutcome::Done);
        assert_eq!(a.attributes.len(), 1);
        assert_eq!(a.get(0), Some(&n(0.0)));
    }

    /// Deleting an element leaves a hole in either part, keeps `length`,
    /// drops its attributes and keeps the storage invariant.
    #[test]
    fn deleted_elements_become_holes() {
        let mut a = ArrayObject::new(None);
        for i in 0..3 {
            a.push(Some(n(f64::from(i))));
        }
        assert_eq!(a.set(MAX_INDEX, n(-1.0)), SetOutcome::Done);
        let hidden = PropertyDescriptor {
            enumerable: Some(false),
            ..PropertyDescriptor::default()
        };
        assert_eq!(a.define_element(1, &hidden), DefineOutcome::Done);
        assert_eq!(a.delete_element(1), DeleteOutcome::Deleted);
        assert_eq!(a.delete_element(1), DeleteOutcome::Absent);
        assert_eq!(a.delete_element(MAX_INDEX), DeleteOutcome::Deleted);
        assert_eq!(a.delete_element(7), DeleteOutcome::Absent);
        assert_eq!(a.length(), u32::MAX);
        assert!(a.attributes.is_empty());
        let order: Vec<u32> = a.elements().map(|(i, _)| i).collect();
        assert_eq!(order, vec![0, 2]);
        assert_eq!((a.dense.len(), a.sparse.len()), (3, 0));
        // Refilled, the element has the default attributes.
        assert_eq!(a.set(1, n(1.0)), SetOutcome::Done);
        assert_eq!(a.element(1).unwrap().flags, PropertyFlags::DATA);
        a.freeze();
        assert_eq!(a.delete_element(0), DeleteOutcome::NonConfigurable);
        assert_eq!(a.get(0), Some(&n(0.0)));
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
