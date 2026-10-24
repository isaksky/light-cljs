//! Property descriptors (clj-quickjs-119): `Object.defineProperty` and
//! `Object.getOwnPropertyDescriptor`, after QuickJS's
//! `js_object_defineProperty`, `js_object_getOwnPropertyDescriptor` and
//! `js_obj_to_desc` (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8);
//! written fresh over this runtime's object model (`crate::object`) and
//! property access (`crate::vm::property`).
//!
//! # Conversions
//!
//! * **ToPropertyKey** of the key argument: `crate::vm::property::to_property_key`
//!   (the ToString of a primitive, `1` and `"1"` name one property); an object
//!   needs ToPrimitive (clj-quickjs-70) and fails with `UnsupportedOperand`.
//!   Symbols do not exist yet (clj-quickjs-127).
//! * **ToPropertyDescriptor** of the attributes argument: a non-object is a
//!   TypeError; otherwise the fields `enumerable`, `configurable`, `value`,
//!   `writable`, `get` and `set` are read in that order with `[[Get]]`
//!   (`crate::vm::property::find`), so inherited fields count and a
//!   field present with the value `undefined` is present. The booleans
//!   take ToBoolean. A `get` or `set` that is neither `undefined` nor
//!   callable is a TypeError, and so is a descriptor with both `get`/`set`
//!   and `value`/`writable`. No field read runs JavaScript today; once
//!   accessors exist (clj-quickjs-120) a read may run a getter, and these
//!   built-ins then become resumable natives (`crate::vm::native`) so the
//!   getter is an ordinary VM frame.
//! * **FromPropertyDescriptor** of a data property: a fresh ordinary object
//!   owned by the running task, with prototype `Object.prototype` and the
//!   own properties `value`, `writable`, `enumerable`, `configurable`, in
//!   that order, each writable, enumerable and configurable.
//!
//! Both built-ins run to completion inside their call instruction
//! (`crate::vm::builtin`, "Calls"): their operands stay on the operand
//! stack, and `getOwnPropertyDescriptor` allocates its result last, with
//! every property value already in the object being allocated.

use crate::heap::{Heap, HeapObject};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, Property, PropertyDescriptor, PropertyFlags};
use crate::realm;
use crate::value::{JsString, Value};
use crate::vm::dispatch::{self, Fault};
use crate::vm::property::{self, Key};

/// `Object.defineProperty(target, key, attributes)`: define the property
/// and return `target`.
pub(crate) fn define_property(heap: &Heap, task: TaskId, args: &[Value]) -> Result<Value, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    let target = arg(0);
    let Value::Object(id) = target else {
        return Err(Fault::TypeError(format!(
            "Object.defineProperty called on a {}, not an object",
            target.type_name()
        )));
    };
    let key = property::to_property_key(&arg(1))?;
    let desc = to_property_descriptor(heap, task, &arg(2))?;
    property::define_own_property(heap, task, id, &key, &desc)?;
    Ok(target)
}

/// `Object.getOwnPropertyDescriptor(target, key)`: the descriptor of the
/// own property, or `undefined` if there is none.
pub(crate) fn get_own_property_descriptor(
    heap: &Heap,
    task: TaskId,
    args: &[Value],
) -> Result<Value, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    // ToObject.
    let id = match arg(0) {
        Value::Object(id) => id,
        v @ (Value::Undefined | Value::Null) => {
            return Err(Fault::TypeError(format!(
                "cannot convert {} to object",
                v.type_name()
            )));
        }
        v => {
            return Err(Fault::Unsupported(format!(
                "Object.getOwnPropertyDescriptor of a {} needs a wrapper object, which is not supported",
                v.type_name()
            )));
        }
    };
    let key = property::to_property_key(&arg(1))?;
    match property::get_own_property(heap, task, id, &key)? {
        Some(property) => from_property_descriptor(heap, task, property),
        None => Ok(Value::Undefined),
    }
}

