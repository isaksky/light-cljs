//! Property access: the named `GetField`, `PutField` and `DefineField`
//! instructions (clj-quickjs-114), after QuickJS's `get_field`, `put_field`
//! and `define_field` opcodes; the computed `GetElem` and `PutElem`
//! (clj-quickjs-118), after `get_array_el` and `put_array_el`; and the array
//! literal instructions `NewArray`, `AppendElement` and `AppendHole`
//! (clj-quickjs-118), after `array_from` and `append`
//! (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8:
//! `JS_GetPropertyInternal`, `JS_SetPropertyInternal`, `JS_ToPropertyKey`,
//! `js_create_array`). Written fresh over this runtime's object model
//! (`crate::object`, `crate::object::array`).
//!
//! # Which values have properties
//!
//! | Base | Read `b.x` / `b[k]` | Write `b.x = v` / `b[k] = v` |
//! |------|-----------|-----------------|
//! | ordinary object owned by the running task | own property, else the prototype chain, else `undefined` | update an own writable data property; otherwise add one, unless the property is inherited read-only (`TypeError`) |
//! | array owned by the running task | an index: the element, else (a hole) the prototype chain; `length`; any other name as an ordinary object | an index: update the element, or (after the prototype-chain check) add it, extending `length`; `length`: `UnsupportedOperand` (clj-quickjs-39); any other name as an ordinary object |
//! | ordinary object created by the embedder (no owner) | as above | `ForeignObject` failure |
//! | ordinary object or array owned by another task | `ForeignObject` failure | `ForeignObject` failure |
//! | realm intrinsic `Object.prototype`, `Array.prototype` (`crate::realm`) | as above, from every task | catchable `TypeError`: built-in objects are shared and immutable |
//! | built-in function (`Object`, `Object.is`, ...; `crate::vm::builtin`) | own property; any other name `UnsupportedOperand` (inherited from `Function.prototype`, clj-quickjs-102) | catchable `TypeError`, as above |
//! | persistent vector, map, occurrence; builder, atom, binding cell; native payload other than an error object | `undefined` (no JavaScript properties) | catchable `TypeError` |
//! | function object (`crate::vm::function`, clj-quickjs-125) | its property record, after the lazy `prototype`; a name `Function.prototype` will own `UnsupportedOperand`; any other name continues at `Object.prototype` | the creating task writes its property record (`ForeignObject` for another task); a published function: catchable `TypeError` |
//! | error object (a caught run-time error) | `UnsupportedOperand` (`message`/`name` are clj-quickjs-230) | `UnsupportedOperand` |
//! | `undefined`, `null` | catchable `TypeError` | catchable `TypeError` |
//! | number, boolean, string | `UnsupportedOperand` (primitive wrappers are clj-quickjs-73) | `UnsupportedOperand` |
//!
//! The non-ordinary heap kinds are this runtime's own values (or values
//! JavaScript code never holds), not JavaScript objects with hidden
//! properties: they behave like objects with no own properties, a null
//! prototype and no extensibility, so a read finds nothing and a write is
//! the strict-mode TypeError of an object that cannot take the property.
//! In particular a persistent vector (`__rt.vector(...)`) is not a
//! JavaScript array: `v[0]` and `v.length` read `undefined`.
//! Function objects and error objects do have JavaScript properties
//! (`length`, `name`, `message`, ...); until those exist, touching them is
//! rejected explicitly instead of answering `undefined`.
//!
//! **Writes and the prototype chain** (clj-quickjs-117, OrdinarySet). A
//! write to a property the receiver has itself updates it (a read-only one
//! is a `TypeError`). Otherwise the chain is walked: the first object on it
//! that has the property decides; if that property is read-only the write
//! is a `TypeError`, else (or if no object has it) the receiver gets a new
//! own data property (a non-extensible receiver is a `TypeError`). The
//! realm's intrinsic properties are writable, so `o.toString = f` gives `o`
//! its own `toString`. The one accessor so far, `Object.prototype.__proto__`
//! (clj-quickjs-121, `crate::vm::prototype`), is applied where a read or
//! write of `__proto__` reaches `Object.prototype`; a function on the chain
//! (`Object.create(f)`) is walked through its property record
//! (clj-quickjs-125). Setters (clj-quickjs-120) will take part in the same
//! walk. An array element is written the same way: a hole or an index past
//! the end consults the chain from `Array.prototype` before the element is
//! added.
//!
//! Each read-only or non-extensible `TypeError` above is ECMAScript's
//! `[[Set]]` returning false (clj-quickjs-119): it is thrown in strict code
//! only; sloppy code (the running code's `Code::strict`, clj-quickjs-95)
//! ignores the write. A write to a realm intrinsic is this runtime's sharing
//! rule, not a `[[Set]]` result, and throws in both modes.
//!
//! **Property attributes** (clj-quickjs-119): [`define_own_property`] and
//! [`get_own_property`] are `[[DefineOwnProperty]]` and
//! `[[GetOwnProperty]]` for `Object.defineProperty` and
//! `Object.getOwnPropertyDescriptor` (`crate::vm::descriptor`), with the
//! same owner and kind rules as the table above. Array elements and
//! `length` keep their attributes in the array (`crate::object::array`): a
//! read-only element rejects writes like a read-only named property, and a
//! read-only `length` rejects new elements at or beyond it; a define that
//! would change `length` fails with `UnsupportedOperand` (clj-quickjs-39).
//!
//! # Property keys
//!
//! A computed key goes through ToPropertyKey ([`to_property_key`]) when the
//! `GetElem`/`PutElem` runs, so for a write after the right-hand side: a
//! string is the key as is; a number is its Number::toString (`1` and `"1"`
//! name the same property, `-0` is `"0"`, `1.5` is `"1.5"`); `true`,
//! `false`, `null` and `undefined` are their ToString. An object key needs
//! ToPrimitive (clj-quickjs-70) and fails with `UnsupportedOperand`. A key
//! that is an array index ([`crate::object::array::array_index`]) is
//! carried as the integer, so `a[1]` on an array never builds a string; an
//! ordinary object (or the chain behind a hole) looks it up by its
//! canonical name.
//!
//! No operation here allocates a heap object or runs JavaScript, except
//! `NewArray` and an access to a function object, which may allocate its
//! property record and lazy `prototype` (`crate::vm::function`), so
//! `GetField`, `PutField`, `GetElem` and `PutElem` are poll points. Each
//! object visited is read or changed under one short object-table lock (a
//! realm intrinsic is read from the realm's immutable view instead, with no
//! lock: `crate::realm`, "Lock-free reads"), and the property-key table is
//! consulted before, never while that lock is held.

