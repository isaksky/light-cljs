//! Prototype reflection (clj-quickjs-121): `Object.getPrototypeOf`,
//! `Object.setPrototypeOf`, `Object.create`, the `Object.prototype.__proto__`
//! accessor and the object literal's `__proto__: value` (`SetProto`), after
//! QuickJS's `js_object_getPrototypeOf`, `js_object_setPrototypeOf`,
//! `js_object_create`, `JS_SetPrototypeInternal`, `js_object_get___proto__`,
//! `js_object_set___proto__` and `OP_set_proto` (`research/quickjs/quickjs.c`,
//! revision 7d1c13c5d8f8); written fresh over this runtime's object model
//! (`crate::object`).
//!
//! # `[[GetPrototypeOf]]` by kind
//!
//! | Value | Prototype |
//! |-------|-----------|
//! | ordinary object, array | its `[[Prototype]]` (another task's object: `ForeignObject`) |
//! | realm intrinsic (`Object.prototype`, `Array.prototype`, `Function.prototype`, built-in functions, `Reflect`) | its fixed prototype (`null`; `Object.prototype`; `Function.prototype` for a built-in function) |
//! | function object | `Function.prototype` (clj-quickjs-102), fixed |
//! | Error instance | ordinary object prototype (initially the matching realm Error prototype) |
//! | persistent vector, map, occurrence; builder, atom, binding cell; other native payloads | `null` (they behave as objects without properties, `crate::vm::property`) |
//!
//! # `[[SetPrototypeOf]]` (OrdinarySetPrototypeOf)
//!
//! Setting the prototype an object already has succeeds and changes
//! nothing, whatever the object. Otherwise only an ordinary object or array
//! owned by the running task changes: a realm intrinsic or built-in function
//! is a `TypeError` (shared and immutable, as for its properties), another
//! task's or the embedder's object is `ForeignObject`, a function (whose
//! prototype cannot be changed yet) is
//! `UnsupportedOperand`, and the other kinds are not extensible
//! (`TypeError`). A non-extensible object (clj-quickjs-122) is a
//! `TypeError`. A new prototype whose chain leads back to the object is a
//! `TypeError` (a cycle): the walk follows ordinary objects and arrays and
//! stops at any other kind, whose prototype is fixed and can never lead back
//! to a task's ordinary object (an intrinsic's chain holds only intrinsics;
//! a function's is `Function.prototype`).
//!
//! # Ownership of prototype links
//!
//! A prototype link is an edge of the task-owned object that holds it, traced
//! with its properties. It may point at an object of the running task, at a
//! realm intrinsic (shared and immutable), at an embedder-made object (which
//! every task reads and none changes) or at a persistent value; it never
//! points at another task's object: `Object.create`, `setPrototypeOf`,
//! `__proto__` and `{__proto__: p}` read the new prototype first, and another
//! task's object fails that read with `ForeignObject`. Task-owned objects are
//! never published (`crate::value::share`), so a prototype link never crosses
//! tasks except into shared, immutable or ownerless objects. Only the owning
//! task changes a link, so a chain the running task walks cannot change
//! under it.
//!
//! # `Object.prototype.__proto__`
//!
//! ECMAScript makes `__proto__` a configurable, non-enumerable accessor
//! property of `Object.prototype` whose getter is `[[GetPrototypeOf]]` of
//! ToObject(this) and whose setter is `[[SetPrototypeOf]]` of `this`,
//! ignoring a value that is neither an object nor `null` (and a primitive
//! `this`). It is stored in the shared immutable realm (clj-quickjs-261)
//! with built-in `get __proto__` / `set __proto__` functions. The generic
//! accessor path calls them when an access reaches the property.
//! An object that does not inherit
//! from `Object.prototype` (`Object.create(null)`) therefore has a plain
//! `__proto__` property, and an own `__proto__` data property (shorthand
//! `{__proto__}`, `defineProperty`) shadows the accessor. Neither built-in
//! runs JavaScript. Descriptor reflection and own-name enumeration expose
//! the property without calling its getter. Its descriptor is configurable,
//! but intrinsic mutation remains forbidden by the realm's sharing policy.

use crate::collections::object_name;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::value::Value;
use crate::vm::dispatch::{self, Fault};
use crate::vm::function;

/// The failure of reading another task's object.
fn foreign(owner: Option<TaskId>, caller: TaskId) -> Fault {
    Fault::ForeignObject { owner, caller }
}

/// Reads: anyone but another task.
fn readable(o: &OrdinaryObject, task: TaskId) -> Result<(), Fault> {
    match o.owner() {
        Some(owner) if owner != task => Err(foreign(Some(owner), task)),
        _ => Ok(()),
    }
}