/// ToPropertyDescriptor (module docs, "Conversions").
fn to_property_descriptor(
    heap: &Heap,
    task: TaskId,
    attributes: &Value,
) -> Result<PropertyDescriptor, Fault> {
    if !matches!(attributes, Value::Object(_)) {
        return Err(Fault::TypeError(format!(
            "a property descriptor must be an object, got {}",
            attributes.type_name()
        )));
    }
    let field = |name: &str| {
        let key = Key::Name(JsString::from(name));
        property::find(heap, task, attributes, &key)
    };
    let accessor = |name: &str| -> Result<Option<Value>, Fault> {
        let value = field(name)?;
        match &value {
            Some(f @ Value::Object(_)) if dispatch::callee(heap, f).is_ok() => {}
            Some(Value::Undefined) | None => {}
            Some(other) => {
                return Err(Fault::TypeError(format!(
                    "property descriptor field '{name}' must be a function or undefined, got {}",
                    other.type_name()
                )));
            }
        }
        Ok(value)
    };
    let desc = PropertyDescriptor {
        enumerable: field("enumerable")?.map(|v| v.truthy()),
        configurable: field("configurable")?.map(|v| v.truthy()),
        value: field("value")?,
        writable: field("writable")?.map(|v| v.truthy()),
        get: accessor("get")?,
        set: accessor("set")?,
    };
    if desc.is_accessor() && desc.is_data() {
        return Err(Fault::TypeError(
            "a property descriptor cannot have both get/set and value/writable".to_string(),
        ));
    }
    Ok(desc)
}

/// FromPropertyDescriptor of a data property (module docs, "Conversions").
fn from_property_descriptor(heap: &Heap, task: TaskId, property: Property) -> Result<Value, Fault> {
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .object_prototype;
    let mut object = OrdinaryObject::with_prototype(task, prototype);
    let flags = property.flags;
    let fields = [
        ("value", property.value),
        ("writable", Value::Bool(flags.writable)),
        ("enumerable", Value::Bool(flags.enumerable)),
        ("configurable", Value::Bool(flags.configurable)),
    ];
    for (name, value) in fields {
        let key = heap
            .keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
        if !object.define_with(key, value, PropertyFlags::DATA) {
            return Err(Fault::Internal("a fresh object is not extensible"));
        }
    }
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::object::PropertyKeys;

    fn ok<T>(result: Result<T, Fault>) -> T {
        result.unwrap_or_else(|_| panic!("unexpected fault"))
    }

    fn key(keys: &PropertyKeys, name: &str) -> crate::object::PropertyKey {
        keys.intern(&JsString::from(name)).unwrap()
    }

    /// ToPropertyDescriptor reads fields with `[[Get]]`: inherited fields
    /// count, and an own field holding `undefined` shadows an inherited
    /// one. Source code cannot build the prototype link until
    /// `Object.create` (clj-quickjs-121).
    #[test]
    fn to_property_descriptor_reads_inherited_fields() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let mut base = OrdinaryObject::new(Some(task));
        base.define(key(heap.keys(), "value"), Value::Number(7.0));
        base.define(key(heap.keys(), "writable"), Value::Bool(true));
        base.define(key(heap.keys(), "enumerable"), Value::Number(1.0));
        let base = heap.alloc(HeapObject::Plain(base)).unwrap();
        let inheriting = OrdinaryObject::with_prototype(task, base);
        let inheriting = Value::Object(heap.alloc(HeapObject::Plain(inheriting)).unwrap());
        let mut shadowing = OrdinaryObject::with_prototype(task, base);
        shadowing.define(key(heap.keys(), "value"), Value::Undefined);
        let shadowing = Value::Object(heap.alloc(HeapObject::Plain(shadowing)).unwrap());

        let desc = ok(to_property_descriptor(&heap, task, &inheriting));
        assert_eq!(desc.value, Some(Value::Number(7.0)));
        assert_eq!(
            (desc.writable, desc.enumerable, desc.configurable),
            (Some(true), Some(true), None)
        );
        assert!(desc.get.is_none() && desc.set.is_none());
        let desc = ok(to_property_descriptor(&heap, task, &shadowing));
        assert_eq!(desc.value, Some(Value::Undefined));
        assert_eq!(desc.writable, Some(true));

        // Through `Object.defineProperty`.
        let target = OrdinaryObject::new(Some(task));
        let target = Value::Object(heap.alloc(HeapObject::Plain(target)).unwrap());
        let name = Value::String(JsString::from("k"));
        for (desc, expected) in [
            (&inheriting, Value::Number(7.0)),
            (&shadowing, Value::Undefined),
        ] {
            let args = [target.clone(), name.clone(), desc.clone()];
            assert!(ok(define_property(&heap, task, &args)).same_object(&target));
            let Value::Object(id) = target else {
                unreachable!()
            };
            let key = Key::Name(JsString::from("k"));
            let property = ok(property::get_own_property(&heap, task, id, &key)).unwrap();
            assert_eq!(property.value, expected);
            assert!(property.flags.writable && property.flags.enumerable);
            assert!(!property.flags.configurable);
            // Writable but not configurable: the second round redefines it.
        }
    }
}
