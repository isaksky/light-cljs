//! Built-in functions (clj-quickjs-117): the function objects of the realm
//! (`crate::realm`) whose behaviour is Rust code, after QuickJS's
//! `JS_CFUNC_DEF` functions (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8: `js_object_constructor`, `js_object_is`,
//! `js_object_toString`, `js_object_valueOf`); written fresh over this
//! runtime's values.
//!
//! # The heap kind
//!
//! A built-in function is a heap object of kind `HeapObject::Builtin`
//! holding a [`BuiltinFunction`]: a [`Builtin`] naming its Rust code, and an
//! intrinsic [`OrdinaryObject`] for its own properties (`length`, `name`,
//! and for a constructor `prototype` and its static methods). It is a realm
//! intrinsic, so it is shared by every task, immutable and
//! [`Shareability::Persistent`](crate::value::share::Shareability). The
//! kind is general: every later built-in (`Function.prototype.call`, the
//! `Array`, `Symbol` and error constructors and their methods) is a new
//! [`Builtin`] variant, appended, with its arm in [`call`].
//!
//! `typeof` of a built-in function is `"function"`, and `Call` and
//! `CallMethod` accept it as a callee (as do the resumable natives that call
//! back into JavaScript, such as `__rt.reduce`). Its [`Trace`] reports its
//! property values; the root inventory lists it under the realm intrinsics.
//!
//! Its `[[Prototype]]` is null until `Function.prototype` exists
//! (clj-quickjs-102); reading a property a built-in function does not have
//! itself therefore fails with `UnsupportedOperand` instead of answering
//! `undefined` for what will be an inherited method (`crate::vm::property`).
//!
//! # Calls
//!
//! A built-in runs to completion inside its `Call`/`CallMethod`
//! instruction: it reads its receiver and arguments in place on the operand
//! stack (so they stay rooted), may allocate (the call is a poll point, and
//! the result replaces the operands in the same instruction, before the next
//! safepoint), calls no JavaScript and never suspends. A later built-in that
//! must call JavaScript (a getter, a `toString` method, a callback) will
//! not run here: its call starts a resumable native frame
//! (`crate::vm::native`), so every JavaScript it runs is an ordinary VM
//! frame across which the task may yield, wait or migrate.
//!
//! The receiver is the base of a method call (`CallMethod`), `undefined`
//! for a plain call (strict mode).
//!
//! | Built-in | Result |
//! |----------|--------|
//! | `Object(v)` | `undefined`/`null`/absent: a new ordinary object owned by the running task, with prototype `Object.prototype`; an object: `v` itself; a boolean, number or string: `UnsupportedOperand` (wrapper objects are clj-quickjs-72) |
//! | `Object.is(a, b)` | SameValue: `NaN` is `NaN`, `0` is not `-0`, strings by content, objects by identity |
//! | `Object.prototype.toString()` | `"[object Undefined]"` / `"[object Null]"` for those receivers; `"[object Function]"` for a function object or built-in function; `"[object Array]"` for an array (clj-quickjs-118); `"[object Object]"` for an ordinary object and for the runtime's other heap values (which behave as objects without properties); an error object or a primitive receiver: `UnsupportedOperand` (their tags belong to clj-quickjs-230 and clj-quickjs-72/-73) |
//! | `Object.prototype.valueOf()` | ToObject(receiver): an object is itself; `undefined`/`null` throw a catchable `TypeError`; a primitive: `UnsupportedOperand` (wrappers, clj-quickjs-72) |
//! | `Array.prototype.toString()` (clj-quickjs-118) | `UnsupportedOperand` until `Array.prototype.join` exists (clj-quickjs-39), never `Object.prototype.toString`'s answer |
//! | `Array(...)` (clj-quickjs-118; reachable as `[].constructor`) | `UnsupportedOperand` until the Array constructor exists (clj-quickjs-39) |
//! | `Object.defineProperty(o, k, attributes)` | define or update the own data property `k` of `o` from the descriptor and return `o` (`crate::vm::descriptor`, clj-quickjs-119); a non-object `o` or descriptor, a descriptor with both `get`/`set` and `value`/`writable`, a define a non-configurable property forbids, a built-in object as `o`: catchable `TypeError`; an accessor descriptor: `UnsupportedOperand` (clj-quickjs-120) |
//! | `Object.getOwnPropertyDescriptor(o, k)` | `undefined` if `o` has no own property `k`, else a fresh object `{value, writable, enumerable, configurable}` (clj-quickjs-119); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: `UnsupportedOperand` (wrappers, clj-quickjs-72) |
//! | `Object.getPrototypeOf(v)` (clj-quickjs-121) | the `[[Prototype]]` of `v` (`crate::vm::prototype`), `null` at the end of a chain; `undefined`/`null` `v`: catchable `TypeError`; a primitive, a function, a built-in function or an error object: `UnsupportedOperand` (wrapper objects, `Function.prototype` and `Error.prototype` do not exist yet) |
//! | `Object.setPrototypeOf(o, proto)` (clj-quickjs-121) | set the `[[Prototype]]` of the object `o` and return `o`; a primitive `o` is returned unchanged; `undefined`/`null` `o`, a `proto` that is neither an object nor `null`, a cycle, a built-in or non-extensible `o`: catchable `TypeError` |
//! | `Object.create(proto)` (clj-quickjs-121) | a new ordinary object owned by the running task with prototype `proto` (an object or `null`, else a catchable `TypeError`); a second argument other than `undefined`: `UnsupportedOperand` (`Object.defineProperties`) |
//! | `Object.preventExtensions(v)`, `Object.freeze(v)` (clj-quickjs-122) | make `v` non-extensible / frozen and return it (`crate::vm::integrity` has the table per kind); a primitive is returned unchanged; a built-in object: catchable `TypeError` (immutable); another task's object: `ForeignObject` |
//! | `Object.isExtensible(v)`, `Object.isFrozen(v)` (clj-quickjs-122) | the `[[Extensible]]` flag / TestIntegrityLevel(frozen); a primitive is not extensible and is frozen; vectors, maps and published functions are frozen (`crate::vm::integrity`) |
//! | `Object.keys(o)`, `Object.getOwnPropertyNames(o)` (clj-quickjs-123) | a fresh array of the own enumerable (`keys`) or all (`getOwnPropertyNames`) string keys of `o`, array indices ascending first, then the other names in creation order (`crate::vm::keys`); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: `UnsupportedOperand` (wrappers, clj-quickjs-72); `getOwnPropertyNames` of a function or of `Object.prototype`: `UnsupportedOperand` (clj-quickjs-101, -261) |
//! | `Number.prototype.toString(radix)`, `.valueOf()` (clj-quickjs-73) | thisNumberValue: a number receiver (a method call on a number primitive passes the number itself, no wrapper is allocated) or `Number.prototype` (`+0`); `toString` gives its Number::toString (`crate::value::string::number_to_string`), with `radix` absent, `undefined` or 10; another radix: `UnsupportedOperand` (clj-quickjs-39); any other receiver: catchable `TypeError` |
//! | `String.prototype.toString()`, `.valueOf()` (clj-quickjs-73) | thisStringValue: a string receiver or `String.prototype` (`""`); any other receiver: catchable `TypeError` |
//! | `Number(...)`, `String(...)` (clj-quickjs-73; global names that carry the prototypes) | `UnsupportedOperand` until the constructors exist (clj-quickjs-72) |