use std::fmt;

use crate::collections::object_name;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::array::{ArrayObject, array_index, number_index};
use crate::object::{
    DefineOutcome, OrdinaryObject, Property, PropertyDescriptor, PropertyKey, SetOutcome,
};
use crate::realm;
use crate::value::string::number_to_string;
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::exception::ErrorObject;
use crate::vm::function::{self, FunctionRead};
use crate::vm::prototype;

/// A property key after ToPropertyKey: an array index, or any other name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Key {
    /// An integer `0 ..= 2^32 - 2`.
    Index(u32),
    /// A name that is not the canonical string of an array index.
    Name(JsString),
}

impl Key {
    /// The key a property name denotes.
    pub(crate) fn from_name(name: &JsString) -> Key {
        match array_index(name) {
            Some(index) => Key::Index(index),
            None => Key::Name(name.clone()),
        }
    }

    /// The key as a property name (an index as its decimal digits).
    fn to_name(&self) -> JsString {
        match self {
            Key::Index(index) => JsString::from(index.to_string().as_str()),
            Key::Name(name) => name.clone(),
        }
    }

    fn is_length(&self) -> bool {
        matches!(self, Key::Name(name) if *name == "length")
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Index(index) => write!(f, "{index}"),
            Key::Name(name) => write!(f, "{name}"),
        }
    }
}

/// ToPropertyKey of a computed key (module docs, "Property keys").
pub(crate) fn to_property_key(key: &Value) -> Result<Key, Fault> {
    Ok(match key {
        Value::String(s) => Key::from_name(s),
        Value::Number(n) => match number_index(*n) {
            Some(index) => Key::Index(index),
            None => Key::Name(JsString::from(number_to_string(*n).as_str())),
        },
        Value::Bool(b) => Key::Name(JsString::from(if *b { "true" } else { "false" })),
        Value::Null => Key::Name(JsString::from("null")),
        Value::Undefined => Key::Name(JsString::from("undefined")),
        Value::Object(_) => {
            return Err(Fault::Unsupported(
                "a property key that is an object needs ToPrimitive, which is not supported"
                    .to_string(),
            ));
        }
    })
}

/// The key of a computed access `base[key]` (`GetElem`, `PutElem`): an
/// `undefined`/`null` base is rejected first, with its catchable
/// `TypeError`, before the key is converted (ECMAScript ToObject of the
/// base precedes ToPropertyKey; QuickJS `JS_GetPropertyValue`), so
/// `null[{}]` is a `TypeError`, not the `UnsupportedOperand` of an object
/// key.
pub(crate) fn element_key(base: &Value, key: &Value, write: bool) -> Result<Key, Fault> {
    let access = if write { Access::Write } else { Access::Read };
    match (base, to_property_key(key)) {
        (Value::Undefined | Value::Null, Ok(key)) => Err(primitive_base(base, &key, access)),
        (Value::Undefined | Value::Null, Err(_)) => Err(Fault::TypeError(format!(
            "cannot {} a property of {}",
            access.verb(),
            base.type_name()
        ))),
        (_, key) => key,
    }
}

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

/// The failure of an access to property `key` of a primitive `base`.
fn primitive_base(base: &Value, key: &Key, access: Access) -> Fault {
    let verb = access.verb();
    match base {
        Value::Undefined | Value::Null => Fault::TypeError(format!(
            "cannot {verb} property '{key}' of {}",
            base.type_name()
        )),
        _ => Fault::Unsupported(format!(
            "cannot {verb} property '{key}' of a {}: properties of primitive values are not supported",
            base.type_name()
        )),
    }
}

/// The failure of an access to a heap object that has no ordinary property
/// store, or `None` when the access proceeds as on an object with no
/// properties (module docs).
fn not_ordinary(object: &HeapObject, key: &Key, access: Access) -> Option<Fault> {
    let verb = access.verb();
    let unsupported = |what: &str| {
        Some(Fault::Unsupported(format!(
            "cannot {verb} property '{key}' of {what}: its properties are not supported"
        )))
    };
    match object {
        HeapObject::Native(payload) => {
            let any: &dyn std::any::Any = &**payload;
            if any.is::<ErrorObject>() {
                return unsupported("an error object");
            }
            no_properties(object, key, access)
        }
        _ => no_properties(object, key, access),
    }
}

fn no_properties(object: &HeapObject, key: &Key, access: Access) -> Option<Fault> {
    match access {
        Access::Read => None,
        Access::Write => Some(Fault::TypeError(format!(
            "cannot set property '{key}' of a {}: it has no JavaScript properties",
            object_name(object)
        ))),
    }
}

/// The `ForeignObject` failure of `caller` touching an object of `owner`.
fn foreign(owner: Option<TaskId>, caller: TaskId) -> Fault {
    Fault::ForeignObject { owner, caller }
}

/// The failure of a write to a realm intrinsic (`crate::realm`).
fn immutable(key: &Key) -> Fault {
    Fault::TypeError(format!(
        "cannot set property '{key}' of a built-in object: built-in objects are shared by all tasks and immutable"
    ))
}

