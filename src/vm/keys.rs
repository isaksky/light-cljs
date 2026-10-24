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
//! [`ForInIterator`], after QuickJS's `build_for_in_iterator`,
//! `js_for_in_prepare_prototype_chain_enum` and `js_for_in_next`
//! (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8).
//! `ForInStart` snapshots the operand's own string keys, enumerable or not,
//! in the order above (an object's, including a function's property
//! record), or a string's indices; `null`, `undefined`, numbers and
//! booleans (whose wrappers have no enumerable own keys) give none. Each
//! `ForInNext` takes the next enumerable key of the snapshot, re-checks it
//! with [`property::get_own_property`] and skips it unless it is still an
//! own enumerable property of the object it was found on (ECMAScript's
//! informative EnumerateObjectProperties re-reads the descriptor before
//! each key; QuickJS checks existence only), so a key deleted before it is
//! reached is not visited; keys added to an object after its snapshot are
//! not visited. Scanning past skipped keys is bounded by the snapshot and
//! allocates nothing.
//!
//! # Prototype chain
//!
//! When an object's keys are used up (clj-quickjs-86), the walk moves to
//! its prototype (`[[GetPrototypeOf]]`, `crate::vm::prototype`) and
//! snapshots that object's own keys the same way, so the chain is read
//! lazily, one object at a time, when the walk reaches it: a prototype
//! changed or a key added to a prototype before the walk reaches it counts,
//! and the walk continues on the prototype it is on (and then that
//! prototype's own chain) when the operand's chain changes under it. A
//! prototype key is visited only if no object the walk has left had an own
//! property of that name in its snapshot, enumerable or not (a
//! non-enumerable own `x` hides an inherited enumerable `x`; QuickJS keeps
//! these names as the properties of its enumerator object, this enumerator
//! in two hash sets, of indices and of names). The walk ends:
//!
//! - at a `null` prototype;
//! - at a realm intrinsic (`Object.prototype`, `Array.prototype`, a built-in
//!   function) when no intrinsic on its chain has an enumerable property,
//!   which is always so today. The intrinsics are shared and immutable
//!   (`crate::realm`), so this is decided from the realm's immutable views
//!   without any lock, and every walk that reaches `Object.prototype` ends
//!   there with no lock taken for it. (Were an intrinsic's property
//!   enumerable, its keys would be named from the view under the key
//!   table's leaf lock, still not the object table's.) The view of
//!   `Object.prototype` omits the `__proto__` accessor, which is
//!   non-enumerable and would hide nothing: nothing is above it;
//! - after a function or built-in function: its prototype is
//!   `Function.prototype` (clj-quickjs-102), an immutable intrinsic with no
//!   enumerable property on its chain, so nothing above could be visited.
//!   The same holds for an error object and `Error.prototype`, but an error
//!   object on the chain fails as an operand does (`UnsupportedOperand`:
//!   its own keys are clj-quickjs-230).
//!
//! Prototype links never lead to another task's object
//! (`crate::vm::prototype`, "Ownership of prototype links"), and chains are
//! acyclic, so the walk ends.
//!
//! The enumerator is a [`HeapObject::Native`] payload: task-owned
//! (`Shareability::TaskOwned`), traced (its only edge is the object the
//! walk is on, the operand and then each prototype: a prototype the body
//! unlinked from the chain stays alive while the walk is on it), and held
//! only by a scratch local of the loop's frame (`crate::frontend` lowers
//! the loop), so it lives exactly as long as the frame needs it, survives
//! yields, waits, collections and migration with the frame, and is never
//! reachable from JavaScript or published. Each `ForInNext` runs within
//! one instruction, without a safepoint, so moving along the chain never
//! leaves an untraced object behind.

