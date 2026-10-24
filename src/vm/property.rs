//! Named property access (clj-quickjs-114): the `GetField`, `PutField` and
//! `DefineField` instructions, after QuickJS's `get_field`, `put_field` and
//! `define_field` opcodes (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8: `JS_GetPropertyInternal`, `JS_SetPropertyInternal`,
//! `JS_CreateDataPropertyUint32`/`add_property`); written fresh over this
//! runtime's object model (`crate::object`).
//!
//! # Which values have properties
//!
//! | Base | Read `b.x` | Write `b.x = v` |
//! |------|-----------|-----------------|
//! | ordinary object owned by the running task | own property, else the prototype chain, else `undefined` | update or add an own data property |
//! | ordinary object created by the embedder (no owner) | as above | `ForeignObject` failure |
//! | ordinary object owned by another task | `ForeignObject` failure | `ForeignObject` failure |
//! | persistent vector, map, occurrence; builder, atom, binding cell; native payload other than an error object | `undefined` (no JavaScript properties) | catchable `TypeError` |
//! | function object | `UnsupportedOperand` (its own properties are clj-quickjs-101/-125) | `UnsupportedOperand` |
//! | error object (a caught run-time error) | `UnsupportedOperand` (`message`/`name` are clj-quickjs-230) | `UnsupportedOperand` |
//! | `undefined`, `null` | catchable `TypeError` | catchable `TypeError` |
//! | number, boolean, string | `UnsupportedOperand` (primitive wrappers are clj-quickjs-73) | `UnsupportedOperand` |
//!
//! The non-ordinary heap kinds are this runtime's own values (or values
//! JavaScript code never holds), not JavaScript objects with hidden
//! properties: they behave like objects with no own properties, a null
//! prototype and no extensibility, so a read finds nothing and a write is
//! the strict-mode TypeError of an object that cannot take the property.
//! Function objects and error objects do have JavaScript properties
//! (`length`, `name`, `message`, ...); until those exist, touching them is
//! rejected explicitly instead of answering `undefined`.
//!
//! A write never consults the prototype chain while it is null (always, in
//! this slice); with a prototype present, a write of a property the object
//! does not have itself fails explicitly until the prototype pellets decide
//! setters and read-only inherited properties (clj-quickjs-120/-121).
//!
//! No operation here allocates a heap object or runs JavaScript, so none is
//! a poll point; each object visited is read or changed under one short
//! object-table lock, and the property-key table is consulted before, never
//! while that lock is held.

use crate::collections::object_name;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::SetOutcome;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::exception::ErrorObject;

/// Which access failed, for messages.
#[derive(Clone, Copy)]
enum Access {
    Read,
    Write,
}

impl Access {
    fn verb(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "set",
        }
    }
}

/// The failure of an access to property `name` of a primitive `base`.
fn primitive_base(base: &Value, name: &JsString, access: Access) -> Fault {
    let verb = access.verb();
    match base {
        Value::Undefined | Value::Null => Fault::TypeError(format!(
            "cannot {verb} property '{name}' of {}",
            base.type_name()
        )),
        _ => Fault::Unsupported(format!(
            "cannot {verb} property '{name}' of a {}: properties of primitive values are not supported",
            base.type_name()
        )),
    }
}

/// The failure of an access to a heap object that is not an ordinary
/// object, or `None` when the access proceeds as on an object with no
/// properties (module docs).
fn not_ordinary(object: &HeapObject, name: &JsString, access: Access) -> Option<Fault> {
    let verb = access.verb();
    let unsupported = |what: &str| {
        Some(Fault::Unsupported(format!(
            "cannot {verb} property '{name}' of {what}: its properties are not supported"
        )))
    };
    match object {
        HeapObject::Function(_) => unsupported("a function"),
        HeapObject::Native(payload) => {
            let any: &dyn std::any::Any = &**payload;
            if any.is::<ErrorObject>() {
                return unsupported("an error object");
            }
            no_properties(object, name, access)
        }
        _ => no_properties(object, name, access),
    }
}

fn no_properties(object: &HeapObject, name: &JsString, access: Access) -> Option<Fault> {
    match access {
        Access::Read => None,
        Access::Write => Some(Fault::TypeError(format!(
            "cannot set property '{name}' of a {}: it has no JavaScript properties",
            object_name(object)
        ))),
    }
}

/// The `ForeignObject` failure of `caller` touching an object of `owner`.
fn foreign(owner: Option<TaskId>, caller: TaskId) -> Fault {
    Fault::ForeignObject { owner, caller }
}

/// One step of a property read.
enum Lookup {
    Found(Value),
    /// Not an own property: continue at the prototype (or stop at null).
    Next(Option<ObjectId>),
}