fn read_only(key: &Key) -> Fault {
    Fault::TypeError(format!("cannot assign to read-only property '{key}'"))
}

fn not_extensible(key: &Key) -> Fault {
    Fault::TypeError(format!(
        "cannot add property '{key}': the object is not extensible"
    ))
}

/// A built-in function has no property `key` of its own: it would inherit
/// it from `Function.prototype`, which does not exist yet.
fn function_prototype_miss(key: &Key, access: Access) -> Fault {
    Fault::Unsupported(format!(
        "cannot {} property '{key}' of a built-in function: properties inherited from Function.prototype are not supported",
        access.verb()
    ))
}

/// Writes: the owner task only.
fn check_write(object: &OrdinaryObject, task: TaskId) -> Result<(), Fault> {
    match object.owner() {
        Some(owner) if owner == task => Ok(()),
        owner => Err(foreign(owner, task)),
    }
}

/// One object visited by a property access.
enum Visit<'a> {
    /// An ordinary object, or a built-in function's property store, that
    /// the running task may read.
    Store(&'a OrdinaryObject, bool),
    /// An array the running task may read.
    Array(&'a ArrayObject),
    /// A kind without an ordinary property store.
    Other(&'a HeapObject),
}

/// Classify `object` for a read by `task` (owner check included). The flag
/// is true for a built-in function.
fn visit<'a>(object: &'a HeapObject, task: TaskId) -> Result<Visit<'a>, Fault> {
    let readable = |o: &OrdinaryObject| match o.owner() {
        Some(owner) if owner != task => Err(foreign(Some(owner), task)),
        _ => Ok(()),
    };
    match object {
        HeapObject::Plain(o) => readable(o).map(|()| Visit::Store(o, false)),
        HeapObject::Array(a) => readable(a.object()).map(|()| Visit::Array(a)),
        HeapObject::Builtin(f) => Ok(Visit::Store(&f.properties, true)),
        other => Ok(Visit::Other(other)),
    }
}

/// The interned key of a property name: `None` before it was looked up (an
/// index key, until an ordinary store needs its name), `Some(None)` for a
/// name that was never interned and so is no object's property.
type Interned = Option<Option<PropertyKey>>;

/// One step of a property read.
enum Lookup {
    Found(Value),
    /// Not an own property: continue at the prototype (or stop at null).
    Next(Option<ObjectId>),
    /// An index key reached an ordinary store: look its name up in the key
    /// table (outside the object lock) and visit the object again.
    NeedsName,
    /// A function object: its own properties decide
    /// (`crate::vm::function::read`).
    Function,
}

/// The own named property of the store `o` (a built-in function's when
/// `builtin`), or where to continue.
fn read_store(
    o: &OrdinaryObject,
    builtin: bool,
    key: &Key,
    interned: Interned,
) -> Result<Lookup, Fault> {
    let Some(k) = interned else {
        return Ok(Lookup::NeedsName);
    };
    match k.and_then(|k| o.get_own(k)) {
        Some(property) => Ok(Lookup::Found(property.value.clone())),
        None if builtin => Err(function_prototype_miss(key, Access::Read)),
        None => Ok(Lookup::Next(o.prototype())),
    }
}

/// `base[key]` for the running task `task` (`[[Get]]`; QuickJS `get_field`
/// and `get_array_el`).
pub(crate) fn get(heap: &Heap, task: TaskId, base: &Value, key: &Key) -> Result<Value, Fault> {
    find(heap, task, base, key).map(|v| v.unwrap_or(Value::Undefined))
}

/// [`get`] that tells an absent property (`None`: neither the object nor
/// its prototype chain has it) from one whose value is `undefined`
/// (HasProperty then Get, as ToPropertyDescriptor reads a descriptor;
/// clj-quickjs-119).
pub(crate) fn find(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
) -> Result<Option<Value>, Fault> {
    let Value::Object(mut id) = *base else {
        return Err(primitive_base(base, key, Access::Read));
    };
    let receiver = id;
    // A name that was never interned is no object's property; the owner
    // and kind checks below still apply.
    let mut interned: Interned = match key {
        Key::Name(name) => Some(heap.keys().lookup(name)),
        Key::Index(_) => None,
    };
    let mut realm = heap.realm().get();
    loop {
        // An intrinsic is read from the realm's immutable view, without the
        // object-table lock (`crate::realm`, "Lock-free reads").
        if let Some(view) = realm.and_then(|r| r.view(id)) {
            let Some(k) = interned else {
                interned = Some(heap.keys().lookup(&key.to_name()));
                continue;
            };
            // `Object.prototype.__proto__`'s getter (`crate::vm::prototype`).
            if k.is_some_and(|k| realm.is_some_and(|r| r.is_proto_accessor(id, k))) {
                return prototype::proto_getter(heap, task, receiver).map(Some);
            }
            match k.and_then(|k| view.get_own(k)) {
                Some(property) => return Ok(Some(property.value.clone())),
                None if view.builtin => return Err(function_prototype_miss(key, Access::Read)),
                None => match view.prototype {
                    Some(prototype) => {
                        id = prototype;
                        continue;
                    }
                    None => return Ok(None),
                },
            }
        }
        let step = heap
            .read(id, |object| match visit(object, task)? {
                Visit::Store(o, builtin) => read_store(o, builtin, key, interned),
                Visit::Array(a) => match key {
                    Key::Index(index) => Ok(match a.get(*index) {
                        Some(value) => Lookup::Found(value.clone()),
                        None => Lookup::Next(a.object().prototype()),
                    }),
                    _ if key.is_length() => Ok(Lookup::Found(Value::Number(f64::from(a.length())))),
                    Key::Name(_) => read_store(a.object(), false, key, interned),
                },
                Visit::Other(HeapObject::Function(_)) => Ok(Lookup::Function),
                Visit::Other(other) => match not_ordinary(other, key, Access::Read) {
                    Some(fault) => Err(fault),
                    None => Ok(Lookup::Next(None)),
                },
            })
            // The frame keeps the base alive and every object keeps its
            // prototype alive; a stale id is reported, not trusted.
            .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
        match step {
            Lookup::Found(v) => return Ok(Some(v)),
            Lookup::Next(Some(prototype)) => id = prototype,
            Lookup::Next(None) => return Ok(None),
            Lookup::NeedsName => interned = Some(heap.keys().lookup(&key.to_name())),
            Lookup::Function => match function::read(heap, task, id, key)? {
                FunctionRead::Found(v) => return Ok(Some(v)),
                FunctionRead::Continue(next) => {
                    id = next;
                    // The read may have built the realm (and interned its
                    // names) to continue at `Object.prototype`.
                    realm = heap.realm().get();
                    if let Key::Name(name) = key {
                        interned = Some(heap.keys().lookup(name));
                    }
                }
            },
        }
    }
}

