//! Own-property key enumeration (clj-quickjs-123): ECMAScript
//! OrdinaryOwnPropertyKeys and the `Object.keys` /
//! `Object.getOwnPropertyNames` built-ins, after QuickJS's
//! `JS_GetOwnPropertyNamesInternal`, `num_keys_cmp`,
//! `JS_GetOwnPropertyNames2`, `js_object_keys` and
//! `js_object_getOwnPropertyNames` (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8); written fresh over this runtime's object model
//! (`crate::object`, `crate::object::array`).
//!
//! # Order
//!
//! [`own_property_keys`] lists an object's own string keys in the order of
//! OrdinaryOwnPropertyKeys:
//!
//! 1. every key that is an array index (the canonical numeric string of an
//!    integer `0 ..= 2^32 - 2`, [`crate::object::array::array_index`]) in
//!    ascending numeric order: an array's elements (holes are absent), and
//!    on an ordinary object the index-named properties wherever they were
//!    created (`{b: 1, 1: 2, 0: 3}` lists `0`, `1`, `b`). `"4294967295"`
//!    (`2^32 - 1`), `"01"` and `"1.5"` are not array indices;
//! 2. every other string key in property creation order (a redefinition
//!    keeps the first position). An array's `length` was created with the
//!    array, so it comes first among them;
//! 3. symbol keys (clj-quickjs-127) would follow in creation order; no key
//!    is a symbol yet, and a key without a name in the key table is
//!    skipped, so `Object.keys` and `Object.getOwnPropertyNames`, which
//!    never list symbols, stay correct once they exist.
//!
//! Each key carries its `enumerable` attribute (clj-quickjs-119), so
//! `Object.keys` and `for`-`in` (clj-quickjs-85, which reuses this order)
//! filter with [`KeyFilter::Enumerable`].
//!
//! # Which objects
//!
//! | Target | Keys |
//! |--------|------|
//! | ordinary object or array of the running task, embedder object, realm intrinsic | its own properties as above |
//! | `Object.prototype` | enumerable keys: none (all its properties are non-enumerable); all keys: `UnsupportedOperand`, because its `__proto__` accessor is applied by the property walk, not stored (clj-quickjs-261) |
//! | built-in function | its own `length`, `name`, `prototype` and methods |
//! | function object (clj-quickjs-125) | enumerable keys: the enumerable properties of its property record (none without one); all keys: `UnsupportedOperand` until functions have their own `length` and `name` (clj-quickjs-101) |
//! | object or array of another task | `ForeignObject` |
//! | error object | `UnsupportedOperand` (its properties are clj-quickjs-230) |
//! | persistent vector, map, occurrence, builder, atom, binding cell, other native payload | none (no JavaScript properties) |
//!
//! The built-ins apply ToObject first: `undefined` and `null` throw a
//! catchable `TypeError`, other primitives fail with `UnsupportedOperand`
//! until wrapper objects exist (clj-quickjs-72). Each returns a fresh array
//! owned by the running task, inheriting from `Array.prototype`, whose
//! elements are the key strings.
//!
//! # Locks
//!
//! The target is read under one object-table lock, collecting interned
//! keys; their names are looked up in the property-key table after that
//! lock is released (the key table's lock is a leaf, `crate::object`). No
//! JavaScript runs and nothing is allocated before the result array, which
//! the built-ins allocate last.
//!
//! # `for`-`in`
//!
//! A `for`-`in` loop (clj-quickjs-85) enumerates through a
//! [`ForInIterator`], after QuickJS's `build_for_in_iterator` and
//! `js_for_in_next` (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8),
//! own properties only (the prototype walk is clj-quickjs-86).
//! `ForInStart` snapshots the operand's own enumerable string keys with
//! [`own_property_keys`] (an object's, including a function's property
//! record), or a string's indices; `null`, `undefined`, numbers and
//! booleans (whose wrappers have no enumerable own keys) give none. Each
//! `ForInNext` re-checks the next key with
//! [`property::get_own_property`] and skips it unless it is still an own
//! enumerable property (ECMAScript's informative EnumerateObjectProperties
//! re-reads the descriptor before each key; QuickJS checks existence only),
//! so a key deleted before it is reached is not visited; keys added during
//! the loop are not visited. Scanning past skipped keys is bounded by the
//! snapshot and allocates nothing.
//!
//! The enumerator is a [`HeapObject::Native`] payload: task-owned
//! (`Shareability::TaskOwned`), traced (its only edge is the enumerated
//! object), and held only by a scratch local of the loop's frame
//! (`crate::frontend` lowers the loop), so it lives exactly as long as the
//! frame needs it, survives yields, waits, collections and migration with
//! the frame, and is never reachable from JavaScript or published.