use crate::atom::same_value;
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::OrdinaryObject;
use crate::realm;
use crate::value::string::number_to_string;
use crate::value::{JsString, Value};
use crate::vm::descriptor;
use crate::vm::dispatch::Fault;
use crate::vm::exception::ErrorObject;
use crate::vm::integrity;
use crate::vm::keys::{self, KeyFilter};
use crate::vm::prototype;

/// Which built-in function: the Rust code a [`BuiltinFunction`] runs.
/// Append-only.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Builtin {
    /// `Object` (QuickJS `js_object_constructor`), called as a function.
    Object,
    /// `Object.is` (QuickJS `js_object_is`).
    ObjectIs,
    /// `Object.prototype.toString` (QuickJS `js_object_toString`).
    ObjectPrototypeToString,
    /// `Object.prototype.valueOf` (QuickJS `js_object_valueOf`).
    ObjectPrototypeValueOf,
    /// `Array.prototype.toString` (clj-quickjs-118): fails explicitly
    /// until `Array.prototype.join` exists (clj-quickjs-39).
    ArrayPrototypeToString,
    /// `Array` (clj-quickjs-118): reachable only as
    /// `Array.prototype.constructor`; fails when called until the Array
    /// constructor exists (clj-quickjs-39).
    Array,
    /// `Object.defineProperty` (QuickJS `js_object_defineProperty`;
    /// clj-quickjs-119, `crate::vm::descriptor`).
    ObjectDefineProperty,
    /// `Object.getOwnPropertyDescriptor` (QuickJS
    /// `js_object_getOwnPropertyDescriptor`; clj-quickjs-119).
    ObjectGetOwnPropertyDescriptor,
    /// `Object.getPrototypeOf` (QuickJS `js_object_getPrototypeOf`;
    /// clj-quickjs-121, `crate::vm::prototype`).
    ObjectGetPrototypeOf,
    /// `Object.setPrototypeOf` (QuickJS `js_object_setPrototypeOf`;
    /// clj-quickjs-121).
    ObjectSetPrototypeOf,
    /// `Object.create` (QuickJS `js_object_create`; clj-quickjs-121).
    ObjectCreate,
    /// `Object.preventExtensions` (QuickJS `js_object_preventExtensions`;
    /// clj-quickjs-122, `crate::vm::integrity`).
    ObjectPreventExtensions,
    /// `Object.isExtensible` (QuickJS `js_object_isExtensible`;
    /// clj-quickjs-122).
    ObjectIsExtensible,
    /// `Object.freeze` (QuickJS `js_object_seal`, freeze flag;
    /// clj-quickjs-122).
    ObjectFreeze,
    /// `Object.isFrozen` (QuickJS `js_object_isSealed`, frozen flag;
    /// clj-quickjs-122).
    ObjectIsFrozen,
    /// `Object.keys` (QuickJS `js_object_keys`; clj-quickjs-123,
    /// `crate::vm::keys`).
    ObjectKeys,
    /// `Object.getOwnPropertyNames` (QuickJS
    /// `js_object_getOwnPropertyNames`; clj-quickjs-123).
    ObjectGetOwnPropertyNames,
    /// `Number` (clj-quickjs-73): the global that carries
    /// `Number.prototype`; fails when called until clj-quickjs-72.
    Number,
    /// `Number.prototype.toString` (QuickJS `js_number_toString`;
    /// clj-quickjs-73), radix 10 only.
    NumberPrototypeToString,
    /// `Number.prototype.valueOf` (QuickJS `js_number_valueOf`;
    /// clj-quickjs-73).
    NumberPrototypeValueOf,
    /// `String` (clj-quickjs-73): the global that carries
    /// `String.prototype`; fails when called until clj-quickjs-72.
    String,
    /// `String.prototype.toString` (QuickJS `js_string_toString`;
    /// clj-quickjs-73).
    StringPrototypeToString,
    /// `String.prototype.valueOf` (clj-quickjs-73; the same function body
    /// as `toString`).
    StringPrototypeValueOf,
}