/// `base.name` for the running task `task` (QuickJS `get_field`).
pub(crate) fn get_field(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &JsString,
) -> Result<Value, Fault> {
    let Value::Object(mut id) = *base else {
        return Err(primitive_base(base, name, Access::Read));
    };
    // A name that was never interned is no object's property; the owner
    // and kind checks below still apply.
    let key = heap.keys().lookup(name);
    loop {
        let step = heap
            .read(id, |object| match object {
                HeapObject::Plain(o) => match o.owner() {
                    Some(owner) if owner != task => Err(foreign(Some(owner), task)),
                    _ => Ok(match key.and_then(|k| o.get_own(k)) {
                        Some(property) => Lookup::Found(property.value.clone()),
                        None => Lookup::Next(o.prototype()),
                    }),
                },
                other => match not_ordinary(other, name, Access::Read) {
                    Some(fault) => Err(fault),
                    None => Ok(Lookup::Found(Value::Undefined)),
                },
            })
            // The frame keeps the base alive and every object keeps its
            // prototype alive; a stale id is reported, not trusted.
            .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
        match step {
            Lookup::Found(v) => return Ok(v),
            Lookup::Next(Some(prototype)) => id = prototype,
            Lookup::Next(None) => return Ok(Value::Undefined),
        }
    }
}

/// `base.name = value` for the running task `task` (QuickJS `put_field`,
/// strict mode: a rejected write is a TypeError).
pub(crate) fn put_field(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &JsString,
    value: Value,
) -> Result<(), Fault> {
    let Value::Object(id) = *base else {
        return Err(primitive_base(base, name, Access::Write));
    };
    let key = heap
        .keys()
        .intern(name)
        .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
    heap.with_object_mut(id, |object| match object {
        HeapObject::Plain(o) if o.owner() != Some(task) => Err(foreign(o.owner(), task)),
        HeapObject::Plain(o) => match o.set(key, value) {
            SetOutcome::Done => Ok(()),
            SetOutcome::ReadOnly => Err(Fault::TypeError(format!(
                "cannot assign to read-only property '{name}'"
            ))),
            SetOutcome::NotExtensible => Err(Fault::TypeError(format!(
                "cannot add property '{name}': the object is not extensible"
            ))),
            SetOutcome::Inherited => Err(Fault::Unsupported(format!(
                "cannot set property '{name}': assignment through a prototype chain is not supported"
            ))),
        },
        other => Err(not_ordinary(other, name, Access::Write)
            .unwrap_or(Fault::Internal("a property write was not rejected"))),
    })
    .unwrap_or(Err(Fault::Internal("property base was reclaimed")))
}

/// Define `name` as an own data property of the object literal `object`
/// (QuickJS `define_field`: CreateDataPropertyOrThrow). The object was
/// created by the running task's `NewObject`, so it is ordinary, owned and
/// extensible; anything else is a broken invariant of hand-assembled code
/// and fails as `UnsupportedOperand` or `Internal`.
pub(crate) fn define_field(
    heap: &Heap,
    task: TaskId,
    object: &Value,
    name: &JsString,
    value: Value,
) -> Result<(), Fault> {
    let Value::Object(id) = *object else {
        return Err(Fault::Unsupported(format!(
            "define_field needs an object, got {}",
            object.type_name()
        )));
    };
    let key = heap
        .keys()
        .intern(name)
        .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
    heap.with_object_mut(id, |target| match target {
        HeapObject::Plain(o) if o.owner() != Some(task) => Err(foreign(o.owner(), task)),
        HeapObject::Plain(o) => {
            if o.define(key, value) {
                Ok(())
            } else {
                Err(Fault::TypeError(format!(
                    "cannot define property '{name}': the object is not extensible"
                )))
            }
        }
        other => Err(Fault::Unsupported(format!(
            "define_field needs an ordinary object, got {}",
            object_name(other)
        ))),
    })
    .unwrap_or(Err(Fault::Internal("object literal was reclaimed")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heap::NativePayload;
    use crate::heap::gc::Trace;
    use crate::value::share::Shareability;

    /// A native payload that is not an error object (error objects are
    /// covered from source in `tests/js_source.rs`).
    #[derive(Debug)]
    struct Opaque;

    impl Trace for Opaque {
        fn trace(&self, _visit: &mut dyn FnMut(ObjectId)) {}
    }

    impl NativePayload for Opaque {
        fn shareability(&self) -> Shareability {
            Shareability::Persistent
        }
    }

    #[test]
    fn natives_other_than_error_objects_have_no_properties() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let name = JsString::from("x");
        let native = heap.alloc(HeapObject::Native(Box::new(Opaque))).unwrap();
        let native = Value::Object(native);
        assert!(matches!(
            get_field(&heap, task, &native, &name),
            Ok(Value::Undefined)
        ));
        match put_field(&heap, task, &native, &name, Value::Null) {
            Err(Fault::TypeError(m)) => assert!(m.contains("no JavaScript properties"), "{m}"),
            _ => panic!("expected a TypeError"),
        }
    }
}