/// `[[GetPrototypeOf]]` of the object `id` for the running task `task`
/// (module docs, table).
pub(crate) fn get_prototype_of(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
) -> Result<Option<ObjectId>, Fault> {
    if let Some(view) = heap.realm().get().and_then(|r| r.view(id)) {
        return Ok(view.prototype);
    }
    let function = heap
        .read(id, |object| match object {
            HeapObject::Plain(o) => readable(o, task).map(|()| Some(o.prototype())),
            HeapObject::Promise(p) => Ok(Some(Some(p.prototype))),
            HeapObject::Generator(g) => {
                readable(&g.object, task).map(|()| Some(g.object.prototype()))
            }
            HeapObject::JsMap(m) => {
                readable(m.object(), task).map(|()| Some(m.object().prototype()))
            }
            HeapObject::ArrayBuffer(b) => {
                readable(&b.object, task).map(|()| Some(b.object.prototype()))
            }
            HeapObject::TypedArray(a) => {
                readable(&a.object, task).map(|()| Some(a.object.prototype()))
            }
            HeapObject::Array(a) => {
                readable(a.object(), task).map(|()| Some(a.object().prototype()))
            }
            // Fixed: `Function.prototype` (clj-quickjs-102), read below,
            // outside the object lock, since it may build the realm.
            HeapObject::Function(_) | HeapObject::BoundFunction(_) => Ok(None),
            HeapObject::Builtin(f) => Ok(Some(f.properties.prototype())),
            _ => Ok(Some(None)),
        })
        .unwrap_or(Err(Fault::Internal("prototype target was reclaimed")))?;
    match function {
        Some(prototype) => Ok(prototype),
        None => function::function_prototype(heap).map(Some),
    }
}

/// `[[SetPrototypeOf]]` of the object `id` to `proto` for the running task
/// `task`, throwing where it returns false (module docs).
pub(crate) fn set_prototype_of(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
    proto: Option<ObjectId>,
) -> Result<(), Fault> {
    if get_prototype_of(heap, task, id)? == proto {
        return Ok(());
    }
    let immutable = || {
        Err(Fault::TypeError(
            "cannot set the prototype of a built-in object: built-in objects are shared by all tasks and immutable"
                .to_string(),
        ))
    };
    let not_extensible = || {
        Err(Fault::TypeError(
            "cannot set the prototype: the object is not extensible".to_string(),
        ))
    };
    let writable = |o: &OrdinaryObject| match o.owner() {
        _ if o.is_intrinsic() => immutable(),
        Some(owner) if owner == task => match o.extensible() {
            true => Ok(()),
            false => not_extensible(),
        },
        owner => Err(foreign(owner, task)),
    };
    heap.read(id, |object| match object {
        HeapObject::Plain(o) => writable(o),
        HeapObject::JsMap(m) => writable(m.object()),

        HeapObject::ArrayBuffer(b) => writable(&b.object),
        HeapObject::Generator(g) => writable(&g.object),
        HeapObject::TypedArray(a) => writable(&a.object),
        HeapObject::Array(a) => writable(a.object()),
        HeapObject::Builtin(_) => immutable(),
        // A function's prototype lives in its property record, which
        // stands for it; changing it is not supported yet.
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => Err(Fault::Unsupported(
            "changing the prototype of a function is not supported".to_string(),
        )),
        // Error objects failed in `get_prototype_of`.
        other => Err(Fault::TypeError(format!(
            "cannot set the prototype of a {}: it is not extensible",
            object_name(other)
        ))),
    })
    .unwrap_or(Err(Fault::Internal("prototype target was reclaimed")))?;
    check_chain(heap, task, proto, Some(id))?;
    heap.with_object_mut(id, |object| match object {
        HeapObject::Plain(o) => {
            o.set_prototype(proto);
            Ok(())
        }
        HeapObject::JsMap(m) => {
            m.object_mut().set_prototype(proto);
            Ok(())
        }
        HeapObject::Array(a) => {
            a.object_mut().set_prototype(proto);
            Ok(())
        }
        HeapObject::ArrayBuffer(b) => {
            b.object.set_prototype(proto);
            Ok(())
        }
        HeapObject::Generator(g) => {
            g.object.set_prototype(proto);
            Ok(())
        }
        HeapObject::TypedArray(a) => {
            a.object.set_prototype(proto);
            Ok(())
        }
        _ => Err(Fault::Internal("prototype target changed kind")),
    })
    .unwrap_or(Err(Fault::Internal("prototype target was reclaimed")))
}