/// `base.name` (`GetField`).
pub(crate) fn get_field(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &JsString,
) -> Result<Value, Fault> {
    get(heap, task, base, &Key::from_name(name))
}

/// What the receiver's part of a write did.
enum Stored {
    Done,
    /// An index key reached an ordinary object: intern its name (outside
    /// the object lock) and try again.
    NeedsName,
    /// The receiver lacks the property and has this prototype: the chain
    /// decides (module docs).
    Inherited(ObjectId),
    /// `[[Set]]` returned false (a read-only property, a non-extensible
    /// receiver): the strict-mode TypeError (module docs).
    Rejected(Fault),
    /// A function object: the write goes to its property record
    /// (`crate::vm::function::write_target`).
    Function,
}

/// The receiver's outcome of OrdinarySet.
fn stored(outcome: SetOutcome, key: &Key, prototype: Option<ObjectId>) -> Result<Stored, Fault> {
    match outcome {
        SetOutcome::Done => Ok(Stored::Done),
        SetOutcome::ReadOnly => Ok(Stored::Rejected(read_only(key))),
        SetOutcome::NotExtensible => Ok(Stored::Rejected(not_extensible(key))),
        SetOutcome::Inherited => prototype
            .map(Stored::Inherited)
            .ok_or(Fault::Internal("an inherited write without a prototype")),
    }
}

/// The receiver's part of `key = value` on `object` for `task`.
fn store(
    object: &mut HeapObject,
    task: TaskId,
    key: &Key,
    interned: Option<PropertyKey>,
    value: &Value,
) -> Result<Stored, Fault> {
    let named = |o: &mut OrdinaryObject| {
        let Some(k) = interned else {
            return Ok(Stored::NeedsName);
        };
        let prototype = o.prototype();
        stored(o.set(k, value.clone()), key, prototype)
    };
    match object {
        HeapObject::Plain(o) if o.is_intrinsic() => Err(immutable(key)),
        HeapObject::Builtin(_) => Err(immutable(key)),
        HeapObject::Function(_) => Ok(Stored::Function),
        HeapObject::Plain(o) => {
            check_write(o, task)?;
            named(o)
        }
        HeapObject::Array(a) => {
            check_write(a.object(), task)?;
            match key {
                Key::Index(index) => {
                    let prototype = a.object().prototype();
                    stored(a.set(*index, value.clone()), key, prototype)
                }
                _ if key.is_length() && !a.length_writable() => Ok(Stored::Rejected(read_only(key))),
                _ if key.is_length() => Err(Fault::Unsupported(
                    "cannot set property 'length' of an array: changing an array's length by assignment is not supported"
                        .to_string(),
                )),
                Key::Name(_) => named(a.object_mut()),
            }
        }
        other => Err(not_ordinary(other, key, Access::Write)
            .unwrap_or(Fault::Internal("a property write was not rejected"))),
    }
}

fn intern(heap: &Heap, name: &JsString) -> Result<PropertyKey, Fault> {
    heap.keys()
        .intern(name)
        .ok_or(Fault::OutOfMemory("property key table exhausted"))
}