use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, NativePayload, ObjectId};
use crate::ids::TaskId;
use crate::object::PropertyKey;
use crate::object::array::{ArrayObject, array_index};
use crate::realm;
use crate::value::share::Shareability;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::exception::ErrorObject;
use crate::vm::function;
use crate::vm::property::{self, Key};

/// Which own keys [`own_property_keys`] lists.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum KeyFilter {
    /// Only enumerable properties (`Object.keys`, `for`-`in`).
    Enumerable,
    /// Every string key (`Object.getOwnPropertyNames`).
    All,
}

/// One own property key, in OrdinaryOwnPropertyKeys order.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct OwnKey {
    /// An array index ([`Key::Index`]) or any other name.
    pub(crate) key: Key,
    /// The property's `enumerable` attribute.
    pub(crate) enumerable: bool,
}

impl OwnKey {
    /// The key as a property name (an index as its decimal digits).
    pub(crate) fn name(&self) -> JsString {
        match &self.key {
            Key::Index(index) => JsString::from(index.to_string().as_str()),
            Key::Name(name) => name.clone(),
        }
    }
}

/// What the object-table read collected: element indices and interned
/// keys, each with its `enumerable` attribute.
#[derive(Default)]
struct Raw {
    elements: Vec<(u32, bool)>,
    /// An array's `length`, which is not in its key store.
    length: bool,
    named: Vec<(PropertyKey, bool)>,
}

/// The own string keys of `target` as `task` sees them, in
/// OrdinaryOwnPropertyKeys order (module docs, "Order"), restricted by
/// `filter`; or the failure of the module docs' table. The shared key
/// enumeration of `Object.keys`, `Object.getOwnPropertyNames` and (later)
/// `for`-`in` and object spread/rest copies.
pub(crate) fn own_property_keys(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    filter: KeyFilter,
) -> Result<Vec<OwnKey>, Fault> {
    let mut store = target;
    let function = heap
        .read(target, |o| matches!(o, HeapObject::Function(_)))
        .ok_or(Fault::Internal("key enumeration target was reclaimed"))?;
    if function {
        if filter == KeyFilter::All {
            return Err(Fault::Unsupported(
                "the own property names of a function need its own length and name, which are not supported"
                    .to_string(),
            ));
        }
        match function::property_record(heap, target)? {
            Some(record) => store = record,
            None => return Ok(Vec::new()),
        }
    } else if filter == KeyFilter::All
        && heap
            .realm()
            .get()
            .is_some_and(|r| r.object_prototype == target)
    {
        return Err(Fault::Unsupported(
            "the own property names of Object.prototype include its __proto__ accessor, which is not stored"
                .to_string(),
        ));
    }
    let raw = heap
        .read(store, |object| collect(object, task))
        .unwrap_or(Err(Fault::Internal("key enumeration target was reclaimed")))?;
    Ok(order(heap, raw, filter))
}

/// Collect the keys of one object under the object-table lock (no key-table
/// lookups here).
fn collect(object: &HeapObject, task: TaskId) -> Result<Raw, Fault> {
    let readable = |owner: Option<TaskId>| match owner {
        Some(owner) if owner != task => Err(Fault::ForeignObject {
            owner: Some(owner),
            caller: task,
        }),
        _ => Ok(()),
    };
    let named = |o: &crate::object::OrdinaryObject| {
        o.properties()
            .map(|(k, p)| (k, p.flags.enumerable))
            .collect()
    };
    match object {
        HeapObject::Plain(o) => {
            readable(o.owner())?;
            Ok(Raw {
                named: named(o),
                ..Raw::default()
            })
        }
        HeapObject::Builtin(f) => Ok(Raw {
            named: named(&f.properties),
            ..Raw::default()
        }),
        HeapObject::Array(a) => {
            readable(a.object().owner())?;
            Ok(Raw {
                elements: a.element_keys().collect(),
                length: true,
                named: named(a.object()),
            })
        }
        HeapObject::Native(payload) => {
            let any: &dyn std::any::Any = &**payload;
            if any.is::<ErrorObject>() {
                return Err(Fault::Unsupported(
                    "the own keys of an error object are not supported".to_string(),
                ));
            }
            Ok(Raw::default())
        }
        HeapObject::Function(_) => Err(Fault::Internal(
            "a function's keys are read from its property record",
        )),
        HeapObject::Vector(_)
        | HeapObject::Map(_)
        | HeapObject::Occurrence(_)
        | HeapObject::Builder(_)
        | HeapObject::Cell(_)
        | HeapObject::Atom(_) => Ok(Raw::default()),
    }
}