use std::collections::HashSet;

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
use crate::vm::prototype;

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
    let function = heap
        .read(target, |o| matches!(o, HeapObject::Function(_)))
        .ok_or(Fault::Internal("key enumeration target was reclaimed"))?;
    if function && filter == KeyFilter::All {
        return Err(Fault::Unsupported(
            "the own property names of a function need its own length and name, which are not supported"
                .to_string(),
        ));
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
    let Some(store) = key_store(heap, target)? else {
        return Ok(Vec::new());
    };
    let raw = heap
        .read(store, |object| collect(object, task))
        .unwrap_or(Err(Fault::Internal("key enumeration target was reclaimed")))?;
    Ok(order(heap, raw, filter))
}

/// The object that holds `target`'s keys: `target` itself, or a function's
/// property record (`None` for a function without one, whose only own
/// keys would be its unstored `length` and `name`).
fn key_store(heap: &Heap, target: ObjectId) -> Result<Option<ObjectId>, Fault> {
    let function = heap
        .read(target, |o| matches!(o, HeapObject::Function(_)))
        .ok_or(Fault::Internal("key enumeration target was reclaimed"))?;
    if function {
        function::property_record(heap, target)
    } else {
        Ok(Some(target))
    }
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

/// The task-owned enumerator of a `for`-`in` loop (clj-quickjs-85, -86;
/// QuickJS `JSForInIterator`, `build_for_in_iterator`,
/// `js_for_in_prepare_prototype_chain_enum`, `js_for_in_next`): the object
/// the walk is on (the operand, then each prototype), a snapshot of that
/// object's own string keys taken when the walk reached it, the position
/// of the next one, and the names of every object the walk has left. It
/// lives in a heap object ([`HeapObject::Native`]) held only by a scratch
/// local of the loop's frame, so it is traced through that frame, survives
/// yields, waits, collections and migration with it, and is never
/// reachable from JavaScript (module docs, "`for`-`in`").
#[derive(Debug)]
pub(crate) struct ForInIterator {
    /// The object whose keys `keys` lists; `None` for a primitive operand
    /// (whose keys never change and whose chain is not walked) and once the
    /// walk has ended.
    object: Option<ObjectId>,
    /// Every own string key of `object`, enumerable or not: a
    /// non-enumerable key is never visited, but it hides the name further
    /// up the chain.
    keys: Vec<OwnKey>,
    next: usize,
    /// The keys of the objects the walk has left (module docs, "Prototype chain").
    visited: Visited,
}

/// A set of property keys (`Key` is a name or an index).
#[derive(Debug, Default)]
struct Visited {
    indices: HashSet<u32>,
    names: HashSet<JsString>,
}

impl Visited {
    fn contains(&self, key: &Key) -> bool {
        match key {
            Key::Index(index) => self.indices.contains(index),
            Key::Name(name) => self.names.contains(name),
        }
    }

    fn insert(&mut self, key: Key) {
        match key {
            Key::Index(index) => self.indices.insert(index),
            Key::Name(name) => self.names.insert(name),
        };
    }
}

/// What [`ForInIterator::step`] found.
enum Step {
    /// A candidate key of `object` (`None`: of a primitive operand).
    Key(Option<ObjectId>, Key),
    /// The keys of this object are used up: continue with its prototype.
    Exhausted(ObjectId),
    /// The walk has ended.
    Done,
}

impl ForInIterator {
    fn new(object: Option<ObjectId>, keys: Vec<OwnKey>) -> Self {
        ForInIterator {
            object,
            keys,
            next: 0,
            visited: Visited::default(),
        }
    }

    /// Advance past the next key that is enumerable in the snapshot and not
    /// hidden by an object the walk has left. Bounded by the snapshot;
    /// allocates nothing.
    fn step(&mut self) -> Step {
        while let Some(key) = self.keys.get(self.next) {
            self.next += 1;
            if key.enumerable && !self.visited.contains(&key.key) {
                return Step::Key(self.object, key.key.clone());
            }
        }
        match self.object {
            Some(object) => Step::Exhausted(object),
            None => Step::Done,
        }
    }

    /// Leave the current object for `next` (`None`: the walk has ended),
    /// whose own keys are `keys`.
    fn enter(&mut self, next: Option<ObjectId>, keys: Vec<OwnKey>) {
        let left = std::mem::replace(&mut self.keys, keys);
        if next.is_some() {
            left.into_iter().for_each(|k| self.visited.insert(k.key));
        }
        self.object = next;
        self.next = 0;
    }
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

/// The own string keys (enumerable or not) of the object `id` that the
/// walk has reached, or `None` when nothing from `id` up its chain can be
/// visited (module docs, "Prototype chain"): `id` is a realm intrinsic and
/// no intrinsic on its chain has an enumerable property, decided from the
/// realm's immutable views without any lock.
fn walk_keys(heap: &Heap, task: TaskId, id: ObjectId) -> Result<Option<Vec<OwnKey>>, Fault> {
    if let Some(realm) = heap.realm().get()
        && let Some(view) = realm.view(id)
    {
        // An intrinsic's chain holds only intrinsics; anything else on it
        // (never the case) is read the general way.
        let enumerable =
            std::iter::successors(Some(id), |&p| realm.view(p).and_then(|v| v.prototype)).any(
                |p| {
                    realm
                        .view(p)
                        .is_none_or(|v| v.own_keys().any(|(_, enumerable)| enumerable))
                },
            );
        if !enumerable {
            return Ok(None);
        }
        // Not the case for any intrinsic yet: name the keys (the key
        // table's leaf lock), still without the object table.
        let raw = Raw {
            named: view.own_keys().collect(),
            ..Raw::default()
        };
        return Ok(Some(order(heap, raw, KeyFilter::All)));
    }
    let Some(store) = key_store(heap, id)? else {
        return Ok(Some(Vec::new()));
    };
    let raw = heap
        .read(store, |object| collect(object, task))
        .unwrap_or(Err(Fault::Internal("key enumeration target was reclaimed")))?;
    Ok(Some(order(heap, raw, KeyFilter::All)))
}

/// The next object of the walk after `id` (module docs, "Prototype
/// chain"): its `[[GetPrototypeOf]]`, except that the walk ends at a
/// function or error object, whose prototype (`Function.prototype`,
/// `Error.prototype`) is an immutable intrinsic with no enumerable
/// property on its chain.
fn chain_next(heap: &Heap, task: TaskId, id: ObjectId) -> Result<Option<ObjectId>, Fault> {
    if let Some(view) = heap.realm().get().and_then(|r| r.view(id)) {
        return Ok(if view.builtin { None } else { view.prototype });
    }
    let fixed = heap
        .read(id, |object| match object {
            HeapObject::Function(_) | HeapObject::Builtin(_) => true,
            HeapObject::Native(payload) => (&**payload as &dyn std::any::Any).is::<ErrorObject>(),
            _ => false,
        })
        .ok_or(Fault::Internal("prototype was reclaimed"))?;
    if fixed {
        return Ok(None);
    }
    prototype::get_prototype_of(heap, task, id)
}

/// `ForInStart`: the enumerator of `value` for the running task `task`, a
/// new heap object (module docs, "`for`-`in`").
pub(crate) fn for_in_start(heap: &Heap, task: TaskId, value: &Value) -> Result<Value, Fault> {
    let iterator = match value {
        Value::Object(id) => match walk_keys(heap, task, *id)? {
            Some(keys) => ForInIterator::new(Some(*id), keys),
            None => ForInIterator::new(None, Vec::new()),
        },
        // A String wrapper's own enumerable keys are its indices; its
        // prototype has no enumerable properties.
        Value::String(s) => {
            let len = u32::try_from(s.len())
                .map_err(|_| Fault::Internal("a string longer than 2^32 - 1 code units"))?;
            let index = |i| OwnKey {
                key: Key::Index(i),
                enumerable: true,
            };
            ForInIterator::new(None, (0..len).map(index).collect())
        }
        // `null` and `undefined` run no iteration; a Number or Boolean
        // wrapper has no own enumerable keys.
        Value::Undefined | Value::Null | Value::Bool(_) | Value::Number(_) => {
            ForInIterator::new(None, Vec::new())
        }
    };
    heap.alloc(HeapObject::Native(Box::new(iterator)))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

/// Apply `f` to the enumerator `id` under the object lock.
fn with_iterator<R>(
    heap: &Heap,
    id: ObjectId,
    f: impl FnOnce(&mut ForInIterator) -> R,
) -> Result<R, Fault> {
    heap.with_object_mut(id, |o| match o {
        HeapObject::Native(payload) => {
            let any: &mut dyn std::any::Any = &mut **payload;
            any.downcast_mut::<ForInIterator>().map(f)
        }
        _ => None,
    })
    .flatten()
    .ok_or(Fault::Internal("for-in enumerator is not an enumerator"))
}

/// `ForInNext`: the next key of the enumerator `iterator`, as a string;
/// `None` when the walk has ended. A key is visited when it is enumerable
/// in its object's snapshot, no object the walk has left had an own
/// property of that name, and it is still an own enumerable property of
/// that object (a key deleted, or made non-enumerable, before it is
/// reached is skipped). The walk moves to the next prototype when an
/// object's keys are used up (module docs, "Prototype chain").
pub(crate) fn for_in_next(
    heap: &Heap,
    task: TaskId,
    iterator: &Value,
) -> Result<Option<JsString>, Fault> {
    let Value::Object(id) = *iterator else {
        return Err(Fault::Internal("for-in enumerator is not an object"));
    };
    loop {
        // Each step takes the enumerator's lock alone; the lookups below
        // take the object lock again, after it is released.
        match with_iterator(heap, id, ForInIterator::step)? {
            Step::Done => return Ok(None),
            Step::Key(object, key) => {
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
            Step::Exhausted(object) => {
                let (next, keys) = match chain_next(heap, task, object)? {
                    Some(proto) => match walk_keys(heap, task, proto)? {
                        Some(keys) => (Some(proto), keys),
                        None => (None, Vec::new()),
                    },
                    None => (None, Vec::new()),
                };
                with_iterator(heap, id, |it| it.enter(next, keys))?;
            }
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

    /// The prototype walk (clj-quickjs-86): the enumerator's only edge is
    /// the object it is on, the operand and then its prototype (which may
    /// be reachable from nothing else once the chain changes); an own
    /// non-enumerable key hides the prototype's; and the walk ends at
    /// `Object.prototype` without entering it (its chain has no enumerable
    /// property, read from the realm's lock-free views).
    #[test]
    fn for_in_enumerator_walks_and_traces_the_prototype_it_is_on() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let object_prototype = realm::intrinsics(&heap).unwrap().object_prototype;
        let key = |name: &str| heap.keys().intern(&JsString::from(name)).unwrap();
        let hidden = crate::object::PropertyFlags {
            writable: true,
            enumerable: false,
            configurable: true,
        };
        let mut proto = OrdinaryObject::new(Some(task));
        proto.set_prototype(Some(object_prototype));
        assert!(proto.define(key("h"), Value::Null));
        assert!(proto.define(key("p"), Value::Null));
        let proto = heap.alloc(HeapObject::Plain(proto)).unwrap();
        let mut o = OrdinaryObject::new(Some(task));
        o.set_prototype(Some(proto));
        assert!(o.define_with(key("h"), Value::Null, hidden));
        assert!(o.define(key("a"), Value::Null));
        let o = heap.alloc(HeapObject::Plain(o)).unwrap();

        let it = ok(for_in_start(&heap, task, &Value::Object(o)));
        let Value::Object(it_id) = it else {
            panic!("not an object")
        };
        let edges = || {
            let mut edges = Vec::new();
            heap.read(it_id, |o| o.trace(&mut |e| edges.push(e)))
                .unwrap();
            edges
        };
        let next = || ok(for_in_next(&heap, task, &it)).map(|s| s.to_string());
        assert_eq!(edges(), [o]);
        assert_eq!(next().as_deref(), Some("a"));
        assert_eq!(edges(), [o]);
        assert_eq!(next().as_deref(), Some("p"));
        assert_eq!(edges(), [proto]);
        assert_eq!(next(), None);
        assert!(edges().is_empty(), "Object.prototype was entered");
        assert_eq!(next(), None);
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