/// `base[key] = value` for the running task `task` (`[[Set]]`; QuickJS
/// `put_field` and `put_array_el`). `strict` is the mode of the running
/// code: a write `[[Set]]` rejects (a read-only property, a non-extensible
/// receiver) is a TypeError in strict code and does nothing in sloppy code
/// (clj-quickjs-119). Module docs, "Writes and the prototype chain".
pub(crate) fn put(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    value: Value,
    strict: bool,
) -> Result<(), Fault> {
    let Value::Object(mut id) = *base else {
        return Err(primitive_base(base, key, Access::Write));
    };
    // The base itself: a function's writes go to its property record
    // (`id` below), but the `__proto__` setter gets the function.
    let receiver = id;
    let mut interned = match key {
        Key::Name(name) => Some(intern(heap, name)?),
        Key::Index(_) => None,
    };
    // A write of `__proto__` that meets `Object.prototype`'s accessor runs
    // its setter before any data-property rule (an immutable intrinsic
    // receiver, a function's property record) applies. The realm is built
    // for it if needed (a function can exist before any object literal).
    let realm = match key {
        Key::Name(name) if *name == "__proto__" => {
            Some(realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?)
        }
        _ => heap.realm().get(),
    };
    if let (Some(realm), Some(k)) = (realm, interned)
        && realm.is_proto_key(k)
        && reaches_proto_accessor(heap, task, receiver, k, key)?
    {
        return prototype::proto_setter(heap, task, receiver, &value);
    }
    // `[[Set]]` returned false (module docs).
    let rejected = |fault: Fault| if strict { Err(fault) } else { Ok(()) };
    let prototype = loop {
        let outcome = heap
            .with_object_mut(id, |object| store(object, task, key, interned, &value))
            .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
        match outcome {
            Stored::Done => return Ok(()),
            Stored::NeedsName => interned = Some(intern(heap, &key.to_name())?),
            Stored::Inherited(prototype) => break prototype,
            Stored::Rejected(fault) => return rejected(fault),
            Stored::Function => id = function::write_target(heap, task, id, key)?,
        }
    };
    // An index name that was never interned is on no ordinary object of
    // the chain, but may still be an element of an array on it.
    let chain_key = match interned {
        Some(k) => Some(k),
        None => heap.keys().lookup(&key.to_name()),
    };
    match check_inherited(heap, task, prototype, chain_key, key) {
        Ok(Chain::Add) => {}
        // `Object.prototype.__proto__`'s setter (`crate::vm::prototype`);
        // its TypeErrors are thrown in sloppy code too. The check above
        // normally catches it first.
        Ok(Chain::ProtoSetter) => {
            return prototype::proto_setter(heap, task, receiver, &value);
        }
        // The walk's only TypeError is an inherited read-only property.
        Err(fault @ Fault::TypeError(_)) => return rejected(fault),
        Err(fault) => return Err(fault),
    }
    // Only the running task changes the receiver, so it still lacks `key`.
    let added = |done: bool| match done {
        true => SetOutcome::Done,
        false => SetOutcome::NotExtensible,
    };
    let added = heap
        .with_object_mut(id, |object| match (object, key) {
            (HeapObject::Array(a), Key::Index(index)) => Ok(a.add(*index, value)),
            (HeapObject::Array(a), _) => Ok(added(
                chain_key.is_some_and(|k| a.object_mut().add(k, value)),
            )),
            (HeapObject::Plain(o), _) => Ok(added(chain_key.is_some_and(|k| o.add(k, value)))),
            _ => Err(Fault::Internal("property base changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
    match added {
        SetOutcome::Done => Ok(()),
        SetOutcome::ReadOnly => rejected(read_only(key)),
        SetOutcome::NotExtensible => rejected(not_extensible(key)),
        SetOutcome::Inherited => Err(Fault::Internal("an inherited write was not decided")),
    }
}

/// `base.name = value` (`PutField`).
pub(crate) fn put_field(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &JsString,
    value: Value,
    strict: bool,
) -> Result<(), Fault> {
    put(heap, task, base, &Key::from_name(name), value, strict)
}

/// What the prototype chain decided about a write of a property the
/// receiver does not have.
#[derive(Debug, PartialEq, Eq)]
enum Chain {
    /// The receiver gets a new own data property.
    Add,
    /// The write reached `Object.prototype.__proto__`: its setter runs
    /// instead (`crate::vm::prototype`).
    ProtoSetter,
}

/// The prototype-chain part of OrdinarySet for a property the receiver does
/// not have: walk from `prototype` to the first object with `k` (the
/// interned name of `key`); fail if that property is read-only. One short
/// object lock per object visited.
fn check_inherited(
    heap: &Heap,
    task: TaskId,
    mut id: ObjectId,
    k: Option<PropertyKey>,
    key: &Key,
) -> Result<Chain, Fault> {
    let mut realm = heap.realm().get();
    loop {
        // Intrinsics through their immutable view, as in `get`.
        if let Some(view) = realm.and_then(|r| r.view(id)) {
            if k.is_some_and(|k| realm.is_some_and(|r| r.is_proto_accessor(id, k))) {
                return Ok(Chain::ProtoSetter);
            }
            match k.and_then(|k| view.get_own(k)) {
                Some(property) if !property.flags.writable => return Err(read_only(key)),
                Some(_) => return Ok(Chain::Add),
                None if view.builtin => return Err(function_prototype_miss(key, Access::Write)),
                None => match view.prototype {
                    Some(prototype) => {
                        id = prototype;
                        continue;
                    }
                    None => return Ok(Chain::Add),
                },
            }
        }
        let in_store = |o: &OrdinaryObject, builtin: bool| match k.and_then(|k| o.get_own(k)) {
            Some(property) if !property.flags.writable => Err(read_only(key)),
            Some(_) => Ok(Walk::Next(None)),
            None if builtin => Err(function_prototype_miss(key, Access::Write)),
            None => Ok(Walk::Next(o.prototype())),
        };
        let next = heap
            .read(id, |object| match visit(object, task)? {
                Visit::Store(o, builtin) => in_store(o, builtin),
                Visit::Array(a) => match key {
                    Key::Index(index) => match a.element(*index) {
                        Some(element) if !element.flags.writable => Err(read_only(key)),
                        Some(_) => Ok(Walk::Next(None)),
                        None => Ok(Walk::Next(a.object().prototype())),
                    },
                    _ if key.is_length() && !a.length_writable() => Err(read_only(key)),
                    _ if key.is_length() => Ok(Walk::Next(None)),
                    _ => in_store(a.object(), false),
                },
                // A function as a prototype (`Object.create(f)`,
                // clj-quickjs-121): its own properties are in its record
                // (clj-quickjs-125).
                Visit::Other(HeapObject::Function(_)) => Ok(Walk::Function),
                // Another kind as a prototype: one without JavaScript
                // properties ends the walk; an error object, whose
                // properties are not supported yet, fails as for a read.
                Visit::Other(other) => match not_ordinary(other, key, Access::Read) {
                    Some(fault) => Err(fault),
                    None => Ok(Walk::Next(None)),
                },
            })
            .unwrap_or(Err(Fault::Internal("prototype was reclaimed")))?;
        match next {
            Walk::Next(Some(prototype)) => id = prototype,
            Walk::Next(None) => return Ok(Chain::Add),
            // Continue at the store that has the function's own `key`, or
            // where its read would continue; a name `Function.prototype`
            // will own fails (`crate::vm::function::read`). The step may
            // build the realm to continue at `Object.prototype`.
            Walk::Function => {
                id = match function::read(heap, task, id, key)? {
                    FunctionRead::Found(_) => match function::own_store(heap, task, id, key)? {
                        Some(record) => record,
                        None => return Err(Fault::Internal("a function property without a store")),
                    },
                    FunctionRead::Continue(next) => next,
                };
                realm = heap.realm().get();
            }
        }
    }
}

/// True when a write of `__proto__` (interned as `k`) to the object `id`
/// meets `Object.prototype`'s accessor: no object before it on the chain,
/// `id` included, has an own `__proto__` (`crate::vm::prototype`). Reads
/// only: a function is looked through without getting a property record.
fn reaches_proto_accessor(
    heap: &Heap,
    task: TaskId,
    mut id: ObjectId,
    k: PropertyKey,
    key: &Key,
) -> Result<bool, Fault> {
    let Some(realm) = heap.realm().get() else {
        return Ok(false);
    };
    loop {
        if realm.is_proto_accessor(id, k) {
            return Ok(true);
        }
        if let Some(view) = realm.view(id) {
            match (view.get_own(k), view.prototype) {
                (None, Some(prototype)) => {
                    id = prototype;
                    continue;
                }
                _ => return Ok(false),
            }
        }
        let own_or_next = |o: &OrdinaryObject| match o.get_own(k) {
            Some(_) => Walk::Next(None),
            None => Walk::Next(o.prototype()),
        };
        let step = heap
            .read(id, |object| match visit(object, task)? {
                Visit::Store(o, _) => Ok(own_or_next(o)),
                Visit::Array(a) => Ok(own_or_next(a.object())),
                Visit::Other(HeapObject::Function(_)) => Ok(Walk::Function),
                Visit::Other(_) => Ok(Walk::Next(None)),
            })
            .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
        match step {
            Walk::Next(Some(prototype)) => id = prototype,
            Walk::Next(None) => return Ok(false),
            Walk::Function => match function::read(heap, task, id, key)? {
                FunctionRead::Found(_) => return Ok(false),
                FunctionRead::Continue(next) => id = next,
            },
        }
    }
}

/// One step of [`check_inherited`] outside the realm's views.
enum Walk {
    /// Continue at this prototype, or (`None`) stop: the receiver may add
    /// the property.
    Next(Option<ObjectId>),
    /// The object is a function (`crate::vm::function`).
    Function,
}

/// Define `name` as an own data property of the object literal `object`
/// (QuickJS `define_field`: CreateDataPropertyOrThrow). The object was
/// created by the running task's `NewObjectLiteral` (or `NewObject`), so it is ordinary, owned and
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
    let key = intern(heap, name)?;
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

/// `[[DefineOwnProperty]]` of `key` on the object `target` for the running
/// task `task`, throwing on a rejection (DefinePropertyOrThrow, as
/// `Object.defineProperty` uses it; clj-quickjs-119). The kinds follow the
/// module table: an owned ordinary object validates and applies `desc`
/// ([`OrdinaryObject::define_own_property`]); an owned array does the same
/// for its named properties, and ArrayDefineOwnProperty for its elements and
/// `length` (`ArrayObject::define_element`, `ArrayObject::define_length`; a
/// define that would change `length` is `UnsupportedOperand`,
/// clj-quickjs-39); a realm intrinsic
/// is a TypeError (immutable); another task's object is `ForeignObject`; a
/// function's property goes to its property record
/// (`crate::vm::function::define_target`, clj-quickjs-125); an error object
/// is `UnsupportedOperand`; the other heap kinds have no properties and are
/// not extensible (TypeError).
pub(crate) fn define_own_property(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
    desc: &PropertyDescriptor,
) -> Result<(), Fault> {
    let target = match is_function(heap, target) {
        true => function::define_target(heap, task, target, key)?,
        false => target,
    };
    // An array index of an array needs no name; every other key does.
    let element = matches!(key, Key::Index(_))
        && heap
            .read(target, |o| matches!(o, HeapObject::Array(_)))
            .unwrap_or(false);
    let interned = match element {
        true => None,
        false => Some(intern(heap, &key.to_name())?),
    };
    let named = |o: &mut OrdinaryObject| {
        let k = interned.ok_or(Fault::Internal("a property name was not interned"))?;
        defined(o.define_own_property(k, desc), key)
    };
    let unsupported = |what: &str| {
        Err(Fault::Unsupported(format!(
            "cannot define property '{key}' of {what}: its properties are not supported"
        )))
    };
    heap.with_object_mut(target, |object| match object {
        HeapObject::Plain(o) if o.is_intrinsic() => Err(immutable_define(key)),
        HeapObject::Builtin(_) => Err(immutable_define(key)),
        HeapObject::Plain(o) => {
            check_write(o, task)?;
            named(o)
        }
        HeapObject::Array(a) => {
            check_write(a.object(), task)?;
            match key {
                Key::Index(index) => defined(a.define_element(*index, desc), key),
                _ if key.is_length() => match a.define_length(desc) {
                    Some(outcome) => defined(outcome, key),
                    None => Err(Fault::Unsupported(
                        "cannot define property 'length' of an array: changing an array's length is not supported"
                            .to_string(),
                    )),
                },
                Key::Name(_) => named(a.object_mut()),
            }
        }
        HeapObject::Function(_) => unsupported("a function"),
        HeapObject::Native(payload) if (&**payload as &dyn std::any::Any).is::<ErrorObject>() => {
            unsupported("an error object")
        }
        other => Err(Fault::TypeError(format!(
            "cannot define property '{key}' of a {}: it has no JavaScript properties",
            object_name(other)
        ))),
    })
    .unwrap_or(Err(Fault::Internal("property target was reclaimed")))
}

/// The fault of a [`DefineOutcome`].
fn defined(outcome: DefineOutcome, key: &Key) -> Result<(), Fault> {
    match outcome {
        DefineOutcome::Done => Ok(()),
        DefineOutcome::Rejected(reason) => Err(Fault::TypeError(format!(
            "cannot define property '{key}': {reason}"
        ))),
        DefineOutcome::Accessor => Err(Fault::Unsupported(format!(
            "cannot define property '{key}': accessor properties (get/set) are not supported"
        ))),
    }
}

/// The own property `key` of the object `target` for the running task
/// `task` (`[[GetOwnProperty]]`, as `Object.getOwnPropertyDescriptor` uses
/// it; clj-quickjs-119): `None` if absent. Ordinary objects (realm
/// intrinsics included), arrays and built-in functions report their own
/// properties; another task's object is `ForeignObject`; a function reports
/// those in its property record (`crate::vm::function::own_store`,
/// clj-quickjs-125); an error object is `UnsupportedOperand`; the other heap
/// kinds have none.
/// `Object.prototype.__proto__` is `UnsupportedOperand` until it is a
/// stored accessor property (`crate::vm::prototype`, clj-quickjs-120).
pub(crate) fn get_own_property(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    let target = match is_function(heap, target) {
        true => match function::own_store(heap, task, target, key)? {
            Some(record) => record,
            None => return Ok(None),
        },
        false => target,
    };
    let k = heap.keys().lookup(&key.to_name());
    if let (Some(realm), Some(k)) = (heap.realm().get(), k)
        && realm.is_proto_accessor(target, k)
    {
        return Err(Fault::Unsupported(
            "the property Object.prototype.__proto__ is an accessor, and accessor properties are not supported"
                .to_string(),
        ));
    }
    let named = |o: &OrdinaryObject| k.and_then(|k| o.get_own(k)).cloned();
    heap.read(target, |object| match visit(object, task)? {
        Visit::Store(o, _) => Ok(named(o)),
        Visit::Array(a) => Ok(match key {
            Key::Index(index) => a.element(*index),
            _ if key.is_length() => Some(a.length_property()),
            Key::Name(_) => named(a.object()),
        }),
        Visit::Other(other) => match not_ordinary(other, key, Access::Read) {
            Some(fault) => Err(fault),
            None => Ok(None),
        },
    })
    .unwrap_or(Err(Fault::Internal("property target was reclaimed")))
}

/// True for a function object, whose properties live in a separate record
/// (`crate::vm::function`).
fn is_function(heap: &Heap, id: ObjectId) -> bool {
    heap.read(id, |o| matches!(o, HeapObject::Function(_)))
        .unwrap_or(false)
}

/// The failure of a define on a realm intrinsic (`crate::realm`).
fn immutable_define(key: &Key) -> Fault {
    Fault::TypeError(format!(
        "cannot define property '{key}' of a built-in object: built-in objects are shared by all tasks and immutable"
    ))
}

/// A new empty array owned by the running task whose prototype is
/// `Array.prototype` (`NewArray`; QuickJS `JS_NewArray`; the realm is
/// built on first use).
pub(crate) fn new_array(heap: &Heap, task: TaskId) -> Result<Value, Fault> {
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let prototype = realm::intrinsics(heap)
        .ok_or_else(exhausted)?
        .array_prototype;
    let array = ArrayObject::with_prototype(task, prototype);
    heap.alloc(HeapObject::Array(Box::new(array)))
        .map(Value::Object)
        .ok_or_else(exhausted)
}

/// Append `element` (an element, or a hole for `None`) at index `length`
/// of the array literal `array` (`AppendElement`, `AppendHole`:
/// CreateDataProperty, which ignores the prototype chain). The array was
/// created by the running task's `NewArray`; anything else is a broken
/// invariant of hand-assembled code and fails as `UnsupportedOperand`.
pub(crate) fn append(
    heap: &Heap,
    task: TaskId,
    array: &Value,
    element: Option<Value>,
) -> Result<(), Fault> {
    let wrong = |found: &str| {
        Fault::Unsupported(format!(
            "append_element and append_hole need an array, got {found}"
        ))
    };
    let Value::Object(id) = *array else {
        return Err(wrong(array.type_name()));
    };
    heap.with_object_mut(id, |target| match target {
        HeapObject::Array(a) => {
            check_write(a.object(), task)?;
            if a.push(element) {
                Ok(())
            } else {
                Err(Fault::Unsupported(
                    "an array literal with more than 2^32 - 1 elements".to_string(),
                ))
            }
        }
        other => Err(wrong(object_name(other))),
    })
    .unwrap_or(Err(Fault::Internal("array literal was reclaimed")))
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

    /// Reads of intrinsic properties (and the prototype-chain check of a
    /// write) use the realm's immutable views: they finish while another
    /// thread holds the object-table lock (clj-quickjs-117).
    #[test]
    fn intrinsic_property_reads_do_not_lock_heap() {
        use std::sync::{Arc, mpsc};
        use std::time::Duration;
        let heap = Arc::new(Heap::default());
        let (prototype, object) = {
            let r = crate::realm::intrinsics(&heap).unwrap();
            (Value::Object(r.object_prototype), Value::Object(r.object))
        };
        let held = heap.hold_objects();
        let (tx, rx) = mpsc::channel();
        let reader = {
            let heap = Arc::clone(&heap);
            std::thread::spawn(move || {
                let task = TaskId::from_raw(1);
                let s = |t: &str| JsString::from(t);
                let read = |base: &Value, name: &str| get_field(&heap, task, base, &s(name));
                let results = [
                    read(&prototype, "toString").is_ok_and(|v| matches!(v, Value::Object(_))),
                    read(&prototype, "missing").is_ok_and(|v| v == Value::Undefined),
                    read(&object, "length").is_ok_and(|v| v == Value::Number(1.0)),
                    read(&object, "prototype").is_ok_and(|v| v.same_object(&prototype)),
                    matches!(read(&object, "call"), Err(Fault::Unsupported(_))),
                    check_inherited(
                        &heap,
                        task,
                        prototype_id(&prototype),
                        Some(key(&heap)),
                        &Key::Name(s("valueOf")),
                    )
                    .is_ok(),
                ];
                tx.send(results).unwrap();
            })
        };
        let results = rx
            .recv_timeout(Duration::from_secs(30))
            .expect("an intrinsic read waited for the object-table lock");
        drop(held);
        reader.join().unwrap();
        assert!(results.iter().all(|&ok| ok), "{results:?}");

        fn prototype_id(v: &Value) -> ObjectId {
            match v {
                Value::Object(id) => *id,
                _ => unreachable!(),
            }
        }
        fn key(heap: &Heap) -> crate::object::PropertyKey {
            heap.keys().lookup(&JsString::from("valueOf")).unwrap()
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
        match put_field(&heap, task, &native, &name, Value::Null, true) {
            Err(Fault::TypeError(m)) => assert!(m.contains("no JavaScript properties"), "{m}"),
            _ => panic!("expected a TypeError"),
        }
        match put(&heap, task, &native, &Key::Index(0), Value::Null, true) {
            Err(Fault::TypeError(m)) => assert!(m.contains("property '0'"), "{m}"),
            _ => panic!("expected a TypeError"),
        }
    }

    #[test]
    fn computed_keys_convert_by_to_property_key() {
        let name = |s: &str| Key::Name(JsString::from(s));
        for (key, expected) in [
            (Value::Number(1.0), Key::Index(1)),
            (Value::Number(-0.0), Key::Index(0)),
            (Value::Number(1.5), name("1.5")),
            (Value::Number(-1.0), name("-1")),
            (Value::Number(4294967295.0), name("4294967295")),
            (Value::Number(1e21), name("1e+21")),
            (Value::Number(f64::NAN), name("NaN")),
            (Value::String(JsString::from("2")), Key::Index(2)),
            (Value::String(JsString::from("02")), name("02")),
            (Value::Bool(true), name("true")),
            (Value::Null, name("null")),
            (Value::Undefined, name("undefined")),
        ] {
            assert_eq!(to_property_key(&key).ok(), Some(expected), "{key:?}");
        }
        let heap = Heap::default();
        let object = Value::Object(heap.alloc(HeapObject::host_object()).unwrap());
        assert!(matches!(
            to_property_key(&object),
            Err(Fault::Unsupported(_))
        ));
    }

    #[test]
    fn index_keys_of_ordinary_objects_are_their_names() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let o = OrdinaryObject::new(Some(task));
        let o = Value::Object(heap.alloc(HeapObject::Plain(o)).unwrap());
        // Reading an index nobody defined interns nothing.
        assert_eq!(
            get(&heap, task, &o, &Key::Index(7)).ok(),
            Some(Value::Undefined)
        );
        assert_eq!(heap.keys().lookup(&JsString::from("7")), None);
        assert!(put(&heap, task, &o, &Key::Index(7), Value::Bool(true), true).is_ok());
        assert_eq!(
            get_field(&heap, task, &o, &JsString::from("7")).ok(),
            Some(Value::Bool(true))
        );
    }

    /// OrdinarySet with an inherited non-writable data property
    /// (clj-quickjs-119): strict code throws, sloppy code ignores the write,
    /// and the receiver gets no own property either way (from source:
    /// `define_property_data_descriptors` in `tests/js_source.rs`).
    #[test]
    fn inherited_read_only_properties_reject_writes() {
        fn ok<T>(result: Result<T, Fault>) -> T {
            result.unwrap_or_else(|_| panic!("unexpected fault"))
        }
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let name = JsString::from("k");
        let k = Key::from_name(&name);
        let parent = heap
            .alloc(HeapObject::Plain(OrdinaryObject::new(Some(task))))
            .unwrap();
        let desc = PropertyDescriptor {
            value: Some(Value::Number(1.0)),
            ..PropertyDescriptor::default()
        };
        ok(define_own_property(&heap, task, parent, &k, &desc));
        let child = OrdinaryObject::with_prototype(task, parent);
        let child = Value::Object(heap.alloc(HeapObject::Plain(child)).unwrap());
        match put_field(&heap, task, &child, &name, Value::Number(2.0), true) {
            Err(Fault::TypeError(m)) => {
                assert_eq!(m, "cannot assign to read-only property 'k'")
            }
            _ => panic!("expected a TypeError"),
        }
        assert!(put_field(&heap, task, &child, &name, Value::Number(3.0), false).is_ok());
        let Value::Object(id) = child else {
            unreachable!()
        };
        assert!(ok(get_own_property(&heap, task, id, &k)).is_none());
        let inherited = ok(get_field(&heap, task, &child, &name));
        assert_eq!(inherited, Value::Number(1.0));
        // A writable inherited property lets the receiver add its own.
        let writable = PropertyDescriptor {
            writable: Some(true),
            ..PropertyDescriptor::default()
        };
        assert!(define_own_property(&heap, task, parent, &k, &writable).is_err());
        let other = Key::from_name(&JsString::from("w"));
        ok(define_own_property(&heap, task, parent, &other, &writable));
        ok(put(&heap, task, &child, &other, Value::Number(4.0), true));
        let own = ok(get_own_property(&heap, task, id, &other)).unwrap();
        assert_eq!(own.value, Value::Number(4.0));
        assert_eq!(own.flags, crate::object::PropertyFlags::DATA);
    }
}