/// Name the interned keys (outside the object lock) and put everything in
/// OrdinaryOwnPropertyKeys order.
fn order(heap: &Heap, raw: Raw, filter: KeyFilter) -> Vec<OwnKey> {
    let wanted = |enumerable: bool| filter == KeyFilter::All || enumerable;
    let mut indices: Vec<(u32, bool)> = raw.elements;
    let mut names: Vec<OwnKey> = Vec::new();
    if raw.length && wanted(false) {
        names.push(OwnKey {
            key: Key::Name(JsString::from("length")),
            enumerable: false,
        });
    }
    for (key, enumerable) in raw.named {
        // A key without a name is a symbol (clj-quickjs-127): never listed.
        let Some(name) = heap.keys().name(key) else {
            continue;
        };
        match array_index(&name) {
            Some(index) => indices.push((index, enumerable)),
            None if wanted(enumerable) => names.push(OwnKey {
                key: Key::Name(name),
                enumerable,
            }),
            None => {}
        }
    }
    // An array's elements are already ascending; an ordinary object's index
    // names are in creation order. Indices are unique, so an unstable sort
    // is exact.
    indices.sort_unstable_by_key(|&(index, _)| index);
    indices
        .into_iter()
        .filter(|&(_, enumerable)| wanted(enumerable))
        .map(|(index, enumerable)| OwnKey {
            key: Key::Index(index),
            enumerable,
        })
        .chain(names)
        .collect()
}

/// `Object.keys(o)` (`filter` [`KeyFilter::Enumerable`]) and
/// `Object.getOwnPropertyNames(o)` ([`KeyFilter::All`]): ToObject, then a
/// fresh array of the key names (module docs).
pub(crate) fn object_keys(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
    filter: KeyFilter,
) -> Result<Value, Fault> {
    let target = args.first().cloned().unwrap_or(Value::Undefined);
    let id = match target {
        Value::Object(id) => id,
        Value::Undefined | Value::Null => {
            return Err(Fault::TypeError(format!(
                "cannot convert {} to object",
                target.type_name()
            )));
        }
        v => {
            let what = match filter {
                KeyFilter::Enumerable => "Object.keys",
                KeyFilter::All => "Object.getOwnPropertyNames",
            };
            return Err(Fault::Unsupported(format!(
                "{what} of a {} needs a wrapper object, which is not supported",
                v.type_name()
            )));
        }
    };
    let keys = own_property_keys(heap, task, id, filter)?;
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .array_prototype;
    let mut array = ArrayObject::with_prototype(task, prototype);
    for key in &keys {
        if !array.push(Some(Value::String(key.name()))) {
            return Err(Fault::Internal("more than 2^32 - 1 own keys"));
        }
    }
    heap.alloc(HeapObject::Array(Box::new(array)))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// The task-owned enumerator of a `for`-`in` loop (clj-quickjs-85; QuickJS
/// `JSForInIterator`, `build_for_in_iterator`, `js_for_in_next`): a
/// snapshot of the object's own enumerable string keys taken when the loop
/// starts, the position of the next one, and the object. It lives in a
/// heap object ([`HeapObject::Native`]) held only by a scratch local of the
/// loop's frame, so it is traced through that frame, survives yields,
/// waits, collections and migration with it, and is never reachable from
/// JavaScript (module docs, "`for`-`in`").
#[derive(Debug)]
pub(crate) struct ForInIterator {
    /// The enumerated object; `None` for a primitive operand, whose keys
    /// never change.
    object: Option<ObjectId>,
    keys: Vec<Key>,
    next: usize,
}

impl Trace for ForInIterator {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        if let Some(object) = self.object {
            visit(object);
        }
    }
}

impl NativePayload for ForInIterator {
    fn shareability(&self) -> Shareability {
        Shareability::TaskOwned
    }
}