/// Walk the chain that starts at `proto` (module docs): the running task
/// must be able to read every ordinary object and array on it, and
/// `target` (the object getting the prototype, if it exists yet) must not
/// be on it.
fn check_chain(
    heap: &Heap,
    task: TaskId,
    mut proto: Option<ObjectId>,
    target: Option<ObjectId>,
) -> Result<(), Fault> {
    let realm = heap.realm().get();
    while let Some(p) = proto {
        if Some(p) == target {
            return Err(Fault::TypeError(
                "cannot set the prototype: the prototype chain would be circular".to_string(),
            ));
        }
        // An intrinsic's chain holds only intrinsics.
        if realm.and_then(|r| r.view(p)).is_some() {
            return Ok(());
        }
        proto = heap
            .read(p, |object| match object {
                HeapObject::Plain(o) => readable(o, task).map(|()| o.prototype()),
                HeapObject::Generator(g) => {
                    readable(&g.object, task).map(|()| g.object.prototype())
                }
                HeapObject::JsMap(m) => readable(m.object(), task).map(|()| m.object().prototype()),

                HeapObject::ArrayBuffer(b) => {
                    readable(&b.object, task).map(|()| b.object.prototype())
                }
                HeapObject::TypedArray(a) => {
                    readable(&a.object, task).map(|()| a.object.prototype())
                }
                HeapObject::Array(a) => readable(a.object(), task).map(|()| a.object().prototype()),
                // A fixed prototype that cannot lead back (module docs).
                _ => Ok(None),
            })
            .unwrap_or(Err(Fault::Internal("prototype was reclaimed")))?;
    }
    Ok(())
}

/// The prototype argument of `Object.create`, `Object.setPrototypeOf` and
/// a literal's `__proto__:`: `Some(None)` for `null`, `Some(Some(id))` for
/// an object, `None` for anything else.
fn as_prototype(value: &Value) -> Option<Option<ObjectId>> {
    match value {
        Value::Null => Some(None),
        Value::Object(id) => Some(Some(*id)),
        _ => None,
    }
}

/// `Object.getPrototypeOf(v)`: ToObject, then `[[GetPrototypeOf]]`.
pub(crate) fn object_get_prototype_of(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
) -> Result<Value, Fault> {
    let v = args.first().cloned().unwrap_or(Value::Undefined);
    match v {
        Value::BigInt(_) => crate::realm::intrinsics(heap)
            .map(|realm| Value::Object(realm.bigint_prototype))
            .ok_or(Fault::OutOfMemory("heap object table exhausted")),
        Value::Object(id) => Ok(prototype_value(get_prototype_of(heap, task, id)?)),
        Value::Undefined | Value::Null => Err(Fault::TypeError(format!(
            "cannot convert {} to object",
            v.type_name()
        ))),
        _ => Err(Fault::Unsupported(format!(
            "Object.getPrototypeOf of a {} needs a wrapper object, which is not supported",
            v.type_name()
        ))),
    }
}

fn prototype_value(proto: Option<ObjectId>) -> Value {
    proto.map_or(Value::Null, Value::Object)
}

/// `Object.setPrototypeOf(o, proto)`: returns `o`.
pub(crate) fn object_set_prototype_of(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
) -> Result<Value, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    let (target, proto) = (arg(0), arg(1));
    if matches!(target, Value::Undefined | Value::Null) {
        return Err(Fault::TypeError(format!(
            "Object.setPrototypeOf called on {}",
            target.type_name()
        )));
    }
    let Some(proto) = as_prototype(&proto) else {
        return Err(Fault::TypeError(format!(
            "an object's prototype must be an object or null, got {}",
            proto.type_name()
        )));
    };
    // A primitive target is returned unchanged.
    if let Value::Object(id) = target {
        set_prototype_of(heap, task, id, proto)?;
    }
    Ok(target)
}

/// `Object.create(proto, properties)`: a new ordinary object owned by the
/// running task with prototype `proto`. A `properties` argument other than
/// `undefined` (Object.defineProperties) fails with `UnsupportedOperand`.
pub(crate) fn object_create(heap: &Heap, task: TaskId, args: &[Value]) -> Result<Value, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    let proto = arg(0);
    let Some(proto) = as_prototype(&proto) else {
        return Err(Fault::TypeError(format!(
            "an object's prototype must be an object or null, got {}",
            proto.type_name()
        )));
    };
    if arg(1) != Value::Undefined {
        return Err(Fault::Unsupported(
            "Object.create with a properties argument (Object.defineProperties) is not supported"
                .to_string(),
        ));
    }
    check_chain(heap, task, proto, None)?;
    let mut object = OrdinaryObject::new(Some(task));
    object.set_prototype(proto);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

