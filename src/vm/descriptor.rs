//! Property descriptors (clj-quickjs-119): `Object.defineProperty` and
//! `Object.getOwnPropertyDescriptor`, after QuickJS's
//! `js_object_defineProperty`, `js_object_getOwnPropertyDescriptor` and
//! `js_obj_to_desc` (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8);
//! written fresh over this runtime's object model (`crate::object`) and
//! property access (`crate::vm::property`).
//!
//! # Conversions
//!
//! * **ToPropertyKey** of the key argument: resumable ToPrimitive with
//!   string hint for objects, then `crate::vm::property::to_property_key`
//!   (the ToString of a primitive, `1` and `"1"` name one property).
//!   Symbols do not exist yet (clj-quickjs-127).
//! * **ToPropertyDescriptor** of the attributes argument: a non-object is a
//!   TypeError; otherwise the fields `enumerable`, `configurable`, `value`,
//!   `writable`, `get` and `set` are read in that order with `[[Get]]`
//!   (`crate::vm::property::find`), so inherited fields count and a
//!   field present with the value `undefined` is present. The booleans
//!   take ToBoolean. A `get` or `set` that is neither `undefined` nor
//!   callable is a TypeError, and so is a descriptor with both `get`/`set`
//!   and `value`/`writable`. `DefineProperty` retains the descriptor cursor
//!   and accumulated fields in a traced native frame; each getter runs as
//!   an ordinary VM callback and fields are never replayed across waits.
//! * **FromPropertyDescriptor**: a fresh ordinary object
//!   owned by the running task, with prototype `Object.prototype` and the
//!   own properties `value`, `writable`, `enumerable`, `configurable`, in
//!   that order (or `get`, `set`, `enumerable`, `configurable` for an
//!   accessor), each writable, enumerable and configurable.
//!
//! Object-valued keys use resumable ToPrimitive first. Descriptor
//! reflection allocates its result last, with every property value
//! already in the object being allocated; it never invokes the accessor
//! being reflected.

use crate::heap::{Heap, HeapObject};
use crate::heap::{ObjectId, gc::Trace};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, Property, PropertyDescriptor, PropertyFlags};
use crate::realm;
use crate::value::{JsString, Value};
use crate::vm::dispatch::{self, Fault};
use crate::vm::property::{self, Key};

/// `Object.defineProperty(target, key, attributes)`: define the property
/// and return `target`.
#[cfg(test)]
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
    // ToObject. A primitive's wrapper is not allocated: a String wrapper's
    // own properties are its `length` and indices
    // (`property::string_own_property`, clj-quickjs-142), and Number and
    // Boolean wrappers have none.
    let target = arg(0);
    if let v @ (Value::Undefined | Value::Null) = &target {
        return Err(Fault::TypeError(format!(
            "cannot convert {} to object",
            v.type_name()
        )));
    }
    let key = property::to_property_key(&arg(1))?;
    let own = match &target {
        Value::Object(id) => property::get_own_property(heap, task, *id, &key)?,
        Value::String(s) => property::string_own_property(s, &key),
        _ => None,
    };
    match own {
        Some(property) => from_property_descriptor(heap, task, property),
        None => Ok(Value::Undefined),
    }
}

/// ToPropertyDescriptor (module docs, "Conversions").
#[cfg(test)]
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
    let mut fields = match property.accessor {
        Some((get, set)) => vec![("get", get), ("set", set)],
        None => vec![
            ("value", property.value),
            ("writable", Value::Bool(flags.writable)),
        ],
    };
    fields.extend([
        ("enumerable", Value::Bool(flags.enumerable)),
        ("configurable", Value::Bool(flags.configurable)),
    ]);
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

/// ToPropertyDescriptor's field cursor and accumulated values survive each
/// getter call. No field is re-read on resumption.
#[derive(Debug)]
pub(crate) struct DefineProperty {
    target: Value,
    key: Key,
    attributes: Value,
    desc: PropertyDescriptor,
    next: usize,
}

pub(crate) enum DefineStep {
    Getter {
        callee: Value,
        this: Value,
    },
    TypedArrayValue {
        target: Value,
        index: f64,
        value: Value,
    },
    Done(Value),
}

impl Trace for DefineProperty {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.target.trace(visit);
        self.attributes.trace(visit);
        for v in [&self.desc.value, &self.desc.get, &self.desc.set]
            .into_iter()
            .flatten()
        {
            v.trace(visit);
        }
    }
}

impl DefineProperty {
    pub(crate) fn new(args: &[Value]) -> Result<Self, Fault> {
        let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
        let target = arg(0);
        if !matches!(target, Value::Object(_)) {
            return Err(Fault::TypeError(
                "Object.defineProperty target must be an object".into(),
            ));
        }
        let key = property::to_property_key(&arg(1))?;
        let attributes = arg(2);
        if !matches!(attributes, Value::Object(_)) {
            return Err(Fault::TypeError(
                "a property descriptor must be an object".into(),
            ));
        }
        Ok(Self {
            target,
            key,
            attributes,
            desc: PropertyDescriptor::default(),
            next: 0,
        })
    }

    fn field(&mut self, heap: &Heap, value: Value) -> Result<(), Fault> {
        match self.next {
            0 => self.desc.enumerable = Some(value.truthy()),
            1 => self.desc.configurable = Some(value.truthy()),
            2 => self.desc.value = Some(value),
            3 => self.desc.writable = Some(value.truthy()),
            4 | 5 => {
                if value != Value::Undefined && dispatch::callee(heap, &value).is_err() {
                    return Err(Fault::TypeError(
                        "property descriptor get/set must be callable or undefined".into(),
                    ));
                }
                if self.next == 4 {
                    self.desc.get = Some(value);
                } else {
                    self.desc.set = Some(value);
                }
            }
            _ => return Err(Fault::Internal("descriptor field cursor out of range")),
        }
        self.next += 1;
        Ok(())
    }

    pub(crate) fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        result: Option<Value>,
    ) -> Result<DefineStep, Fault> {
        if let Some(value) = result {
            self.field(heap, value)?;
        }
        let names = [
            "enumerable",
            "configurable",
            "value",
            "writable",
            "get",
            "set",
        ];
        while self.next < names.len() {
            let key = Key::Name(JsString::from(names[self.next]));
            match property::read(heap, task, &self.attributes, &key)? {
                None => self.next += 1,
                Some(property::Read::Value(value)) => self.field(heap, value)?,
                Some(property::Read::Getter(callee)) => {
                    return Ok(DefineStep::Getter {
                        callee,
                        this: self.attributes.clone(),
                    });
                }
            }
        }
        if self.desc.is_accessor() && self.desc.is_data() {
            return Err(Fault::TypeError(
                "a property descriptor cannot have both get/set and value/writable".into(),
            ));
        }
        let Value::Object(id) = self.target else {
            unreachable!()
        };
        if let Some(value @ Value::Object(_)) = &self.desc.value
            && let Some((_, index)) =
                property::typed_array_descriptor(heap, task, id, &self.key, &self.desc)?
        {
            return Ok(DefineStep::TypedArrayValue {
                target: self.target.clone(),
                index,
                value: value.clone(),
            });
        }
        property::define_own_property(heap, task, id, &self.key, &self.desc)?;
        Ok(DefineStep::Done(self.target.clone()))
    }
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
    /// one (from source: `prototype_reflection` in
    /// `tests/js_source.rs`).
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