/// `ForInStart`: the enumerator of `value` for the running task `task`, a
/// new heap object (module docs, "`for`-`in`").
pub(crate) fn for_in_start(heap: &Heap, task: TaskId, value: &Value) -> Result<Value, Fault> {
    let (object, keys) = match value {
        Value::Object(id) => {
            let keys = own_property_keys(heap, task, *id, KeyFilter::Enumerable)?;
            (Some(*id), keys.into_iter().map(|k| k.key).collect())
        }
        // A String wrapper's own enumerable keys are its indices.
        Value::String(s) => {
            let len = u32::try_from(s.len())
                .map_err(|_| Fault::Internal("a string longer than 2^32 - 1 code units"))?;
            (None, (0..len).map(Key::Index).collect())
        }
        // `null` and `undefined` run no iteration; a Number or Boolean
        // wrapper has no own enumerable keys.
        Value::Undefined | Value::Null | Value::Bool(_) | Value::Number(_) => (None, Vec::new()),
    };
    let iterator = ForInIterator {
        object,
        keys,
        next: 0,
    };
    heap.alloc(HeapObject::Native(Box::new(iterator)))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

/// `ForInNext`: the next key of the enumerator `iterator` that is still an
/// own enumerable property of its object (a key deleted, or made
/// non-enumerable, before it is reached is skipped), as a string; `None`
/// when the keys are exhausted.
pub(crate) fn for_in_next(
    heap: &Heap,
    task: TaskId,
    iterator: &Value,
) -> Result<Option<JsString>, Fault> {
    let Value::Object(id) = *iterator else {
        return Err(Fault::Internal("for-in enumerator is not an object"));
    };
    loop {
        // Take the next key under the object lock, then look it up without
        // it (the lookup takes the lock again).
        let next = heap
            .with_object_mut(id, |o| match o {
                HeapObject::Native(payload) => {
                    let any: &mut dyn std::any::Any = &mut **payload;
                    let it = any.downcast_mut::<ForInIterator>()?;
                    let Some(key) = it.keys.get(it.next).cloned() else {
                        return Some(None);
                    };
                    it.next += 1;
                    Some(Some((it.object, key)))
                }
                _ => None,
            })
            .flatten()
            .ok_or(Fault::Internal("for-in enumerator is not an enumerator"))?;
        let Some((object, key)) = next else {
            return Ok(None);
        };
        let present = match object {
            Some(object) => property::get_own_property(heap, task, object, &key)?
                .is_some_and(|p| p.flags.enumerable),
            None => true,
        };
        if present {
            return Ok(Some(match key {
                Key::Index(index) => JsString::from(index.to_string().as_str()),
                Key::Name(name) => name,
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::OrdinaryObject;

    fn names(keys: &[OwnKey]) -> Vec<String> {
        keys.iter().map(|k| k.name().to_string()).collect()
    }

    fn ok<T>(result: Result<T, Fault>) -> T {
        result.unwrap_or_else(|_| panic!("unexpected fault"))
    }

    /// Index-named properties of an ordinary object sort ascending ahead of
    /// the other names, above the key-index threshold too, and the key
    /// carries the index form.
    #[test]
    fn ordinary_keys_sort_indices_first() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let mut o = OrdinaryObject::new(Some(task));
        let order = ["z", "10", "a", "2", "4294967295", "01", "0", "b", "c", "d"];
        for name in order {
            let key = heap.keys().intern(&JsString::from(name)).unwrap();
            assert!(o.define(key, Value::Null));
        }
        let id = heap.alloc(HeapObject::Plain(o)).unwrap();
        let keys = ok(own_property_keys(&heap, task, id, KeyFilter::All));
        assert_eq!(
            names(&keys),
            ["0", "2", "10", "z", "a", "4294967295", "01", "b", "c", "d"]
        );
        assert_eq!(keys[2].key, Key::Index(10));
        assert_eq!(keys[5].key, Key::Name(JsString::from("4294967295")));
        // Another task may not enumerate it.
        assert!(matches!(
            own_property_keys(&heap, TaskId::from_raw(2), id, KeyFilter::Enumerable),
            Err(Fault::ForeignObject { .. })
        ));
    }

    /// A `for`-`in` enumerator (clj-quickjs-85) snapshots the enumerable
    /// keys, reports its object as its only edge, skips a key that is no
    /// longer an own enumerable property, and stays exhausted; another
    /// task's object cannot be enumerated.
    #[test]
    fn for_in_enumerator_rechecks_keys_and_traces_its_object() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let mut o = OrdinaryObject::new(Some(task));
        for name in ["b", "1", "a"] {
            let key = heap.keys().intern(&JsString::from(name)).unwrap();
            assert!(o.define(key, Value::Null));
        }
        let id = heap.alloc(HeapObject::Plain(o)).unwrap();
        let it = ok(for_in_start(&heap, task, &Value::Object(id)));
        let Value::Object(it_id) = it else {
            panic!("not an object")
        };
        let mut edges = Vec::new();
        heap.read(it_id, |o| o.trace(&mut |e| edges.push(e)))
            .unwrap();
        assert_eq!(edges, [id]);
        let next = |heap: &Heap| ok(for_in_next(heap, task, &it)).map(|s| s.to_string());
        assert_eq!(next(&heap).as_deref(), Some("1"));
        // `b` becomes non-enumerable before it is reached.
        let b = heap.keys().intern(&JsString::from("b")).unwrap();
        heap.with_object_mut(id, |o| match o {
            HeapObject::Plain(o) => {
                let flags = crate::object::PropertyFlags {
                    writable: true,
                    enumerable: false,
                    configurable: true,
                };
                assert!(o.define_with(b, Value::Null, flags));
            }
            _ => unreachable!(),
        })
        .unwrap();
        assert_eq!(next(&heap).as_deref(), Some("a"));
        assert_eq!(next(&heap), None);
        assert_eq!(next(&heap), None);
        assert!(matches!(
            for_in_start(&heap, TaskId::from_raw(2), &Value::Object(id)),
            Err(Fault::ForeignObject { .. })
        ));
        // A string's indices; no keys for other primitives.
        let s = ok(for_in_start(
            &heap,
            task,
            &Value::String(JsString::from("xy")),
        ));
        assert_eq!(ok(for_in_next(&heap, task, &s)).unwrap().to_string(), "0");
        assert_eq!(ok(for_in_next(&heap, task, &s)).unwrap().to_string(), "1");
        assert!(ok(for_in_next(&heap, task, &s)).is_none());
        let n = ok(for_in_start(&heap, task, &Value::Number(5.0)));
        assert!(ok(for_in_next(&heap, task, &n)).is_none());
    }

    /// Array elements (dense, then sparse) precede `length` and the named
    /// properties; holes are absent.
    #[test]
    fn array_keys_list_elements_then_length() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let mut a = ArrayObject::new(Some(task));
        a.push(None);
        a.push(Some(Value::Null));
        assert_eq!(
            a.set(crate::object::array::MAX_INDEX, Value::Null),
            crate::object::SetOutcome::Done
        );
        let key = heap.keys().intern(&JsString::from("x")).unwrap();
        assert!(a.object_mut().define(key, Value::Null));
        let id = heap.alloc(HeapObject::Array(Box::new(a))).unwrap();
        let all = ok(own_property_keys(&heap, task, id, KeyFilter::All));
        assert_eq!(names(&all), ["1", "4294967294", "length", "x"]);
        let enumerable = ok(own_property_keys(&heap, task, id, KeyFilter::Enumerable));
        assert_eq!(names(&enumerable), ["1", "4294967294", "x"]);
        // Another task may not enumerate it.
        assert!(matches!(
            own_property_keys(&heap, TaskId::from_raw(2), id, KeyFilter::All),
            Err(Fault::ForeignObject { .. })
        ));
    }

    /// A function lists its property record's enumerable keys, which only
    /// the creating task may read; all its names wait for its own `length`
    /// and `name` (clj-quickjs-101).
    #[test]
    fn function_keys_come_from_its_owner_checked_record() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let code = crate::frontend::compile_function("function f() {}")
            .unwrap()
            .0;
        let f = function::FunctionObject::new(code, Box::new([]), task);
        let f = heap.alloc(HeapObject::Function(f)).unwrap();
        let enumerable =
            |t: u64| own_property_keys(&heap, TaskId::from_raw(t), f, KeyFilter::Enumerable);
        assert!(ok(enumerable(1)).is_empty(), "no record, no keys");

        let mut record = OrdinaryObject::new(Some(task));
        let x = heap.keys().intern(&JsString::from("x")).unwrap();
        let hidden = heap.keys().intern(&JsString::from("hidden")).unwrap();
        assert!(record.define(x, Value::Null));
        let flags = crate::object::PropertyFlags {
            writable: true,
            enumerable: false,
            configurable: true,
        };
        assert!(record.define_with(hidden, Value::Null, flags));
        let record = heap.alloc(HeapObject::Plain(record)).unwrap();
        heap.with_object_mut(f, |o| match o {
            HeapObject::Function(f) => f.properties = Some(record),
            _ => unreachable!(),
        })
        .unwrap();

        assert_eq!(names(&ok(enumerable(1))), ["x"]);
        assert!(matches!(enumerable(2), Err(Fault::ForeignObject { .. })));
        assert!(matches!(
            own_property_keys(&heap, task, f, KeyFilter::All),
            Err(Fault::Unsupported(_))
        ));
    }
}