/// The `__proto__` getter: ToObject(this), then `[[GetPrototypeOf]]`.
/// A supported primitive's wrapper has the realm's primitive prototype;
/// no wrapper allocation is needed to return that fixed link.
pub(crate) fn proto_getter(heap: &Heap, task: TaskId, this: &Value) -> Result<Value, Fault> {
    match this {
        Value::Object(receiver) => get_prototype_of(heap, task, *receiver).map(prototype_value),
        Value::Undefined | Value::Null => Err(Fault::TypeError(format!(
            "cannot convert {} to object",
            this.type_name()
        ))),
        _ => {
            let realm = crate::realm::intrinsics(heap)
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            realm.primitive_prototype(this).map(Value::Object).ok_or_else(|| {
                Fault::Unsupported(format!(
                    "__proto__ getter on a {} needs a wrapper prototype, which is not supported",
                    this.type_name()
                ))
            })
        }
    }
}

/// The `__proto__` setter: RequireObjectCoercible(this), ignore a
/// non-object/non-null prototype or primitive receiver, otherwise
/// `[[SetPrototypeOf]]`. It always returns `undefined`.
pub(crate) fn proto_setter(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    value: &Value,
) -> Result<Value, Fault> {
    if matches!(this, Value::Undefined | Value::Null) {
        return Err(Fault::TypeError(format!(
            "__proto__ setter called on {}",
            this.type_name()
        )));
    }
    if let (Value::Object(receiver), Some(proto)) = (this, as_prototype(value)) {
        set_prototype_of(heap, task, *receiver, proto)?;
    }
    Ok(Value::Undefined)
}

/// `SetProto` (QuickJS `OP_set_proto`): a literal's `__proto__: value` sets
/// the prototype of the literal's fresh object `object` when `value` is an
/// object or `null`, and is ignored otherwise.
pub(crate) fn set_literal_proto(
    heap: &Heap,
    task: TaskId,
    object: &Value,
    value: &Value,
) -> Result<(), Fault> {
    let Value::Object(id) = *object else {
        return Err(Fault::Unsupported(format!(
            "set_proto needs an object, got {}",
            object.type_name()
        )));
    };
    match as_prototype(value) {
        Some(proto) => set_prototype_of(heap, task, id, proto),
        None => Ok(()),
    }
}

/// `v instanceof target` (`Instanceof`, clj-quickjs-75): ECMAScript
/// InstanceofOperator and OrdinaryHasInstance, after QuickJS
/// `JS_IsInstanceOf` and `JS_OrdinaryIsInstanceOf`. `target` must be an
/// object (`Symbol.hasInstance` is not consulted: protocol dispatch is deferred)
/// and callable; a primitive `v` is not an instance of anything; otherwise
/// `target.prototype` must be an object and the answer is whether it is on
/// `v`'s prototype chain, compared by identity. Bound functions delegate
/// to their targets, including when their own `prototype` is assigned.
///
/// The walk only reads: each step is `[[GetPrototypeOf]]`
/// ([`get_prototype_of`]), so its owner rules apply; functions continue at
/// `Function.prototype`. Chains are acyclic
/// ([`set_prototype_of`] checks), so it ends. Reading `prototype` may allocate a function's lazy
/// prototype object; the caller keeps `v` and `target` reachable.
/// Validate and unwrap the callable before reading its prototype. A
/// primitive left operand returns false without running a getter.
pub(crate) fn instance_target(
    heap: &Heap,
    v: &Value,
    target: &Value,
) -> Result<Option<Value>, Fault> {
    let mut target = target.clone();
    while let Ok(dispatch::Callee::Bound(bound)) = dispatch::callee(heap, &target) {
        target = bound.callee;
    }
    let target = &target;
    let invalid = || Fault::TypeError("invalid 'instanceof' right operand".to_string());
    if !matches!(target, Value::Object(_)) {
        return Err(invalid());
    }
    match dispatch::callee(heap, target) {
        Ok(_) => {}
        Err(Fault::NotCallable(_)) => return Err(invalid()),
        Err(fault) => return Err(fault),
    }
    Ok(matches!(v, Value::Object(_)).then(|| target.clone()))
}