impl Builtin {
    /// The function's `name` property.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Builtin::Object => "Object",
            Builtin::ObjectIs => "is",
            Builtin::ObjectPrototypeToString => "toString",
            Builtin::ObjectPrototypeValueOf => "valueOf",
            Builtin::ArrayPrototypeToString => "toString",
            Builtin::Array => "Array",
            Builtin::ObjectDefineProperty => "defineProperty",
            Builtin::ObjectGetOwnPropertyDescriptor => "getOwnPropertyDescriptor",
            Builtin::ObjectGetPrototypeOf => "getPrototypeOf",
            Builtin::ObjectSetPrototypeOf => "setPrototypeOf",
            Builtin::ObjectCreate => "create",
            Builtin::ObjectPreventExtensions => "preventExtensions",
            Builtin::ObjectIsExtensible => "isExtensible",
            Builtin::ObjectFreeze => "freeze",
            Builtin::ObjectIsFrozen => "isFrozen",
            Builtin::ObjectKeys => "keys",
            Builtin::ObjectGetOwnPropertyNames => "getOwnPropertyNames",
            Builtin::Number => "Number",
            Builtin::String => "String",
            Builtin::NumberPrototypeToString | Builtin::StringPrototypeToString => "toString",
            Builtin::NumberPrototypeValueOf | Builtin::StringPrototypeValueOf => "valueOf",
        }
    }

    /// The function's `length` property: its number of declared parameters.
    pub(crate) fn length(self) -> u32 {
        match self {
            Builtin::Object | Builtin::Array => 1,
            Builtin::ObjectIs => 2,
            Builtin::ObjectPrototypeToString
            | Builtin::ObjectPrototypeValueOf
            | Builtin::ArrayPrototypeToString => 0,
            Builtin::ObjectDefineProperty => 3,
            Builtin::ObjectGetOwnPropertyDescriptor => 2,
            Builtin::ObjectGetPrototypeOf => 1,
            Builtin::ObjectSetPrototypeOf | Builtin::ObjectCreate => 2,
            Builtin::ObjectPreventExtensions
            | Builtin::ObjectIsExtensible
            | Builtin::ObjectFreeze
            | Builtin::ObjectIsFrozen => 1,
            Builtin::ObjectKeys | Builtin::ObjectGetOwnPropertyNames => 1,
            Builtin::Number | Builtin::String | Builtin::NumberPrototypeToString => 1,
            Builtin::NumberPrototypeValueOf
            | Builtin::StringPrototypeToString
            | Builtin::StringPrototypeValueOf => 0,
        }
    }
}