pub(crate) fn instance_of_prototype(
    heap: &Heap,
    task: TaskId,
    v: &Value,
    prototype: &Value,
) -> Result<bool, Fault> {
    let Value::Object(id) = *v else {
        return Ok(false);
    };
    let Value::Object(prototype) = *prototype else {
        return Err(Fault::TypeError(
            "operand 'prototype' property is not an object".to_string(),
        ));
    };
    let mut at = id;
    while let Some(next) = get_prototype_of(heap, task, at)? {
        if next == prototype {
            return Ok(true);
        }
        at = next;
    }
    Ok(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::realm;

    fn plain(heap: &Heap, owner: Option<TaskId>) -> ObjectId {
        heap.alloc(HeapObject::Plain(OrdinaryObject::new(owner)))
            .unwrap()
    }

    /// Cross-task and embedder prototypes (module docs, "Ownership of
    /// prototype links"); source code cannot reach another task's object.
    #[test]
    fn prototype_links_never_reach_another_tasks_object() {
        let heap = Heap::default();
        let (t1, t2) = (TaskId::from_raw(1), TaskId::from_raw(2));
        let mine = plain(&heap, Some(t1));
        let theirs = plain(&heap, Some(t2));
        let embedder = plain(&heap, None);
        let foreign = |r: Result<(), Fault>| matches!(r, Err(Fault::ForeignObject { .. }));
        assert!(foreign(set_prototype_of(&heap, t1, mine, Some(theirs))));
        assert!(foreign(set_prototype_of(&heap, t1, theirs, None)));
        let create = object_create(&heap, t1, &[Value::Object(theirs)]);
        assert!(matches!(create, Err(Fault::ForeignObject { .. })));
        // An embedder object is readable by every task and changed by none.
        assert!(set_prototype_of(&heap, t1, mine, Some(embedder)).is_ok());
        assert_eq!(get_prototype_of(&heap, t2, embedder).ok(), Some(None));
        assert!(foreign(set_prototype_of(&heap, t1, embedder, Some(mine))));
        // Setting the prototype an object has is a no-op even where a change
        // is not allowed.
        let proto = realm::intrinsics(&heap).unwrap().object_prototype;
        assert!(set_prototype_of(&heap, t1, proto, None).is_ok());
        assert!(matches!(
            set_prototype_of(&heap, t1, proto, Some(mine)),
            Err(Fault::TypeError(_))
        ));
    }

    /// The stored built-ins retain the same ownership boundary as the
    /// property-access path, even when explicitly called with a receiver.
    #[test]
    fn proto_accessors_reject_cross_task_links() {
        use crate::vm::builtin::{self, Builtin};

        let heap = Heap::default();
        let (owner, caller) = (TaskId::from_raw(1), TaskId::from_raw(2));
        let mine = Value::Object(plain(&heap, Some(caller)));
        let theirs = Value::Object(plain(&heap, Some(owner)));
        let get = |receiver| {
            builtin::call(
                &heap,
                caller,
                Builtin::ObjectPrototypeProtoGetter,
                receiver,
                &[],
            )
        };
        let set = |receiver, value| {
            builtin::call(
                &heap,
                caller,
                Builtin::ObjectPrototypeProtoSetter,
                receiver,
                &[value],
            )
        };
        assert!(matches!(get(&theirs), Err(Fault::ForeignObject { .. })));
        assert!(matches!(
            set(&mine, theirs.clone()),
            Err(Fault::ForeignObject { .. })
        ));
        assert!(matches!(
            set(&theirs, Value::Null),
            Err(Fault::ForeignObject { .. })
        ));
        // An ignored value does not read or mutate the receiver or prototype.
        assert_eq!(
            set(&theirs, Value::Number(1.0)).ok(),
            Some(Value::Undefined)
        );
        assert_eq!(
            set(&Value::Number(1.0), theirs.clone()).ok(),
            Some(Value::Undefined)
        );
        assert_eq!(get(&mine).ok(), Some(Value::Null));
    }

    /// A non-extensible object rejects a new prototype (from source:
    /// `tests/js_source.rs`, `object_extensibility`, clj-quickjs-122).
    #[test]
    fn non_extensible_objects_keep_their_prototype() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let mut object = OrdinaryObject::new(Some(task));
        object.prevent_extensions();
        let id = heap.alloc(HeapObject::Plain(object)).unwrap();
        let other = plain(&heap, Some(task));
        assert!(set_prototype_of(&heap, task, id, None).is_ok());
        match set_prototype_of(&heap, task, id, Some(other)) {
            Err(Fault::TypeError(m)) => assert!(m.contains("not extensible"), "{m}"),
            _ => panic!("expected a TypeError"),
        }
        assert_eq!(get_prototype_of(&heap, task, id).ok(), Some(None));
    }
}