/// Storage of a built-in function. See the module docs.
#[derive(Debug)]
pub(crate) struct BuiltinFunction {
    pub(crate) which: Builtin,
    /// Own properties: an intrinsic, immutable ordinary-object store.
    pub(crate) properties: OrdinaryObject,
}

impl Trace for BuiltinFunction {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.properties.trace(visit);
    }
}

/// Call the built-in `which` for the running task `task` with receiver
/// `this` and arguments `args` (module docs, "Calls").
pub(crate) fn call(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
) -> Result<Value, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    match which {
        Builtin::Object => match arg(0) {
            Value::Undefined | Value::Null => new_object(heap, task),
            v @ Value::Object(_) => Ok(v),
            v => Err(Fault::Unsupported(format!(
                "Object() of a {} needs a wrapper object, which is not supported",
                v.type_name()
            ))),
        },
        Builtin::ObjectIs => Ok(Value::Bool(same_value(&arg(0), &arg(1)))),
        Builtin::ObjectPrototypeToString => {
            let tag = to_string_tag(heap, this)?;
            Ok(Value::String(JsString::from(
                format!("[object {tag}]").as_str(),
            )))
        }
        Builtin::ObjectPrototypeValueOf => match this {
            Value::Undefined | Value::Null => Err(Fault::TypeError(format!(
                "cannot convert {} to object",
                this.type_name()
            ))),
            Value::Object(_) => Ok(this.clone()),
            v => Err(Fault::Unsupported(format!(
                "valueOf on a {} needs a wrapper object, which is not supported",
                v.type_name()
            ))),
        },
        Builtin::ArrayPrototypeToString => Err(Fault::Unsupported(
            "Array.prototype.toString (Array.prototype.join) is not supported".to_string(),
        )),
        Builtin::Array => Err(Fault::Unsupported(
            "the Array constructor is not supported".to_string(),
        )),
        Builtin::ObjectDefineProperty => descriptor::define_property(heap, task, args),
        Builtin::ObjectGetOwnPropertyDescriptor => {
            descriptor::get_own_property_descriptor(heap, task, args)
        }
        Builtin::ObjectGetPrototypeOf => prototype::object_get_prototype_of(heap, task, args),
        Builtin::ObjectSetPrototypeOf => prototype::object_set_prototype_of(heap, task, args),
        Builtin::ObjectCreate => prototype::object_create(heap, task, args),
        Builtin::ObjectPreventExtensions => integrity::prevent_extensions(heap, task, &arg(0)),
        Builtin::ObjectIsExtensible => integrity::is_extensible(heap, task, &arg(0)),
        Builtin::ObjectFreeze => integrity::freeze(heap, task, &arg(0)),
        Builtin::ObjectIsFrozen => integrity::is_frozen(heap, task, &arg(0)),
        Builtin::ObjectKeys => keys::object_keys(heap, task, args, KeyFilter::Enumerable),
        Builtin::ObjectGetOwnPropertyNames => keys::object_keys(heap, task, args, KeyFilter::All),
        Builtin::Number | Builtin::String => Err(Fault::Unsupported(format!(
            "calling the {} constructor is not supported",
            which.name()
        ))),
        Builtin::NumberPrototypeToString => {
            let n = this_number_value(heap, this, "toString")?;
            match arg(0) {
                Value::Undefined => {}
                Value::Number(10.0) => {}
                _ => {
                    return Err(Fault::Unsupported(
                        "Number.prototype.toString with a radix other than 10 is not supported"
                            .to_string(),
                    ));
                }
            }
            Ok(Value::String(JsString::from(number_to_string(n).as_str())))
        }
        Builtin::NumberPrototypeValueOf => {
            this_number_value(heap, this, "valueOf").map(Value::Number)
        }
        Builtin::StringPrototypeToString | Builtin::StringPrototypeValueOf => {
            this_string_value(heap, this, which.name()).map(Value::String)
        }
    }
}

/// thisNumberValue (ECMAScript §21.1.3; QuickJS `js_thisNumberValue`): a
/// number receiver is itself; `Number.prototype`, a Number object holding
/// `+0` in ECMAScript, is `0`. Number wrapper objects are clj-quickjs-72;
/// any other receiver is a catchable `TypeError`.
fn this_number_value(heap: &Heap, this: &Value, method: &str) -> Result<f64, Fault> {
    match this {
        Value::Number(n) => Ok(*n),
        Value::Object(id) if is_intrinsic(heap, *id, |r| r.number_prototype) => Ok(0.0),
        other => Err(Fault::TypeError(format!(
            "Number.prototype.{method} requires that 'this' be a Number, not {}",
            describe_receiver(other)
        ))),
    }
}

/// thisStringValue (ECMAScript §22.1.3; QuickJS `js_thisStringValue`): a
/// string receiver is itself; `String.prototype` (a String object holding
/// `""`) is `""`. String wrapper objects are clj-quickjs-72; any other
/// receiver is a catchable `TypeError`.
fn this_string_value(heap: &Heap, this: &Value, method: &str) -> Result<JsString, Fault> {
    match this {
        Value::String(s) => Ok(s.clone()),
        Value::Object(id) if is_intrinsic(heap, *id, |r| r.string_prototype) => {
            Ok(JsString::from(""))
        }
        other => Err(Fault::TypeError(format!(
            "String.prototype.{method} requires that 'this' be a String, not {}",
            describe_receiver(other)
        ))),
    }
}

/// True when `id` is the realm intrinsic `which` selects.
fn is_intrinsic(heap: &Heap, id: ObjectId, which: fn(&realm::Intrinsics) -> ObjectId) -> bool {
    heap.realm().get().is_some_and(|r| which(r) == id)
}

/// A receiver's kind for a thisNumberValue/thisStringValue message.
fn describe_receiver(v: &Value) -> &'static str {
    match v {
        Value::Undefined => "undefined",
        Value::Null => "null",
        Value::Bool(_) => "a boolean",
        Value::Number(_) => "a number",
        Value::String(_) => "a string",
        Value::Object(_) => "an object",
    }
}

/// A new ordinary object owned by `task` whose prototype is
/// `Object.prototype` (OrdinaryObjectCreate(%Object.prototype%)).
pub(crate) fn new_object(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .object_prototype;
    let object = OrdinaryObject::with_prototype(task, prototype);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// The builtinTag of `Object.prototype.toString` (ECMAScript
/// Object.prototype.toString steps 1-14; no `@@toStringTag` while symbols
/// do not exist).
fn to_string_tag(heap: &Heap, this: &Value) -> Result<&'static str, Fault> {
    let unsupported = |what: &str| {
        Err(Fault::Unsupported(format!(
            "Object.prototype.toString of {what} is not supported"
        )))
    };
    match this {
        Value::Undefined => Ok("Undefined"),
        Value::Null => Ok("Null"),
        Value::Bool(_) => unsupported("a boolean"),
        Value::Number(_) => unsupported("a number"),
        Value::String(_) => unsupported("a string"),
        Value::Object(id) => heap
            .read(*id, |object| match object {
                HeapObject::Function(_) | HeapObject::Builtin(_) => Ok("Function"),
                HeapObject::Array(_) => Ok("Array"),
                HeapObject::Native(payload) => {
                    let any: &dyn std::any::Any = &**payload;
                    if any.is::<ErrorObject>() {
                        unsupported("an error object")
                    } else {
                        Ok("Object")
                    }
                }
                HeapObject::Plain(_)
                | HeapObject::Vector(_)
                | HeapObject::Map(_)
                | HeapObject::Occurrence(_)
                | HeapObject::Builder(_)
                | HeapObject::Cell(_)
                | HeapObject::Atom(_) => Ok("Object"),
            })
            // The receiver is an operand of the running frame.
            .unwrap_or(Err(Fault::Internal("receiver was reclaimed"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag(heap: &Heap, this: &Value) -> String {
        match call(
            heap,
            TaskId::from_raw(1),
            Builtin::ObjectPrototypeToString,
            this,
            &[],
        ) {
            Ok(Value::String(s)) => s.to_string(),
            Ok(other) => panic!("not a string: {other:?}"),
            Err(_) => "unsupported".to_string(),
        }
    }

    /// The receivers source code cannot pass yet (`Function.prototype.call`
    /// is clj-quickjs-102).
    #[test]
    fn to_string_tags_of_other_receivers() {
        let heap = Heap::default();
        let object = Value::Object(realm::intrinsics(&heap).unwrap().object);
        assert_eq!(tag(&heap, &object), "[object Function]");
        let vector = crate::collections::new_vector(&heap, &[]).unwrap();
        assert_eq!(tag(&heap, &vector), "[object Object]");
        for primitive in [
            Value::Bool(true),
            Value::Number(1.0),
            Value::String(JsString::from("s")),
        ] {
            assert_eq!(tag(&heap, &primitive), "unsupported");
        }
        assert_eq!(tag(&heap, &Value::Null), "[object Null]");
    }
}
