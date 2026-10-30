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
//! | array owned by the running task | an index: the element, else (a hole) the prototype chain; `length`; any other name as an ordinary object | an index: update the element, or (after the prototype-chain check) add it, extending `length`; `length`: ArraySetLength (clj-quickjs-154); any other name as an ordinary object |
//! | ordinary object created by the embedder (no owner) | as above | `ForeignObject` failure |
//! | ordinary object or array owned by another task | `ForeignObject` failure | `ForeignObject` failure |
//! | realm intrinsic `Object.prototype`, `Array.prototype`, `Reflect` (`crate::realm`) | as above, from every task | catchable `TypeError`: built-in objects are shared and immutable |
//! | built-in function (`Object`, `Object.is`, ...; `crate::vm::builtin`) | own property, else the chain from `Function.prototype` (clj-quickjs-102); a standard name `Function.prototype` lacks so far (`toString`, ...) `UnsupportedOperand` | catchable `TypeError`, as above |
//! | shareable regex value (a `regex` compiler-literal form, `crate::regexp::shared`, light-cljs-563) | own `lastIndex`, always 0 (task-local for a quoted global or sticky literal); any other key, symbols included, continues at `RegExp.prototype` | `lastIndex`: 0 changes nothing, any other value is a `TypeError` (a quoted global or sticky literal keeps the task's number); anything else is not extensible |
//! | persistent vector, map, occurrence; builder, atom, binding cell; native payload | `undefined` (no JavaScript properties) | catchable `TypeError` |
//! | function object (`crate::vm::function`, clj-quickjs-125) | code metadata (`length`, `name`) until its property record exists, plus a lazy `prototype`; names `Function.prototype` lacks so far `UnsupportedOperand`; any other name continues at `Function.prototype` (clj-quickjs-102) | the creating task writes its property record (`ForeignObject` for another task); a published function: catchable `TypeError` |
//! | module namespace owned by the running task (`crate::module::namespace`) | live exporting cells (ReferenceError in TDZ); fixed Symbol.toStringTag; absent names undefined | always rejects (`TypeError` in strict code, ignored in sloppy code); another task cannot access it |
//! | Error instance, including a caught engine error | ordinary object properties and prototype chain | ordinary object ownership and attributes |
//! | `undefined`, `null` | catchable `TypeError` | catchable `TypeError` |
//! | number, string (clj-quickjs-73) | `Number.prototype` / `String.prototype` and its chain, without a wrapper (see "Primitive bases"), after a string's own `length` and indices below it (clj-quickjs-142) | `TypeError` in strict code, nothing in sloppy code; `__proto__` does nothing |
//! | boolean | `Boolean.prototype` and its chain (clj-quickjs-302), without allocation | as a number |
//!
//! The non-ordinary heap kinds are this runtime's own values (or values
//! JavaScript code never holds), not JavaScript objects with hidden
//! properties: they behave like objects with no own properties, a null
//! prototype and no extensibility, so a read finds nothing and a write is
//! the strict-mode TypeError of an object that cannot take the property.
//! In particular a persistent vector (`__rt.vector(...)`) is not a
//! JavaScript array: `v[0]` and `v.length` read `undefined`.
//! Error instances use ordinary property storage (clj-quickjs-230).
//! Function metadata and own properties use their property records.
//!
//! **Writes and the prototype chain** (clj-quickjs-117, OrdinarySet). A
//! write to a property the receiver has itself updates it (a read-only one
//! is a `TypeError`). Otherwise the chain is walked: the first object on it
//! that has the property decides; if that property is read-only the write
//! is a `TypeError`, else (or if no object has it) the receiver gets a new
//! own data property (a non-extensible receiver is a `TypeError`). The
//! realm's intrinsic properties are writable, so `o.toString = f` gives `o`
//! its own `toString`. An accessor requests a getter or setter call with
//! the original receiver through the VM's generic accessor path, including
//! the stored built-in `Object.prototype.__proto__` (clj-quickjs-261).
//! A function on the chain (`Object.create(f)`) is walked through its
//! property record (clj-quickjs-125). An array element is written the same way: a hole or an index past
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
//! read-only `length` rejects new elements at or beyond it. Length changes
//! use ArraySetLength (clj-quickjs-154): validate a primitive as a u32,
//! grow without creating elements, or delete from the end until blocked by
//! a non-configurable element. Invalid lengths are catchable RangeErrors.
//!
//! # Primitive bases
//!
//! A property access on a number or string (clj-quickjs-73; ECMAScript
//! GetValue and PutValue, which apply ToObject to the base; QuickJS
//! `JS_GetPrototypePrimitive`) acts on the wrapper object ToObject would
//! make, without allocating it. A read first looks at the wrapper's own
//! properties: a Number wrapper has none; a String wrapper's
//! ([`string_own_property`], clj-quickjs-142; ECMAScript String exotic
//! objects, QuickJS `js_string_get_own_property`) are `length`, the
//! code-unit count, and each index below it, the one-code-unit string
//! there (`"\u{10ffff}"[0]` is the lone surrogate `"\u{dbff}"`). Both are
//! read in constant time from the string's code units: the index key is
//! already an integer ([`Key::Index`]) and nothing is rescanned. Any other
//! key (an index at or past the length, `"-1"`, `"1.5"`, `"01"`) continues
//! the walk at `Number.prototype` or `String.prototype` (`crate::realm`,
//! building the realm if needed), so `(1).toString` is
//! `Number.prototype.toString`. The same own properties answer
//! `Object.getOwnPropertyDescriptor`, `Object.keys` and
//! `Object.getOwnPropertyNames` of a primitive (`crate::vm::descriptor`,
//! `crate::vm::keys`), a `delete` and a write. `__proto__` answers the wrapper's
//! prototype. A method call `(1).m()` reads `m` this way and passes the
//! primitive itself as `this` (the `CallMethod` receiver is the base value,
//! as in strict mode). A write cannot succeed: the receiver is a primitive,
//! so `[[Set]]` returns false (a `TypeError` in strict code, nothing in
//! sloppy code), except that `Object.prototype`'s `__proto__` setter
//! accepts a primitive `this` and ignores it. Booleans follow the same
//! write rule; their reads use `Boolean.prototype` (clj-quickjs-302).
//!
//! # Deleting properties
//!
//! [`delete`] is the `Delete` instruction (clj-quickjs-124; QuickJS
//! `OP_delete`, `JS_DeleteProperty`, `delete_property`): `[[Delete]]` of an
//! own property, by the owner and kind rules of the table above (a delete
//! changes the object, so it is owner-only like a write). A configurable
//! own property is removed, keeping the creation order of the rest; an
//! array element becomes a hole. An absent property is `true`, with
//! nothing changed, also on a realm intrinsic, a published function or a
//! kind without JavaScript properties. A non-configurable property (an
//! array's `length`, a constructor's `prototype`) stays: a `TypeError` in
//! strict code, `false` in sloppy code. A property a realm intrinsic or
//! built-in function has is a `TypeError` in both modes (the sharing rule
//! of writes). A primitive base acts as its wrapper object without
//! allocating one. Deleting initial function metadata first creates its property record;
//! deleting an absent property creates nothing. It runs no JavaScript.
//!
//! # The `in` operator
//!
//! [`in_operator`] (clj-quickjs-74) is the `In` instruction and
//! [`has_property`] its HasProperty, the read walk above answering only
//! whether the property exists (a kind without JavaScript properties has
//! none; `Object.prototype.__proto__` exists without its getter running).
//!
//! # Property keys
//!
//! A computed key goes through ToPropertyKey ([`to_property_key`]) when the
//! `GetElem`/`PutElem` runs, so for a write after the right-hand side: a
//! string is the key as is; a number is its Number::toString (`1` and `"1"`
//! name the same property, `-0` is `"0"`, `1.5` is `"1.5"`); `true`,
//! `false`, `null` and `undefined` are their ToString. The VM converts
//! object keys with resumable ToPrimitive and hint string before calling
//! these primitive-key helpers (clj-quickjs-70). A key
//! that is an array index ([`crate::object::array::array_index`]) is
//! carried as the integer, so `a[1]` on an array never builds a string; an
//! ordinary object (or the chain behind a hole) looks it up by its
//! canonical name.
//!
//! No operation here allocates a heap object or runs JavaScript, except
//! `NewArray` and an access to a function object, which may allocate its
//! property record and lazy `prototype` (`crate::vm::function`), so
//! `GetField`, `PutField`, `GetElem`, `PutElem` and `In` are poll points. Each
//! object visited is read or changed under its own short access word, never
//! the object-table lock (`crate::heap::storage`; a realm intrinsic is read
//! from the realm's immutable view instead, with no lock: `crate::realm`,
//! "Lock-free reads"), and the property-key table is consulted before,
//! never while that word is held.
//! Polls run before the operation, never inside these helpers. Interned
//! operation-local keys (including `Interned`) end before returning to the
//! dispatch loop; executable bindings validate the runtime/collection stamp
//! before reusing a key. Accessor calls are returned as values for a later VM step. Keys
//! carried across that step use `Key` names/symbols, never raw key ids.

use std::fmt;

use crate::collections::{form, object_name};
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::array::{ArrayObject, array_index, number_index};
use crate::object::{
    DefineOutcome, DeleteOutcome, InlineCache, OrdinaryObject, Property, PropertyDescriptor,
    PropertyFlags, PropertyKey, SetOutcome,
};
use crate::realm;
use crate::value::string::number_to_string;
use crate::value::{JsString, JsSymbol, Value};
use crate::vm::dispatch::Fault;
use crate::vm::function::{self, FunctionRead};
use crate::vm::promise::Promise;

/// Classified once at executable load; runtime-owned atom IDs bind lazily.
/// The atomic binding includes the table/GC generation, so this immutable
/// executable can safely run in several heaps without retaining their keys.
#[derive(Debug)]
pub(crate) struct NamedProperty {
    pub(crate) key: Key,
    name: JsString,
    binding: std::sync::atomic::AtomicU64,
}

impl NamedProperty {
    pub(crate) fn new(name: &JsString) -> Self {
        Self {
            key: Key::from_name(name),
            name: name.clone(),
            binding: Default::default(),
        }
    }

    fn intern(&self, heap: &Heap) -> Result<PropertyKey, Fault> {
        heap.keys()
            .bind_name(&self.name, &self.binding)
            .ok_or(Fault::OutOfMemory("property-key table exhausted"))
    }
}

/// A Rust-side name using the executable's runtime/collection-stamped key
/// binding and live slot validation. Only successful lookups are cached, so
/// reading an absent property does not intern it or consume heap bytes.
pub(crate) struct FixedName {
    text: &'static str,
    bound: std::sync::OnceLock<(NamedProperty, InlineCache)>,
}

impl FixedName {
    pub(crate) const fn new(text: &'static str) -> Self {
        Self {
            text,
            bound: std::sync::OnceLock::new(),
        }
    }

    fn bound(&self) -> &(NamedProperty, InlineCache) {
        self.bound.get_or_init(|| {
            (
                NamedProperty::new(&JsString::fixed(self.text)),
                InlineCache::default(),
            )
        })
    }

    pub(crate) fn intern(&self, heap: &Heap) -> Result<PropertyKey, Fault> {
        self.bound().0.intern(heap)
    }

    pub(crate) fn matches(&self, text: &JsString) -> bool {
        &self.bound().0.name == text
    }

    pub(crate) fn read(
        &self,
        heap: &Heap,
        task: TaskId,
        base: &Value,
    ) -> Result<Option<Read>, Fault> {
        let (name, cache) = self.bound();
        let key = heap.keys().lookup_bound_name(&name.name, &name.binding);
        lookup_cached(
            heap,
            task,
            base,
            &name.key,
            false,
            false,
            key.map(|key| (key, cache)),
        )
    }
}

pub(crate) mod names {
    use super::FixedName;

    pub(crate) static LENGTH: FixedName = FixedName::new("length");
    pub(crate) static NEXT: FixedName = FixedName::new("next");
    pub(crate) static VALUE: FixedName = FixedName::new("value");
    pub(crate) static DONE: FixedName = FixedName::new("done");
    pub(crate) static ITERATOR: FixedName = FixedName::new("iterator");
    pub(crate) static PROTOTYPE: FixedName = FixedName::new("prototype");
}

pub(crate) fn read_named(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &NamedProperty,
    cache: &InlineCache,
) -> Result<Option<Read>, Fault> {
    lookup_cached(
        heap,
        task,
        base,
        &name.key,
        false,
        false,
        Some((name.intern(heap)?, cache)),
    )
}

/// A property key after ToPropertyKey: an array index, or any other name.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Key {
    /// An integer `0 ..= 2^32 - 2`.
    Index(u32),
    /// A name that is not the canonical string of an array index.
    Name(JsString),
    Symbol(JsSymbol),
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
            Key::Symbol(_) => unreachable!("symbol key has no string name"),
        }
    }

    fn lookup(&self, heap: &Heap) -> Option<PropertyKey> {
        match self {
            Key::Symbol(s) => heap.keys().lookup_symbol(s),
            Key::Name(name) => heap.keys().lookup(name),
            Key::Index(_) => heap.keys().lookup(&self.to_name()),
        }
    }

    fn intern(&self, heap: &Heap) -> Result<PropertyKey, Fault> {
        match self {
            Key::Symbol(s) => heap
                .keys()
                .intern_symbol(s)
                .ok_or(Fault::OutOfMemory("property-key table exhausted")),
            Key::Name(name) => intern(heap, name),
            Key::Index(_) => intern(heap, &self.to_name()),
        }
    }

    pub(crate) fn is_length(&self) -> bool {
        matches!(self, Key::Name(name) if names::LENGTH.matches(name))
    }
}

impl fmt::Display for Key {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Key::Index(index) => write!(f, "{index}"),
            Key::Name(name) => write!(f, "{name}"),
            Key::Symbol(s) => write!(f, "{s}"),
        }
    }
}

/// ToPropertyKey of a computed key (module docs, "Property keys").
pub(crate) fn to_property_key(key: &Value) -> Result<Key, Fault> {
    Ok(match key {
        Value::String(s) => Key::from_name(s),
        Value::Symbol(s) => Key::Symbol(s.clone()),
        Value::BigInt(_) => Key::from_name(&super::dispatch::to_js_string(key)?),
        Value::Number(n) => match number_index(*n) {
            Some(index) => Key::Index(index),
            None => Key::Name(JsString::from(number_to_string(*n).as_str())),
        },
        Value::Bool(b) => Key::Name(JsString::fixed(if *b { "true" } else { "false" })),
        Value::Null => Key::Name(JsString::fixed("null")),
        Value::Undefined => Key::Name(JsString::fixed("undefined")),
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
    base_key(base, key, access)
}

/// [`element_key`] for `access`.
fn base_key(base: &Value, key: &Value, access: Access) -> Result<Key, Fault> {
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
    Delete,
}

impl Access {
    fn verb(self) -> &'static str {
        match self {
            Access::Read => "read",
            Access::Write => "set",
            Access::Delete => "delete",
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

/// The attributes of a String object's index properties (ECMAScript
/// StringGetOwnProperty): read-only, enumerable, not configurable.
const STRING_INDEX: PropertyFlags = PropertyFlags {
    writable: false,
    enumerable: true,
    configurable: false,
};

/// The own property `key` of the String wrapper of `s` (module docs,
/// "Primitive bases"; ECMAScript StringGetOwnProperty and the String
/// object's `length`; QuickJS `js_string_get_own_property`), or `None`:
/// `length` is the code-unit count, read-only, not enumerable, not
/// configurable; an index below the length is the one-code-unit string
/// there, read-only, enumerable, not configurable. Any other key (`"-1"`,
/// `"1.5"`, `"01"`, an index at or past the length) is not an own
/// property. Constant time: an index key is already an integer and the
/// unit is read directly. An index's value is a fresh one-unit string the
/// caller may retain, so it is charged to `heap`'s byte budget first
/// (`HeapLimit` at the limit); a caller that needs only the property's
/// existence or attributes uses the allocation-free [`string_own_flags`].
pub(crate) fn string_own_property(
    heap: &Heap,
    s: &JsString,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    Ok(match key {
        Key::Index(index) => s
            .code_unit_at_accounted(heap, *index as usize)?
            .map(|unit| Property {
                accessor: None,
                value: Value::String(unit),
                flags: STRING_INDEX,
            }),
        _ if key.is_length() => Some(Property {
            accessor: None,
            // At most `MAX_STRING_LEN` units, exact as an f64.
            value: Value::Number(s.len() as f64),
            flags: PropertyFlags::FIXED,
        }),
        Key::Name(_) | Key::Symbol(_) => None,
    })
}

/// The attributes of [`string_own_property`]'s property, without making
/// its value: `None` exactly when that is `None`.
pub(crate) fn string_own_flags(s: &JsString, key: &Key) -> Option<PropertyFlags> {
    match key {
        Key::Index(index) => ((*index as usize) < s.len()).then_some(STRING_INDEX),
        _ if key.is_length() => Some(PropertyFlags::FIXED),
        Key::Name(_) | Key::Symbol(_) => None,
    }
}

/// [`string_own_property`] with an uncharged value, for a caller that only
/// compares it and drops it within the same leaf step (transient storage,
/// `docs/architecture.md`, "Byte accounting").
fn string_own_transient(s: &JsString, key: &Key) -> Option<Property> {
    let flags = string_own_flags(s, key)?;
    let value = match key {
        Key::Index(index) => Value::String(s.code_unit_at(*index as usize)?),
        _ => Value::Number(s.len() as f64),
    };
    Some(Property {
        accessor: None,
        value,
        flags,
    })
}

/// Where a read of `key` on the primitive `base` continues once its
/// wrapper's own properties ([`string_own_property`]) lack it (module docs,
/// "Primitive bases"): `Number.prototype` for a number, `String.prototype`
/// for a string, `Symbol.prototype` for a symbol, `BigInt.prototype` for a BigInt,
/// `Boolean.prototype` for a boolean, building the realm if needed;
/// `undefined`/`null` are a catchable `TypeError`.
fn primitive_prototype(heap: &Heap, base: &Value, key: &Key) -> Result<ObjectId, Fault> {
    let intrinsics = match base {
        Value::Number(_)
        | Value::String(_)
        | Value::BigInt(_)
        | Value::Symbol(_)
        | Value::Bool(_) => {
            realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        }
        _ => return Err(primitive_base(base, key, Access::Read)),
    };
    intrinsics.primitive_prototype(base).ok_or(Fault::Internal(
        "a primitive without its registered prototype",
    ))
}

/// A struct's field as a JavaScript property: a struct is a frozen object
/// (`crate::collections::form`, "Structs"), so its fields are enumerable,
/// non-writable and non-configurable, it is not extensible, and writes,
/// deletes and defines fail as they do on any frozen object.
const STRUCT_FIELD: PropertyFlags = PropertyFlags {
    writable: false,
    enumerable: true,
    configurable: false,
};

/// A RegExp's `lastIndex`, on a shareable regex value as on any RegExp.
const LAST_INDEX: PropertyFlags = PropertyFlags {
    writable: true,
    enumerable: false,
    configurable: false,
};

/// The failure of an access to a heap object without an ordinary store.
fn not_ordinary(object: &HeapObject, key: &Key, access: Access) -> Option<Fault> {
    match access {
        // Nothing to find, so nothing to delete.
        Access::Read | Access::Delete => None,
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

/// A lookup reached `Function.prototype` without finding `key`, one of its
/// standard properties that does not exist yet
/// (`crate::vm::function::lacks_inherited`): an explicit gap instead of
/// continuing at `Object.prototype`.
fn function_prototype_miss(key: &Key, access: Access) -> Fault {
    Fault::Unsupported(format!(
        "cannot {} property '{key}' inherited from Function.prototype: it is not supported yet",
        access.verb()
    ))
}

/// Writes: the owner task only.
fn check_write(object: &OrdinaryObject, task: TaskId) -> Result<(), Fault> {
    // Internal template arrays are deeply immutable. Let their frozen
    // descriptors reject writes/defines/deletes with normal JS semantics.
    // Plain realm intrinsics are rejected separately before this check.
    if object.is_intrinsic() && object.is_frozen() {
        return Ok(());
    }
    match object.owner() {
        Some(owner) if owner == task => Ok(()),
        owner => Err(foreign(owner, task)),
    }
}

/// One object visited by a property access.
enum Visit<'a> {
    /// An ordinary object, or a built-in function's property store, that
    /// the running task may read.
    Store(&'a OrdinaryObject),
    /// An array the running task may read.
    Array(&'a ArrayObject),
    /// A kind without an ordinary property store.
    Other(&'a HeapObject),
}

/// Classify `object` for a read by `task` (owner check included).
fn visit<'a>(object: &'a HeapObject, task: TaskId) -> Result<Visit<'a>, Fault> {
    let readable = |o: &OrdinaryObject| match o.owner() {
        Some(owner) if owner != task => Err(foreign(Some(owner), task)),
        _ => Ok(()),
    };
    match object {
        HeapObject::Plain(o) => readable(o).map(|()| Visit::Store(o)),
        HeapObject::Promise(Promise {
            properties: Some(o),
            ..
        }) => readable(o).map(|()| Visit::Store(o)),
        HeapObject::Generator(g) => readable(&g.object).map(|()| Visit::Store(&g.object)),
        HeapObject::JsMap(m) | HeapObject::JsSet(m) => {
            readable(m.object()).map(|()| Visit::Store(m.object()))
        }
        HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => {
            readable(m.object()).map(|()| Visit::Store(m.object()))
        }
        HeapObject::WeakRef(m) => readable(m.object()).map(|()| Visit::Store(m.object())),
        HeapObject::FinalizationRegistry(m) => {
            readable(m.object()).map(|()| Visit::Store(m.object()))
        }
        HeapObject::DisposableStack(m) => readable(m.object()).map(|()| Visit::Store(m.object())),
        HeapObject::ArrayBuffer(b) => readable(&b.object).map(|()| Visit::Store(&b.object)),
        HeapObject::TypedArray(a) => readable(&a.object).map(|()| Visit::Store(&a.object)),
        HeapObject::Array(a) => readable(a.object()).map(|()| Visit::Array(a)),
        HeapObject::Builtin(f) => Ok(Visit::Store(&f.properties)),
        other => Ok(Visit::Other(other)),
    }
}

/// The interned key of a property name: `None` before it was looked up (an
/// index key, until an ordinary store needs its name), `Some(None)` for a
/// name that was never interned and so is no object's property.
type Interned = Option<Option<PropertyKey>>;

/// Snapshot exotic routing while the property store is already borrowed.
/// Ordinary accesses do not probe the table again or parse numeric names.
enum Exotic {
    Proxy,
    Namespace(Option<crate::module::namespace::Entry>),
    Typed(crate::object::typed_array::View, f64),
}

fn exotic(object: &HeapObject, task: TaskId, key: &Key) -> Result<Option<Exotic>, Fault> {
    Ok(match object {
        HeapObject::Proxy(p) => {
            if p.owner != task {
                return Err(foreign(Some(p.owner), task));
            }
            Some(Exotic::Proxy)
        }
        HeapObject::ModuleNamespace(ns) => Some(Exotic::Namespace(ns.entry(task, key)?)),
        HeapObject::TypedArray(a) => {
            if let Some(index) = crate::object::typed_array::numeric_key(key) {
                if a.object.owner() != Some(task) {
                    return Err(foreign(a.object.owner(), task));
                }
                Some(Exotic::Typed(a.view, index))
            } else {
                None
            }
        }
        _ => None,
    })
}

/// One step of a property read.
enum Lookup {
    Exotic(Exotic),
    Found(Read),
    /// Not an own property: continue at the prototype (or stop at null).
    Next(Option<ObjectId>),
    /// An index key reached an ordinary store: look its name up in the key
    /// table (outside the object lock) and visit the object again.
    NeedsName,
    /// A function object: its own properties decide
    /// (`crate::vm::function::read`).
    Function,
    /// A shareable regex value without the key: continue at
    /// `RegExp.prototype` (`crate::regexp::shared`).
    RegExpPrototype,
}

/// Whether `key` names a RegExp's `lastIndex`.
fn is_last_index(key: &Key) -> bool {
    matches!(key, Key::Name(name) if name == "lastIndex")
}

/// A property read either produces a value or requests a resumable getter
/// call. The caller supplies the original receiver, including for inheritance.
#[derive(Clone, Debug)]
pub(crate) enum Read {
    Value(Value),
    Deferred(Deferred),
}

/// A read that executes user code. Proxy reads retain the lookup object and
/// key here; the caller supplies the original receiver just as for a getter.
/// A native form's named member is answered by the task's core-protocol
/// resolver (`crate::vm::native::core_protocol`), with the receiver unchanged:
/// a form is the same object in JavaScript and in the compiler's runtime.
#[derive(Clone, Debug)]
pub(crate) enum Deferred {
    Getter(Value),
    Proxy {
        target: ObjectId,
        key: Key,
        conversion: bool,
    },
    Form {
        target: ObjectId,
        key: Key,
    },
}

impl Deferred {
    pub(crate) fn action(self, receiver: Value) -> super::native::Action {
        match self {
            Self::Getter(callee) => super::native::Action::Call {
                callee,
                this: receiver,
                args: vec![],
            },
            Self::Proxy {
                target,
                key,
                conversion,
            } => super::native::Action::GetProperty {
                target,
                key,
                receiver,
                conversion,
            },
            Self::Form { target, key } => super::native::Action::FormMember {
                form: Value::Object(target),
                key,
            },
        }
    }
}

impl Read {
    pub(crate) fn property(property: &Property, has: bool) -> Read {
        match &property.accessor {
            Some(_) if has => Read::Value(Value::Undefined),
            Some((Value::Undefined, _)) => Read::Value(Value::Undefined),
            Some((get, _)) => Read::Deferred(Deferred::Getter(get.clone())),
            None => Read::Value(property.value.clone()),
        }
    }

    pub(crate) fn leaf(self) -> Result<Value, Fault> {
        match self {
            Read::Value(v) => Ok(v),
            Read::Deferred(_) => Err(Fault::Unsupported(
                "this operation requires a resumable property read".into(),
            )),
        }
    }
}

/// The own named property of the store `o`, or where to continue.
/// (`Function.prototype`, the one store whose misses can fail, is an
/// intrinsic and always read through its view.)
fn read_store(
    heap: &Heap,
    o: &OrdinaryObject,
    key: &Key,
    interned: Interned,
    has: bool,
    cache: Option<&InlineCache>,
) -> Result<Lookup, Fault> {
    if has {
        // A presence check makes no index string.
        if string_wrapper_flags(o, key).is_some() {
            return Ok(Lookup::Found(Read::Value(Value::Undefined)));
        }
    } else if let Some(property) = string_wrapper_property(heap, o, key)? {
        return Ok(Lookup::Found(Read::Value(property.value)));
    }
    let Some(k) = interned else {
        return Ok(Lookup::NeedsName);
    };
    match k.and_then(|k| match cache {
        Some(cache) => o.get_own_cached(k, cache),
        None => o.get_own(k),
    }) {
        Some(property) => Ok(Lookup::Found(Read::property(property, has))),
        None => Ok(Lookup::Next(o.prototype())),
    }
}

/// Virtual own properties of an allocated String wrapper. The intrinsic
/// String.prototype wraps the empty string and already stores its length.
/// An index value is charged ([`string_own_property`]).
fn string_wrapper_property(
    heap: &Heap,
    o: &OrdinaryObject,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    match wrapped_string(o) {
        Some(s) => string_own_property(heap, s, key),
        None => Ok(None),
    }
}

/// The attributes of [`string_wrapper_property`]'s property, without making
/// its value.
fn string_wrapper_flags(o: &OrdinaryObject, key: &Key) -> Option<PropertyFlags> {
    wrapped_string(o).and_then(|s| string_own_flags(s, key))
}

fn wrapped_string(o: &OrdinaryObject) -> Option<&JsString> {
    if !o.is_intrinsic()
        && let Some(crate::object::PrimitiveData::String(s)) = o.primitive_data()
    {
        return Some(s);
    }
    None
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
    read(heap, task, base, key)?.map(Read::leaf).transpose()
}

pub(crate) fn read(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
) -> Result<Option<Read>, Fault> {
    lookup(heap, task, base, key, false, false)
}

/// HasProperty(`base`, `key`) for the running task `task` (QuickJS
/// `JS_HasProperty`; the `in` operator, clj-quickjs-74): whether the object
/// `base` or an object on its prototype chain has the property, whatever
/// its value. The walk and its failures are [`find`]'s, except that
/// reaching `Object.prototype.__proto__` answers true without running its
/// getter (it is an own property of `Object.prototype`, not of `base`).
pub(crate) fn has_property(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
) -> Result<bool, Fault> {
    lookup(heap, task, base, key, true, false).map(|found| found.is_some())
}

/// HasProperty stops at the first Proxy so the VM can run its trap without
/// retaining a Rust property-walk frame. Ordinary presence never calls getters.
pub(crate) enum Presence {
    Value(bool),
    Proxy(ObjectId),
    /// A native form's named member that is not a struct field: present
    /// when the task's resolver supplies it (`Deferred::Form`).
    Form {
        target: ObjectId,
        key: Key,
    },
}

pub(crate) fn probe(heap: &Heap, task: TaskId, base: &Value, key: &Key) -> Result<Presence, Fault> {
    match lookup_cursor(heap, task, base, key, LookupMode::Probe, None)? {
        LookupResult::Read(Some(Read::Deferred(Deferred::Form { target, key }))) => {
            Ok(Presence::Form { target, key })
        }
        LookupResult::Read(found) => Ok(Presence::Value(found.is_some())),
        LookupResult::Proxy(id) => Ok(Presence::Proxy(id)),
    }
}

/// `key in base` (`In`, clj-quickjs-74; QuickJS `OP_in`,
/// `js_operator_in`): a `base` that is not an object is a catchable
/// `TypeError`, checked before the key is converted; then
/// ToPropertyKey(`key`) ([`to_property_key`], after resumable
/// object conversion in the VM) and [`has_property`]. Heap
/// kinds without JavaScript properties (module docs) have none, so the
/// answer is false. Error instances use ordinary property storage.
pub(crate) fn in_operator(
    heap: &Heap,
    task: TaskId,
    key: &Value,
    base: &Value,
) -> Result<bool, Fault> {
    if !matches!(base, Value::Object(_)) {
        // Only the message converts the key: a primitive's conversion has
        // no effects, and an object key is not converted.
        let wanted = match to_property_key(key) {
            Ok(key) => format!("'{key}'"),
            Err(_) => "an object key".to_string(),
        };
        let base = match base {
            Value::Number(n) => number_to_string(*n),
            Value::Bool(b) => b.to_string(),
            Value::String(s) => format!("\"{s}\""),
            other => other.type_name().to_string(),
        };
        return Err(Fault::TypeError(format!(
            "cannot use 'in' operator to search for {wanted} in {base}"
        )));
    }
    has_property(heap, task, base, &to_property_key(key)?)
}

/// `Some(None)` means an invalid integer-indexed key: stop the prototype walk.
fn typed_element(
    heap: &Heap,
    task: TaskId,
    id: ObjectId,
    key: &Key,
) -> Result<Option<Option<Value>>, Fault> {
    let Some(index) = crate::object::typed_array::numeric_key(key) else {
        return Ok(None);
    };
    let Some(view) = crate::object::typed_array::view(heap, task, id)? else {
        return Ok(None);
    };
    view.get(heap, index).map(Some)
}

/// [`find`], or with `has` [`has_property`]'s walk: the found value is
/// then only a presence marker.
fn lookup(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    has: bool,
    conversion: bool,
) -> Result<Option<Read>, Fault> {
    lookup_cached(heap, task, base, key, has, conversion, None)
}

fn lookup_cached(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    has: bool,
    conversion: bool,
    cache: Option<(PropertyKey, &InlineCache)>,
) -> Result<Option<Read>, Fault> {
    let mode = if has {
        LookupMode::Has
    } else if conversion {
        LookupMode::Conversion
    } else {
        LookupMode::Read
    };
    match lookup_cursor(heap, task, base, key, mode, cache)? {
        LookupResult::Read(found) => Ok(found),
        LookupResult::Proxy(_) => Err(Fault::Internal("leaf lookup suspended at a Proxy")),
    }
}

enum LookupResult {
    Read(Option<Read>),
    Proxy(ObjectId),
}

enum LookupMode {
    Read,
    Conversion,
    Has,
    Probe,
}

fn lookup_cursor(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    mode: LookupMode,
    cache: Option<(PropertyKey, &InlineCache)>,
) -> Result<LookupResult, Fault> {
    use LookupResult::Read as Found;
    let has = matches!(mode, LookupMode::Has | LookupMode::Probe);
    let conversion = matches!(mode, LookupMode::Conversion);
    // A primitive base reads its wrapper's own properties, then its
    // wrapper's prototype without allocating a wrapper. Accessors receive
    // the original base through the VM's generic call path.
    let mut id = match base {
        Value::Object(id) => *id,
        // A presence check makes no index string.
        Value::String(s) if has && string_own_flags(s, key).is_some() => {
            return Ok(Found(Some(Read::Value(Value::Undefined))));
        }
        Value::String(s) if !has && let Some(own) = string_own_property(heap, s, key)? => {
            return Ok(Found(Some(Read::Value(own.value))));
        }
        _ => primitive_prototype(heap, base, key)?,
    };
    // A name that was never interned is no object's property; the owner
    // and kind checks below still apply.
    let mut interned: Interned = cache.map(|(key, _)| Some(key)).or_else(|| match key {
        Key::Name(_) | Key::Symbol(_) => Some(key.lookup(heap)),
        Key::Index(_) => None,
    });
    let mut realm = heap.realm().get();
    let mut remaining = 4096usize;
    loop {
        remaining = remaining
            .checked_sub(1)
            .ok_or_else(|| Fault::StackOverflow("maximum property lookup depth exceeded".into()))?;
        // An intrinsic is read from the realm's immutable view, without the
        // object-table lock (`crate::realm`, "Lock-free reads").
        if let Some(view) = realm.and_then(|r| r.view(id)) {
            let Some(k) = interned else {
                interned = Some(key.lookup(heap));
                continue;
            };
            match k.and_then(|k| match cache {
                Some((_, cache)) => view.get_own_cached(k, cache),
                None => view.get_own(k),
            }) {
                Some(property) => return Ok(Found(Some(Read::property(property, has)))),
                None if realm.is_some_and(|r| id == r.date_prototype)
                    && matches!(key, Key::Name(name)
                        if name == &JsString::from("toJSON")) =>
                {
                    // toJSON remains deferred. Presence checks neither
                    // retrieve nor run it.
                    if has {
                        return Ok(Found(Some(Read::Value(Value::Undefined))));
                    }
                    // Falling through would let JSON.stringify encode Date
                    // as an ordinary object.
                    return Err(Fault::Unsupported("Date toJSON is not supported".into()));
                }
                None if view.function_prototype
                    && function::lacks_inherited(key)
                    && !conversion =>
                {
                    // Explicit reads report the missing Function method.
                    // Conversion uses whatever its prototype chain provides.
                    return Err(function_prototype_miss(key, Access::Read));
                }
                None => match view.prototype {
                    Some(prototype) => {
                        id = prototype;
                        continue;
                    }
                    None => return Ok(Found(None)),
                },
            }
        }
        let step = heap
            .read(id, |object| {
                if let Some(route) = exotic(object, task, key)? {
                    return Ok(Lookup::Exotic(route));
                }
                match visit(object, task)? {
                    Visit::Store(o) => {
                        read_store(heap, o, key, interned, has, cache.map(|(_, cache)| cache))
                    }
                    Visit::Array(a) => match key {
                        Key::Index(index) => Ok(match a.element(*index) {
                            Some(property) => Lookup::Found(Read::property(&property, has)),
                            None => Lookup::Next(a.object().prototype()),
                        }),
                        _ if key.is_length() => Ok(Lookup::Found(Read::Value(Value::Number(
                            f64::from(a.length()),
                        )))),
                        Key::Name(_) | Key::Symbol(_) => read_store(
                            heap,
                            a.object(),
                            key,
                            interned,
                            has,
                            cache.map(|(_, cache)| cache),
                        ),
                    },
                    Visit::Other(HeapObject::Function(_) | HeapObject::BoundFunction(_)) => {
                        Ok(Lookup::Function)
                    }
                    Visit::Other(HeapObject::Promise(p)) => {
                        p.check_owner(task).map(|()| Lookup::Next(p.prototype()))
                    }
                    // An occurrence is transparent: the walk continues at
                    // the object it wraps (a form's members, a plain
                    // object's store).
                    Visit::Other(HeapObject::Occurrence(o)) => Ok(Lookup::Next(match o.value {
                        Value::Object(inner) => Some(inner),
                        _ => None,
                    })),
                    // A struct's fields are its own data properties; a
                    // form's other named members (methods, callable
                    // adapters, conversions, what it inherits from
                    // Object.prototype) belong to the task's runtime, for an
                    // object inheriting from a form too: reads, conversions
                    // and the resumable presence probe (`in`, `Reflect.has`)
                    // defer to it, a leaf presence check sees only the
                    // fields, and symbol and index keys nothing.
                    // A shareable regex value is a RegExp whose own
                    // `lastIndex` is 0 (`crate::regexp::shared`);
                    // every other key, symbols included, is RegExp.prototype's.
                    Visit::Other(HeapObject::Form(f)) if let Some(cell) = f.regex_cell() => {
                        Ok(if is_last_index(key) {
                            Lookup::Found(Read::Value(if has {
                                Value::Undefined
                            } else {
                                cell.last_index(task)
                            }))
                        } else {
                            Lookup::RegExpPrototype
                        })
                    }
                    Visit::Other(HeapObject::Form(f)) if let Key::Name(name) = key => {
                        if let Some(value) = form::struct_own_property(f, name) {
                            if !has && matches!(f.body(), form::FormBody::Struct(s) if s.mutable.is_some()) {
                                return Ok(Lookup::Found(Read::Deferred(Deferred::Form { target: id, key: key.clone() })));
                            }
                            Ok(Lookup::Found(Read::Value(if has {
                                Value::Undefined
                            } else {
                                value
                            })))
                        } else if matches!(mode, LookupMode::Has) {
                            Ok(Lookup::Next(None))
                        } else {
                            Ok(Lookup::Found(Read::Deferred(Deferred::Form {
                                target: id,
                                key: key.clone(),
                            })))
                        }
                    }
                    Visit::Other(other) => match not_ordinary(other, key, Access::Read) {
                        Some(fault) => Err(fault),
                        None => Ok(Lookup::Next(None)),
                    },
                }
            })
            // The frame keeps the base alive and every object keeps its
            // prototype alive; a stale id is reported, not trusted.
            .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
        match step {
            Lookup::Exotic(Exotic::Proxy) => {
                if matches!(mode, LookupMode::Probe) {
                    return Ok(LookupResult::Proxy(id));
                }
                if !has {
                    return Ok(Found(Some(Read::Deferred(Deferred::Proxy {
                        target: id,
                        key: key.clone(),
                        conversion,
                    }))));
                }
                id = super::proxy::forward(heap, task, id, if has { "has" } else { "get" })?;
                // Proxy forwarding has its own bound; do not charge its
                // classification as an additional prototype hop.
                remaining += 1;
            }
            Lookup::Exotic(Exotic::Namespace(entry)) => {
                return entry
                    .map(|entry| {
                        if has {
                            Ok(Read::Value(Value::Undefined))
                        } else {
                            crate::module::namespace::property(heap, task, entry)
                                .map(|p| Read::Value(p.value))
                        }
                    })
                    .transpose()
                    .map(Found);
            }
            Lookup::Exotic(Exotic::Typed(view, index)) => {
                return view.get(heap, index).map(|v| Found(v.map(Read::Value)));
            }
            Lookup::Found(v) => return Ok(Found(Some(v))),
            Lookup::Next(Some(prototype)) => id = prototype,
            Lookup::Next(None) => return Ok(Found(None)),
            Lookup::NeedsName => interned = Some(key.lookup(heap)),
            Lookup::RegExpPrototype => {
                id = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("RegExp realm"))?
                    .regexp_prototype;
                // Building the realm may have interned the name.
                realm = heap.realm().get();
                interned = None;
            }
            Lookup::Function => match if conversion {
                // Conversion asks only for valueOf/toString, never for
                // lazy own metadata such as prototype, length or name.
                let next = match function::property_record(heap, id)? {
                    Some(record) => record,
                    None => match function::inheritance_prototype(heap, id)? {
                        Some(prototype) => prototype,
                        None => return Ok(Found(None)),
                    },
                };
                Ok(FunctionRead::Continue(next))
            } else if has {
                function::probe(heap, task, id, key)
            } else {
                function::read(heap, task, id, key)
            }? {
                FunctionRead::Found(v) => return Ok(Found(Some(Read::Value(v)))),
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

/// A conversion method, skipping absent Function.prototype methods.
pub(crate) fn conversion_method(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &JsString,
) -> Result<Read, Fault> {
    read_conversion(heap, task, base, &Key::from_name(name))
        .map(|v| v.unwrap_or(Read::Value(Value::Undefined)))
}

/// Preserve conversion's fallback past unavailable Function.prototype methods
/// when a transparent Proxy forwards the lookup, including through prototypes.
pub(super) fn read_conversion(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
) -> Result<Option<Read>, Fault> {
    lookup(heap, task, base, key, false, true)
}

/// What the receiver's part of a write did.
enum Stored {
    MutableStruct,
    Exotic(Exotic),
    Done,
    Setter(Value),
    /// An uninterned key reached an ordinary object: intern its name
    /// (outside the object lock) and try again.
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
        SetOutcome::HeapLimit => Err(Fault::HeapLimit),
        SetOutcome::Setter(setter) => Ok(Stored::Setter(setter)),
        SetOutcome::ReadOnly => Ok(Stored::Rejected(read_only(key))),
        SetOutcome::NotExtensible => Ok(Stored::Rejected(not_extensible(key))),
        SetOutcome::Inherited => prototype
            .map(Stored::Inherited)
            .ok_or(Fault::Internal("an inherited write without a prototype")),
    }
}

/// The receiver's part of `key = value` on `object` for `task`.
fn store(
    heap: &Heap,
    object: &mut HeapObject,
    task: TaskId,
    key: &Key,
    interned: Option<PropertyKey>,
    value: &Value,
    cache: Option<&InlineCache>,
) -> Result<Stored, Fault> {
    if let Some(route) = exotic(object, task, key)? {
        return Ok(Stored::Exotic(route));
    }
    let named = |o: &mut OrdinaryObject| {
        if string_wrapper_flags(o, key).is_some() {
            return Ok(Stored::Rejected(read_only(key)));
        }
        let Some(k) = interned else {
            return Ok(Stored::NeedsName);
        };
        let prototype = o.prototype();
        let outcome = match cache {
            Some(cache) => o.set_cached(k, value.clone(), cache),
            None => o.set(k, value.clone()),
        };
        stored(outcome, key, prototype)
    };
    match object {
        HeapObject::Plain(o) if o.is_intrinsic() => Err(immutable(key)),
        HeapObject::Builtin(_) => Err(immutable(key)),
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => Ok(Stored::Function),
        HeapObject::Plain(o) => {
            check_write(o, task)?;
            named(o)
        }
        HeapObject::Promise(Promise {
            properties: Some(o),
            ..
        }) => {
            check_write(o, task)?;
            named(o)
        }
        HeapObject::JsMap(m) | HeapObject::JsSet(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::WeakRef(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::FinalizationRegistry(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::DisposableStack(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::ArrayBuffer(b) => {
            check_write(&b.object, task)?;
            named(&mut b.object)
        }
        HeapObject::Generator(b) => {
            check_write(&b.object, task)?;
            named(&mut b.object)
        }
        HeapObject::TypedArray(a) => {
            check_write(&a.object, task)?;
            named(&mut a.object)
        }
        HeapObject::Array(a) => {
            check_write(a.object(), task)?;
            match key {
                Key::Index(index) => {
                    let prototype = a.object().prototype();
                    stored(a.set(*index, value.clone()), key, prototype)
                }
                _ if key.is_length() && !a.length_writable() => {
                    Ok(Stored::Rejected(read_only(key)))
                }
                _ if key.is_length() => {
                    let desc = length_descriptor(&PropertyDescriptor {
                        value: Some(value.clone()),
                        ..PropertyDescriptor::default()
                    })?;
                    match defined(a.define_length(&desc), key) {
                        Ok(()) => Ok(Stored::Done),
                        Err(fault) => Ok(Stored::Rejected(fault)),
                    }
                }
                Key::Name(_) | Key::Symbol(_) => named(a.object_mut()),
            }
        }
        // A shareable regex value: `lastIndex` stays 0
        // (`crate::regexp::shared`); nothing can be added.
        HeapObject::Form(f) if let Some(cell) = f.regex_cell() => {
            if is_last_index(key) {
                cell.set_last_index(heap, task, value)?;
                Ok(Stored::Done)
            } else {
                Ok(Stored::Rejected(not_extensible(key)))
            }
        }
        // A frozen object: a field is read-only, anything else cannot be
        // added (strict mode throws, sloppy mode ignores).
        HeapObject::Form(f) if matches!(key, Key::Name(name) if form::struct_own_writable(f, name)) => {
            Ok(Stored::MutableStruct)
        }
        HeapObject::Form(f) => Ok(Stored::Rejected(match key {
            Key::Name(name) if form::struct_own_property(f, name).is_some() => read_only(key),
            _ => not_extensible(key),
        })),
        // An occurrence wraps a form (or a scalar): nothing can be added.
        HeapObject::Occurrence(_) => Ok(Stored::Rejected(not_extensible(key))),
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
#[cfg(test)]
fn put(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    value: Value,
    strict: bool,
) -> Result<(), Fault> {
    match prepare_put(heap, task, base, key, value, strict)? {
        None => Ok(()),
        Some(_) => Err(Fault::Unsupported(
            "this operation requires a resumable property write".into(),
        )),
    }
}

/// A property write can finish, request a setter, or dispatch an exotic
/// continuation. No heap borrow survives this result.
pub(crate) enum Write {
    Complete(Option<Value>),
    MutableStruct,
    Proxy(ObjectId),
    Typed(crate::object::typed_array::View, f64),
}

/// Leaf clients forward unsupported proxy traps and require primitive typed
/// array values; VM clients use `prepare_write` to resume those operations.
pub(crate) fn prepare_put(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    value: Value,
    strict: bool,
) -> Result<Option<Value>, Fault> {
    match prepare_write(heap, task, base, key, value.clone(), strict)? {
        Write::Complete(setter) => Ok(setter),
        Write::Proxy(id) => {
            let target = super::proxy::forward(heap, task, id, "set")?;
            prepare_put(heap, task, &Value::Object(target), key, value, strict)
        }
        Write::Typed(view, index) => view.set(heap, index, &value).map(|()| None),
        Write::MutableStruct => Err(Fault::Unsupported(
            "a mutable struct write requires its runtime resolver".into(),
        )),
    }
}

/// Apply data writes and classify exotic routing in the same table access.
pub(crate) fn prepare_write(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    value: Value,
    strict: bool,
) -> Result<Write, Fault> {
    prepare_write_cached(heap, task, base, key, value, strict, None)
}

pub(crate) fn prepare_write_named(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    name: &NamedProperty,
    value: Value,
    strict: bool,
    cache: &InlineCache,
) -> Result<Write, Fault> {
    prepare_write_cached(
        heap,
        task,
        base,
        &name.key,
        value,
        strict,
        Some((name.intern(heap)?, cache)),
    )
}

fn prepare_write_cached(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    value: Value,
    strict: bool,
    cache: Option<(PropertyKey, &InlineCache)>,
) -> Result<Write, Fault> {
    let Value::Object(mut id) = *base else {
        if matches!(
            base,
            Value::Number(_)
                | Value::String(_)
                | Value::Bool(_)
                | Value::BigInt(_)
                | Value::Symbol(_)
        ) && !(matches!(base, Value::String(s) if string_own_flags(s, key).is_some()))
        {
            let prototype = primitive_prototype(heap, base, key)?;
            if let Ok(Chain::Setter(setter)) =
                check_inherited(heap, task, prototype, key.lookup(heap), key)
            {
                return setter_result(setter, key, strict).map(Write::Complete);
            }
        }
        return put_primitive(base, key, strict).map(|()| Write::Complete(None));
    };
    // Existing names need one key lookup. A new name is interned only when
    // ordinary storage requests it, after exotic and ownership checks.
    let mut interned = cache.map(|(key, _)| key).or_else(|| match key {
        Key::Name(_) | Key::Symbol(_) => key.lookup(heap),
        Key::Index(_) => None,
    });
    // Accessor calls precede data-property mutation rules. An intrinsic
    // setter may accept a no-op even on a shared receiver; a function's
    // inherited setter must run without creating a property record.
    if heap.realm().get().is_some_and(|r| r.view(id).is_some()) {
        match check_inherited(heap, task, id, interned.or_else(|| key.lookup(heap)), key) {
            Ok(Chain::Setter(setter)) => {
                return setter_result(setter, key, strict).map(Write::Complete);
            }
            Ok(Chain::Ignored) => return Ok(Write::Complete(None)),
            // The store below applies the receiver's data-write policy:
            // immutable intrinsics always throw; function metadata follows
            // the caller's strictness.
            // Missing Function.prototype methods also belong to that
            // policy: intrinsic writes still throw the immutable TypeError.
            Ok(Chain::Add) | Err(Fault::Unsupported(_)) => {}
            Err(Fault::TypeError(message)) if !message.starts_with("unsupported Proxy trap:") => {}
            Err(fault) => return Err(fault),
        }
    }
    // `[[Set]]` returned false (module docs).
    let rejected = |fault: Fault| {
        if strict {
            Err(fault)
        } else {
            Ok(Write::Complete(None))
        }
    };
    let prototype = loop {
        let outcome = heap
            .with_object_mut(id, |object| {
                store(
                    heap,
                    object,
                    task,
                    key,
                    interned,
                    &value,
                    cache.map(|(_, cache)| cache),
                )
            })
            .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
        match outcome {
            Stored::MutableStruct => return Ok(Write::MutableStruct),
            Stored::Exotic(Exotic::Proxy) => {
                return Ok(Write::Proxy(id));
            }
            Stored::Exotic(Exotic::Namespace(_)) => return rejected(read_only(key)),
            Stored::Exotic(Exotic::Typed(view, index)) => {
                if matches!(value, Value::Object(_)) {
                    return Ok(Write::Typed(view, index));
                }
                view.set(heap, index, &value)?;
                return Ok(Write::Complete(None));
            }
            Stored::Done => return Ok(Write::Complete(None)),
            Stored::Setter(setter) => {
                return setter_result(setter, key, strict).map(Write::Complete);
            }
            Stored::NeedsName => interned = Some(key.intern(heap)?),
            Stored::Inherited(prototype) => break prototype,
            Stored::Rejected(fault) => return rejected(fault),
            Stored::Function => {
                // Functions may inherit setters before their mutable property
                // record exists. Defer this walk until classification actually
                // encounters a function, avoiding a separate ordinary probe.
                match check_inherited(heap, task, id, interned.or_else(|| key.lookup(heap)), key) {
                    Ok(Chain::Setter(setter)) => {
                        return setter_result(setter, key, strict).map(Write::Complete);
                    }
                    Ok(Chain::Ignored) => return Ok(Write::Complete(None)),
                    Ok(Chain::Add) | Err(Fault::Unsupported(_)) => {}
                    Err(Fault::TypeError(message))
                        if !message.starts_with("unsupported Proxy trap:") => {}
                    Err(fault) => return Err(fault),
                }
                if function::rejects_metadata_write(heap, task, id, key)? {
                    return rejected(read_only(key));
                }
                id = function::write_target(heap, task, id, key)?;
            }
        }
    };
    // An index name that was never interned is on no ordinary object of
    // the chain, but may still be an element of an array on it.
    let chain_key = match interned {
        Some(k) => Some(k),
        None => key.lookup(heap),
    };
    match check_inherited(heap, task, prototype, chain_key, key) {
        Ok(Chain::Add) => {}
        Ok(Chain::Ignored) => return Ok(Write::Complete(None)),
        Ok(Chain::Setter(setter)) => {
            return setter_result(setter, key, strict).map(Write::Complete);
        }
        // Sloppy code ignores an ordinary read-only rejection, while an
        // unsupported proxy trap remains an exception.
        Err(Fault::TypeError(message)) if !message.starts_with("unsupported Proxy trap:") => {
            return rejected(Fault::TypeError(message));
        }
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
            (
                HeapObject::Promise(Promise {
                    properties: Some(o),
                    ..
                }),
                _,
            ) => Ok(added(chain_key.is_some_and(|k| o.add(k, value)))),
            (HeapObject::Plain(o), _) => Ok(added(chain_key.is_some_and(|k| o.add(k, value)))),
            (HeapObject::JsMap(m) | HeapObject::JsSet(m), _) => Ok(added(
                chain_key.is_some_and(|k| m.object_mut().add(k, value)),
            )),
            (HeapObject::WeakMap(m) | HeapObject::WeakSet(m), _) => Ok(added(
                chain_key.is_some_and(|k| m.object_mut().add(k, value)),
            )),
            (HeapObject::WeakRef(m), _) => Ok(added(
                chain_key.is_some_and(|k| m.object_mut().add(k, value)),
            )),
            (HeapObject::FinalizationRegistry(m), _) => Ok(added(
                chain_key.is_some_and(|k| m.object_mut().add(k, value)),
            )),
            (HeapObject::DisposableStack(m), _) => Ok(added(
                chain_key.is_some_and(|k| m.object_mut().add(k, value)),
            )),

            (HeapObject::ArrayBuffer(b), _) => {
                Ok(added(chain_key.is_some_and(|k| b.object.add(k, value))))
            }
            (HeapObject::Generator(b), _) => {
                Ok(added(chain_key.is_some_and(|k| b.object.add(k, value))))
            }
            (HeapObject::TypedArray(a), _) => {
                Ok(added(chain_key.is_some_and(|k| a.object.add(k, value))))
            }
            _ => Err(Fault::Internal("property base changed kind")),
        })
        .unwrap_or(Err(Fault::Internal("property base was reclaimed")))?;
    match added {
        SetOutcome::Done => Ok(Write::Complete(None)),
        SetOutcome::HeapLimit => Err(Fault::HeapLimit),
        SetOutcome::Setter(setter) => setter_result(setter, key, strict).map(Write::Complete),
        SetOutcome::ReadOnly => rejected(read_only(key)),
        SetOutcome::NotExtensible => rejected(not_extensible(key)),
        SetOutcome::Inherited => Err(Fault::Internal("an inherited write was not decided")),
    }
}

fn setter_result(setter: Value, key: &Key, strict: bool) -> Result<Option<Value>, Fault> {
    if setter == Value::Undefined {
        if strict {
            Err(read_only(key))
        } else {
            Ok(None)
        }
    } else {
        Ok(Some(setter))
    }
}

/// Super [[Set]] first consults the lookup base, independently of the receiver.
pub(crate) enum SuperWrite {
    Setter(Value),
    Receiver,
    Done,
}

pub(crate) fn super_write(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Key,
    strict: bool,
) -> Result<SuperWrite, Fault> {
    let Value::Object(base) = base else {
        return Err(Fault::TypeError("super lookup base is null".into()));
    };
    match check_inherited(heap, task, *base, key.lookup(heap), key) {
        Ok(Chain::Add) => Ok(SuperWrite::Receiver),
        Ok(Chain::Ignored) => Ok(SuperWrite::Done),
        Ok(Chain::Setter(setter)) => Ok(match setter_result(setter, key, strict)? {
            Some(setter) => SuperWrite::Setter(setter),
            None => SuperWrite::Done,
        }),
        Err(Fault::TypeError(message))
            if !strict && !message.starts_with("unsupported Proxy trap:") =>
        {
            Ok(SuperWrite::Done)
        }
        Err(fault) => Err(fault),
    }
}

/// OrdinarySetWithOwnDescriptor's receiver check. An own accessor on the
/// receiver rejects a data write; it must never be called as a setter.
pub(crate) fn super_receiver_descriptor(
    heap: &Heap,
    task: TaskId,
    receiver: &Value,
    key: &Key,
    value: Value,
    strict: bool,
) -> Result<Option<PropertyDescriptor>, Fault> {
    let rejected = || {
        if strict {
            Err(read_only(key))
        } else {
            Ok(None)
        }
    };
    let Value::Object(id) = receiver else {
        return rejected();
    };
    check_super_receiver_mutability(heap, *id, key)?;
    let own = get_own_attributes(heap, task, *id, key)?;
    if let Some(own) = &own
        && (own.accessor.is_some() || !own.flags.writable)
    {
        return rejected();
    }
    let desc = PropertyDescriptor {
        value: Some(value),
        writable: own.is_none().then_some(true),
        enumerable: own.is_none().then_some(true),
        configurable: own.is_none().then_some(true),
        ..PropertyDescriptor::default()
    };
    match typed_array_descriptor(heap, task, *id, key, &desc) {
        Err(Fault::TypeError(_)) if !strict => Ok(None),
        Err(fault) => Err(fault),
        Ok(_) => Ok(Some(desc)),
    }
}

pub(crate) fn put_super_receiver(
    heap: &Heap,
    task: TaskId,
    receiver: ObjectId,
    key: &Key,
    desc: &PropertyDescriptor,
    strict: bool,
) -> Result<(), Fault> {
    // Sharing/ownership faults are runtime mutation policy, even in sloppy
    // code. Resolve function storage before suppressing ordinary rejection.
    let receiver = if is_function(heap, receiver) {
        function::define_target(heap, task, receiver, key)?
    } else {
        receiver
    };
    check_super_receiver_mutability(heap, receiver, key)?;
    match define_own_property(heap, task, receiver, key, desc) {
        Err(Fault::TypeError(message))
            if !strict && !message.starts_with("unsupported Proxy trap:") =>
        {
            Ok(())
        }
        result => result,
    }
}

fn check_super_receiver_mutability(
    heap: &Heap,
    receiver: ObjectId,
    key: &Key,
) -> Result<(), Fault> {
    let immutable = heap
        .read(receiver, |object| {
            matches!(object, HeapObject::Plain(o) if o.is_intrinsic())
                || matches!(object, HeapObject::Builtin(_))
        })
        .ok_or(Fault::Internal("super receiver was reclaimed"))?;
    if immutable {
        return Err(immutable_define(key));
    }
    Ok(())
}

/// `base[key] = value` on a primitive `base` (module docs, "Primitive
/// bases"; ECMAScript PutValue: OrdinarySet on ToObject(base) with the
/// primitive as the receiver, which never creates a wrapper here).
/// `undefined`/`null` are a TypeError in both modes. A number, string or
/// boolean can take no property: a string's own index or `length` is
/// read-only, an inherited data property or an absent one needs an object
/// receiver, so `[[Set]]` returns false (a TypeError in strict code,
/// nothing in sloppy code). The one exception is `__proto__`, whose
/// `Object.prototype` setter accepts a primitive `this` and does nothing.
fn put_primitive(base: &Value, key: &Key, strict: bool) -> Result<(), Fault> {
    let what = match base {
        Value::Undefined | Value::Null => return Err(primitive_base(base, key, Access::Write)),
        Value::Object(_) => return Err(Fault::Internal("an object base is not primitive")),
        other => other.type_name(),
    };
    if !strict {
        return Ok(());
    }
    let own = match base {
        Value::String(s) => string_own_flags(s, key).is_some(),
        _ => false,
    };
    Err(Fault::TypeError(if own {
        format!("cannot assign to read-only property '{key}' of a string")
    } else {
        format!(
            "cannot create property '{key}' on a {what}: primitive values have no own properties"
        )
    }))
}

/// `base.name = value` (`PutField`).
#[cfg(test)]
fn put_field(
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
#[derive(Debug, PartialEq)]
enum Chain {
    /// The receiver gets a new own data property.
    Add,
    /// An invalid integer index on a typed-array prototype is a no-op.
    Ignored,
    Setter(Value),
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
            match k.and_then(|k| view.get_own(k)) {
                Some(property) if property.accessor.is_some() => {
                    return Ok(Chain::Setter(property.accessor.as_ref().unwrap().1.clone()));
                }
                Some(property) if !property.flags.writable => return Err(read_only(key)),
                Some(_) => return Ok(Chain::Add),
                None if view.function_prototype && function::lacks_inherited(key) => {
                    return Err(function_prototype_miss(key, Access::Write));
                }
                None => match view.prototype {
                    Some(prototype) => {
                        id = prototype;
                        continue;
                    }
                    None => return Ok(Chain::Add),
                },
            }
        }
        let in_store = |o: &OrdinaryObject| {
            if string_wrapper_flags(o, key).is_some() {
                return Err(read_only(key));
            }
            match k.and_then(|k| o.get_own(k)) {
                Some(property) if property.accessor.is_some() => {
                    Ok(Walk::Setter(property.accessor.as_ref().unwrap().1.clone()))
                }
                Some(property) if !property.flags.writable => Err(read_only(key)),
                Some(_) => Ok(Walk::Next(None)),
                None => Ok(Walk::Next(o.prototype())),
            }
        };
        let next = heap
            .read(id, |object| {
                if let Some(route) = exotic(object, task, key)? {
                    return Ok(Walk::Exotic(route));
                }
                match visit(object, task)? {
                    Visit::Store(o) => in_store(o),
                    Visit::Array(a) => match key {
                        Key::Index(index) => match a.element(*index) {
                            Some(element) if element.accessor.is_some() => {
                                Ok(Walk::Setter(element.accessor.unwrap().1))
                            }
                            Some(element) if !element.flags.writable => Err(read_only(key)),
                            Some(_) => Ok(Walk::Next(None)),
                            None => Ok(Walk::Next(a.object().prototype())),
                        },
                        _ if key.is_length() && !a.length_writable() => Err(read_only(key)),
                        _ if key.is_length() => Ok(Walk::Next(None)),
                        _ => in_store(a.object()),
                    },
                    // A function as a prototype (`Object.create(f)`,
                    // clj-quickjs-121): its own properties are in its record
                    // (clj-quickjs-125).
                    Visit::Other(HeapObject::Function(_) | HeapObject::BoundFunction(_)) => {
                        Ok(Walk::Function)
                    }
                    Visit::Other(HeapObject::Promise(p)) => {
                        p.check_owner(task).map(|()| Walk::Next(p.prototype()))
                    }
                    // A regex value as a prototype: its `lastIndex` is a
                    // writable data property, so the receiver may add its
                    // own; other keys continue at RegExp.prototype
                    // (`crate::regexp::shared`).
                    Visit::Other(HeapObject::Form(f)) if f.regex_cell().is_some() => {
                        Ok(if is_last_index(key) {
                            Walk::Next(None)
                        } else {
                            Walk::RegExpPrototype
                        })
                    }
                    // A form as a prototype: a struct's field is frozen, so
                    // a shadowing write is rejected as on any frozen
                    // prototype; other names continue at Object.prototype.
                    // An occurrence continues at the object it wraps.
                    Visit::Other(HeapObject::Form(f)) => match key {
                        Key::Name(name) if form::struct_own_property(f, name).is_some() => {
                            if form::struct_own_writable(f, name) {
                                Ok(Walk::Next(None))
                            } else {
                                Err(read_only(key))
                            }
                        }
                        _ => Ok(Walk::ObjectPrototype),
                    },
                    Visit::Other(HeapObject::Occurrence(o)) => Ok(Walk::Next(match o.value {
                        Value::Object(inner) => Some(inner),
                        _ => None,
                    })),
                    // Another kind as a prototype: one without JavaScript
                    // properties ends the walk.
                    Visit::Other(other) => match not_ordinary(other, key, Access::Read) {
                        Some(fault) => Err(fault),
                        None => Ok(Walk::Next(None)),
                    },
                }
            })
            .unwrap_or(Err(Fault::Internal("prototype was reclaimed")))?;
        match next {
            Walk::Exotic(Exotic::Proxy) => id = super::proxy::forward(heap, task, id, "set")?,
            Walk::Exotic(Exotic::Namespace(_)) => return Err(read_only(key)),
            Walk::Exotic(Exotic::Typed(view, index)) => {
                return Ok(if view.get(heap, index)?.is_some() {
                    Chain::Add
                } else {
                    Chain::Ignored
                });
            }
            Walk::Setter(setter) => return Ok(Chain::Setter(setter)),
            Walk::ObjectPrototype => match realm.map(|r| r.object_prototype) {
                Some(prototype) => id = prototype,
                None => return Ok(Chain::Add),
            },
            Walk::RegExpPrototype => {
                id = crate::realm::intrinsics(heap)
                    .ok_or(Fault::OutOfMemory("RegExp realm"))?
                    .regexp_prototype;
                realm = heap.realm().get();
            }
            Walk::Next(Some(prototype)) => id = prototype,
            Walk::Next(None) => return Ok(Chain::Add),
            // Continue at the store that has the function's own `key`, or
            // where its read would continue; a name `Function.prototype`
            // will own fails (`crate::vm::function::read`). The step may
            // build the realm to continue at `Object.prototype`.
            Walk::Function => {
                // Probe a lazy prototype without materializing it: setter
                // lookup must not create task-owned state, especially on a
                // published function whose writes must stay catchable errors.
                let property = if function::property_record(heap, id)?.is_some() {
                    get_own_property(heap, task, id, key)?
                } else {
                    function::own_metadata(heap, task, id, key)?
                };
                if let Some(property) = property {
                    if let Some((_, setter)) = property.accessor {
                        return Ok(Chain::Setter(setter));
                    }
                    if !property.flags.writable {
                        return Err(read_only(key));
                    }
                    return Ok(Chain::Add);
                }
                id = match function::probe(heap, task, id, key)? {
                    FunctionRead::Found(_) => return Ok(Chain::Add),
                    FunctionRead::Continue(next) => next,
                };
                realm = heap.realm().get();
            }
        }
    }
}

/// One step of [`check_inherited`] outside the realm's views.
enum Walk {
    Exotic(Exotic),
    Setter(Value),
    /// Continue at this prototype, or (`None`) stop: the receiver may add
    /// the property.
    Next(Option<ObjectId>),
    /// Continue at `Object.prototype` (a native form's prototype).
    ObjectPrototype,
    /// Continue at `RegExp.prototype` (a shareable regex value's).
    RegExpPrototype,
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
    define_field_interned(heap, task, object, name, value, intern(heap, name)?)
}

pub(crate) fn define_named(
    heap: &Heap,
    task: TaskId,
    object: &Value,
    name: &NamedProperty,
    value: Value,
) -> Result<(), Fault> {
    define_field_interned(heap, task, object, &name.name, value, name.intern(heap)?)
}

fn define_field_interned(
    heap: &Heap,
    task: TaskId,
    object: &Value,
    name: &JsString,
    value: Value,
    key: PropertyKey,
) -> Result<(), Fault> {
    let Value::Object(id) = *object else {
        return Err(Fault::Unsupported(format!(
            "define_field needs an object, got {}",
            object.type_name()
        )));
    };
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
/// `length` (`ArrayObject::define_element`, `ArrayObject::define_length`,
/// clj-quickjs-154); a realm intrinsic
/// is a TypeError (immutable); another task's object is `ForeignObject`; a
/// function's property goes to its property record
/// (`crate::vm::function::define_target`, clj-quickjs-125); the other heap
/// kinds have no properties and are
/// not extensible (TypeError).
pub(crate) fn typed_array_descriptor(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
    desc: &PropertyDescriptor,
) -> Result<Option<(crate::object::typed_array::View, f64)>, Fault> {
    let Some(index) = crate::object::typed_array::numeric_key(key) else {
        return Ok(None);
    };
    let Some(view) = crate::object::typed_array::view(heap, task, target)? else {
        return Ok(None);
    };
    if view.get(heap, index)?.is_none()
        || desc.get.is_some()
        || desc.set.is_some()
        || desc.configurable == Some(false)
        || desc.enumerable == Some(false)
        || desc.writable == Some(false)
    {
        return Err(Fault::TypeError(
            "invalid typed array element descriptor".into(),
        ));
    }
    Ok(Some((view, index)))
}

/// Whether `key` names a writable mutable deftype field of `target`
/// (`crate::collections::mutable_fields`), and whether its cell is
/// published. A store to such a field, however reached (`Reflect.set`, a
/// transparent Proxy), goes through the runtime's ordered field setter
/// ([`Write::MutableStruct`]), and a published field's value is read
/// through its ordered member read, never copied here.
pub(crate) fn mutable_field(heap: &Heap, target: ObjectId, key: &Key) -> Option<(bool, bool)> {
    let Key::Name(name) = key else {
        return None;
    };
    heap.read(target, |o| match o {
        HeapObject::Form(f) => form::mutable_field(f, name)
            .map(|(cell, index)| (cell.writable(index), cell.is_shared())),
        _ => None,
    })
    .flatten()
}

/// The current value of the mutable deftype field `key` of `target`.
pub(crate) fn mutable_field_value(heap: &Heap, target: ObjectId, key: &Key) -> Option<Value> {
    let Key::Name(name) = key else {
        return None;
    };
    heap.read(target, |o| match o {
        HeapObject::Form(f) => {
            form::mutable_field(f, name).and_then(|(cell, index)| cell.get(index))
        }
        _ => None,
    })
    .flatten()
}

/// Store `value` in the mutable deftype field `key` of `target`, unordered:
/// for a task without the compiler's resolver.
pub(crate) fn store_mutable_field(
    heap: &Heap,
    target: ObjectId,
    key: &Key,
    value: Value,
) -> Result<(), Fault> {
    let Key::Name(name) = key else {
        return Err(Fault::Internal("mutable field store without a name"));
    };
    let (cell, index) = heap
        .read(target, |o| match o {
            HeapObject::Form(f) => form::mutable_field(f, name),
            _ => None,
        })
        .flatten()
        .ok_or(Fault::Internal("mutable field store target"))?;
    cell.set(heap, index, value)
}

/// Whether a descriptor only replaces a value: the shape OrdinarySet
/// defines on a receiver that already owns a writable data property.
pub(crate) fn value_only(desc: &PropertyDescriptor) -> bool {
    desc.value.is_some()
        && desc.get.is_none()
        && desc.set.is_none()
        && desc.writable.is_none()
        && desc.enumerable.is_none()
        && desc.configurable.is_none()
}

pub(crate) fn define_own_property(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
    desc: &PropertyDescriptor,
) -> Result<(), Fault> {
    let target = super::proxy::forward(heap, task, target, "defineProperty")?;
    if let Some(entry) = crate::module::namespace::probe(heap, task, target, key)? {
        return crate::module::namespace::define(heap, task, entry, desc);
    }
    if let Some((view, index)) = typed_array_descriptor(heap, task, target, key, desc)? {
        if let Some(value) = &desc.value {
            view.set(heap, index, value)?;
        }
        return Ok(());
    }
    if let Key::Name(name) = key
        && let Some((cell, index)) = heap
            .read(target, |o| match o {
                HeapObject::Form(f) => form::mutable_field(f, name),
                _ => None,
            })
            .flatten()
    {
        return cell.define(heap, index, desc);
    }
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
        false => Some(key.intern(heap)?),
    };
    let named = |o: &mut OrdinaryObject| {
        // Validated and discarded within this step, so not charged.
        if let Some(property) = wrapped_string(o).and_then(|s| string_own_transient(s, key)) {
            return match crate::object::validate_and_apply(Some(&property), desc) {
                Ok(_) => Ok(()),
                Err(outcome) => defined(outcome, key),
            };
        }
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
        HeapObject::Promise(Promise {
            properties: Some(o),
            ..
        }) => {
            check_write(o, task)?;
            named(o)
        }
        HeapObject::JsMap(m) | HeapObject::JsSet(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::WeakRef(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::FinalizationRegistry(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::DisposableStack(m) => {
            check_write(m.object(), task)?;
            named(m.object_mut())
        }
        HeapObject::ArrayBuffer(b) => {
            check_write(&b.object, task)?;
            named(&mut b.object)
        }
        HeapObject::Generator(b) => {
            check_write(&b.object, task)?;
            named(&mut b.object)
        }
        HeapObject::TypedArray(a) => {
            check_write(&a.object, task)?;
            named(&mut a.object)
        }
        HeapObject::Array(a) => {
            check_write(a.object(), task)?;
            match key {
                Key::Index(index) => defined(a.define_element(*index, desc), key),
                _ if key.is_length() => defined(a.define_length(&length_descriptor(desc)?), key),
                Key::Name(_) | Key::Symbol(_) => named(a.object_mut()),
            }
        }
        HeapObject::Function(_) | HeapObject::BoundFunction(_) => unsupported("a function"),
        // A shareable regex value's `lastIndex` stays a writable 0
        // (`crate::regexp::shared`); nothing can be added.
        HeapObject::Form(f) if let Some(cell) = f.regex_cell() => {
            if !is_last_index(key) {
                return Err(not_extensible(key));
            }
            let current = Property {
                value: cell.last_index(task),
                flags: LAST_INDEX,
                accessor: None,
            };
            match crate::object::validate_and_apply(Some(&current), desc) {
                Ok(_) if desc.writable == Some(false) => Err(Fault::TypeError(
                    "unsupported operation: a shared regex value's lastIndex stays writable".into(),
                )),
                Ok(_) => match &desc.value {
                    Some(value) => cell.set_last_index(heap, task, value),
                    None => Ok(()),
                },
                Err(outcome) => defined(outcome, key),
            }
        }
        // A frozen object: a field accepts only a compatible descriptor
        // (ValidateAndApplyPropertyDescriptor), nothing can be added.
        HeapObject::Form(f) => match key {
            Key::Name(name) => match form::struct_own_property(f, name) {
                Some(value) => {
                    let current = Property {
                        value,
                        flags: STRUCT_FIELD,
                        accessor: None,
                    };
                    match crate::object::validate_and_apply(Some(&current), desc) {
                        Ok(_) => Ok(()),
                        Err(outcome) => defined(outcome, key),
                    }
                }
                None => Err(not_extensible(key)),
            },
            _ => Err(not_extensible(key)),
        },
        other => Err(Fault::TypeError(format!(
            "cannot define property '{key}' of a {}: it has no JavaScript properties",
            object_name(other)
        ))),
    })
    .unwrap_or(Err(Fault::Internal("property target was reclaimed")))
}

/// ArraySetLength's ToUint32/ToNumber validation. For primitive values both
/// conversions are pure, so the equality holds exactly for finite integers
/// in 0..=2^32-1 (including -0, normalized to +0). Objects still fail
/// explicitly through ToPrimitive; no JavaScript runs under an object lock.
fn length_descriptor(desc: &PropertyDescriptor) -> Result<PropertyDescriptor, Fault> {
    let mut desc = desc.clone();
    if let Some(value) = &desc.value {
        let number = super::dispatch::to_number(value)?;
        if !(0.0..=f64::from(u32::MAX)).contains(&number) || number.fract() != 0.0 {
            return Err(Fault::RangeError("invalid array length".to_string()));
        }
        desc.value = Some(Value::Number(f64::from(number as u32)));
    }
    Ok(desc)
}

/// Accessor literal definitions merge the two halves, preserving the
/// other half only when the previous definition was also an accessor.
pub(crate) fn define_accessor(
    heap: &Heap,
    task: TaskId,
    object: &Value,
    name: &JsString,
    function: Value,
    getter: bool,
) -> Result<(), Fault> {
    let Value::Object(id) = object else {
        return Err(Fault::Internal("accessor literal without object"));
    };
    let key = intern(heap, name)?;
    heap.with_object_mut(*id, |target| match target {
        HeapObject::Plain(o) => {
            check_write(o, task)?;
            let desc = PropertyDescriptor {
                get: getter.then_some(function.clone()),
                set: (!getter).then_some(function),
                enumerable: Some(true),
                configurable: Some(true),
                ..PropertyDescriptor::default()
            };
            defined(o.define_own_property(key, &desc), &Key::from_name(name))
        }
        _ => Err(Fault::Internal("accessor literal on non-ordinary object")),
    })
    .unwrap_or(Err(Fault::Internal(
        "accessor literal object was reclaimed",
    )))
}

/// The fault of a [`DefineOutcome`].
fn defined(outcome: DefineOutcome, key: &Key) -> Result<(), Fault> {
    match outcome {
        DefineOutcome::Done => Ok(()),
        DefineOutcome::HeapLimit => Err(Fault::HeapLimit),
        DefineOutcome::Rejected(reason) => Err(Fault::TypeError(format!(
            "cannot define property '{key}': {reason}"
        ))),
    }
}

/// The own property `key` of the object `target` for the running task
/// `task` (`[[GetOwnProperty]]`, as `Object.getOwnPropertyDescriptor` uses
/// it; clj-quickjs-119): `None` if absent. Ordinary objects (realm
/// intrinsics included), arrays and built-in functions report their own
/// properties; another task's object is `ForeignObject`; a function reports
/// those in its property record (`crate::vm::function::own_store`,
/// clj-quickjs-125); the other heap
/// kinds have none.
pub(crate) fn get_own_property(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    own_property(heap, task, target, key, true)
}

/// [`get_own_property`] for a caller that needs only the property's
/// presence, attributes or accessors: a String wrapper's index property
/// has an `undefined` placeholder value, so no index string is made or
/// charged.
pub(crate) fn get_own_attributes(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
) -> Result<Option<Property>, Fault> {
    own_property(heap, task, target, key, false)
}

fn own_property(
    heap: &Heap,
    task: TaskId,
    target: ObjectId,
    key: &Key,
    with_value: bool,
) -> Result<Option<Property>, Fault> {
    enum Own {
        Property(Option<Property>),
        Exotic(Exotic),
        Function,
    }
    let k = key.lookup(heap);
    if let Some(view) = heap.realm().get().and_then(|r| r.view(target)) {
        return Ok(k.and_then(|k| view.get_own(k)).cloned());
    }
    let named = |o: &OrdinaryObject| k.and_then(|k| o.get_own(k)).cloned();
    let own = heap
        .read(target, |object| {
            if let Some(route) = exotic(object, task, key)? {
                return Ok(Own::Exotic(route));
            }
            let property = match visit(object, task)? {
                Visit::Store(o) if with_value => {
                    string_wrapper_property(heap, o, key)?.or_else(|| named(o))
                }
                Visit::Store(o) => string_wrapper_flags(o, key)
                    .map(|flags| Property {
                        accessor: None,
                        value: Value::Undefined,
                        flags,
                    })
                    .or_else(|| named(o)),
                Visit::Array(a) => match key {
                    Key::Index(index) => a.element(*index),
                    _ if key.is_length() => Some(a.length_property()),
                    Key::Name(_) | Key::Symbol(_) => named(a.object()),
                },
                Visit::Other(HeapObject::Function(_) | HeapObject::BoundFunction(_)) => {
                    return Ok(Own::Function);
                }
                // A shareable regex value's own `lastIndex`, always 0.
                Visit::Other(HeapObject::Form(f)) if let Some(cell) = f.regex_cell() => {
                    is_last_index(key).then(|| Property {
                        value: if with_value {
                            cell.last_index(task)
                        } else {
                            Value::Undefined
                        },
                        flags: LAST_INDEX,
                        accessor: None,
                    })
                }
                // A struct's fields are its own frozen data properties.
                Visit::Other(HeapObject::Form(f)) if let Key::Name(name) = key => {
                    form::struct_own_property(f, name).map(|value| Property {
                        value: if with_value { value } else { Value::Undefined },
                        flags: PropertyFlags {
                            writable: form::struct_own_writable(f, name),
                            ..STRUCT_FIELD
                        },
                        accessor: None,
                    })
                }
                Visit::Other(other) => match not_ordinary(other, key, Access::Read) {
                    Some(fault) => return Err(fault),
                    None => None,
                },
            };
            Ok(Own::Property(property))
        })
        .unwrap_or(Err(Fault::Internal("property target was reclaimed")))?;
    match own {
        Own::Property(property) => Ok(property),
        Own::Exotic(Exotic::Proxy) => {
            let target = super::proxy::forward(heap, task, target, "getOwnPropertyDescriptor")?;
            own_property(heap, task, target, key, with_value)
        }
        Own::Exotic(Exotic::Namespace(entry)) => entry
            .map(|e| crate::module::namespace::property(heap, task, e))
            .transpose(),
        Own::Exotic(Exotic::Typed(view, index)) => {
            Ok(view.get(heap, index)?.map(|value| Property {
                value,
                flags: PropertyFlags::DATA,
                accessor: None,
            }))
        }
        Own::Function => {
            if let Some(property) = function::own_metadata(heap, task, target, key)? {
                return Ok(Some(property));
            }
            match function::own_store(heap, task, target, key)? {
                Some(record) => own_property(heap, task, record, key, with_value),
                None => Ok(None),
            }
        }
    }
}

/// Observe the current own enumerable flag without creating a virtual
/// function prototype or property record. No callback or allocation.
pub(crate) fn own_enumerable(
    heap: &Heap,
    task: TaskId,
    source: &Value,
    key: &Key,
) -> Result<bool, Fault> {
    let own = match source {
        Value::Object(id) if is_function(heap, *id) => {
            return function::own_enumerable(heap, task, *id, key);
        }
        Value::Object(id) => get_own_attributes(heap, task, *id, key)?,
        Value::String(s) => return Ok(string_own_flags(s, key).is_some_and(|f| f.enumerable)),
        _ => None,
    };
    Ok(own.is_some_and(|p| p.flags.enumerable))
}

/// True for a function object, whose properties live in a separate record
/// (`crate::vm::function`).
fn is_function(heap: &Heap, id: ObjectId) -> bool {
    heap.read(id, |o| {
        matches!(o, HeapObject::Function(_) | HeapObject::BoundFunction(_))
    })
    .unwrap_or(false)
}

/// `delete base[key]` for the running task `task` (`Delete`,
/// clj-quickjs-124; ECMAScript's `delete` of a property reference: ToObject
/// of the base, ToPropertyKey of the key, then `[[Delete]]`; QuickJS
/// `OP_delete` and `JS_DeleteProperty`). An `undefined`/`null` base is a
/// TypeError before the key is converted. True when the base has no own
/// property `key` afterwards; a non-configurable own property stays, and
/// is a TypeError in strict code and false in sloppy code. Module docs,
/// "Deleting properties".
pub(crate) fn delete(
    heap: &Heap,
    task: TaskId,
    base: &Value,
    key: &Value,
    strict: bool,
) -> Result<bool, Fault> {
    let key = base_key(base, key, Access::Delete)?;
    let outcome = match base {
        Value::Object(id) => delete_own(heap, task, *id, &key)?,
        // The String wrapper ToObject would make: its indices below the
        // length and its `length` are its only own properties, and none of
        // them is configurable.
        Value::String(s) => match string_own_flags(s, &key) {
            Some(_) => DeleteOutcome::NonConfigurable,
            None => DeleteOutcome::Absent,
        },
        // Number and Boolean wrappers have no own properties.
        _ => DeleteOutcome::Absent,
    };
    match outcome {
        DeleteOutcome::Deleted | DeleteOutcome::Absent => Ok(true),
        DeleteOutcome::NonConfigurable if strict => Err(Fault::TypeError(format!(
            "cannot delete non-configurable property '{key}'"
        ))),
        DeleteOutcome::NonConfigurable => Ok(false),
    }
}

/// `[[Delete]]` of the own property `key` of the object `id`, with the
/// owner and kind rules of the module table: an owned ordinary object or
/// array removes it (an element becomes a hole; `length` is not
/// configurable); a function's goes to its property record
/// ([`function::delete_target`]); a realm intrinsic or built-in function
/// that has it is a TypeError in either mode (shared and immutable), one
/// that lacks it has nothing to delete; another task's or an
/// embedder-created object is `ForeignObject`; the other heap kinds have
/// no properties.
fn delete_own(heap: &Heap, task: TaskId, id: ObjectId, key: &Key) -> Result<DeleteOutcome, Fault> {
    let id = super::proxy::forward(heap, task, id, "deleteProperty")?;
    if let Some(entry) = crate::module::namespace::probe(heap, task, id, key)? {
        return Ok(if entry.is_some() {
            DeleteOutcome::NonConfigurable
        } else {
            DeleteOutcome::Absent
        });
    }
    if let Some(element) = typed_element(heap, task, id, key)? {
        return Ok(if element.is_some() {
            DeleteOutcome::NonConfigurable
        } else {
            DeleteOutcome::Absent
        });
    }
    let id = match is_function(heap, id) {
        true => match function::delete_target(heap, task, id, key)? {
            function::FunctionDelete::Record(record) => record,
            function::FunctionDelete::Absent => return Ok(DeleteOutcome::Absent),
            function::FunctionDelete::NonConfigurable => {
                return Ok(DeleteOutcome::NonConfigurable);
            }
        },
        false => id,
    };
    // An array index of an array needs no name; every other key needs its
    // interned key, looked up before the object lock is taken. A name that
    // was never interned is no object's property.
    let element = matches!(key, Key::Index(_))
        && heap
            .read(id, |o| matches!(o, HeapObject::Array(_)))
            .unwrap_or(false);
    let k = match element {
        true => None,
        false => key.lookup(heap),
    };
    let shared = |o: &OrdinaryObject| match k.is_some_and(|k| o.get_own(k).is_some()) {
        true => Err(Fault::TypeError(format!(
            "cannot delete property '{key}' of a built-in object: built-in objects are shared by all tasks and immutable"
        ))),
        false => Ok(DeleteOutcome::Absent),
    };
    let named = |o: &mut OrdinaryObject| k.map_or(DeleteOutcome::Absent, |k| o.delete(k));
    heap.with_object_mut(id, |object| match object {
        HeapObject::Plain(o) if o.is_intrinsic() => shared(o),
        HeapObject::Builtin(f) => shared(&f.properties),
        HeapObject::Plain(o) => {
            check_write(o, task)?;
            if string_wrapper_flags(o, key).is_some() {
                return Ok(DeleteOutcome::NonConfigurable);
            }
            Ok(named(o))
        }
        HeapObject::Promise(Promise {
            properties: Some(o),
            ..
        }) => {
            check_write(o, task)?;
            if string_wrapper_flags(o, key).is_some() {
                return Ok(DeleteOutcome::NonConfigurable);
            }
            Ok(named(o))
        }
        HeapObject::JsMap(m) | HeapObject::JsSet(m) => {
            check_write(m.object(), task)?;
            Ok(named(m.object_mut()))
        }
        HeapObject::WeakMap(m) | HeapObject::WeakSet(m) => {
            check_write(m.object(), task)?;
            Ok(named(m.object_mut()))
        }
        HeapObject::WeakRef(m) => {
            check_write(m.object(), task)?;
            Ok(named(m.object_mut()))
        }
        HeapObject::FinalizationRegistry(m) => {
            check_write(m.object(), task)?;
            Ok(named(m.object_mut()))
        }
        HeapObject::DisposableStack(m) => {
            check_write(m.object(), task)?;
            Ok(named(m.object_mut()))
        }
        HeapObject::Array(a) => {
            check_write(a.object(), task)?;
            Ok(match key {
                Key::Index(index) => a.delete_element(*index),
                _ if key.is_length() => DeleteOutcome::NonConfigurable,
                Key::Name(_) | Key::Symbol(_) => named(a.object_mut()),
            })
        }
        HeapObject::ArrayBuffer(b) => {
            check_write(&b.object, task)?;
            Ok(named(&mut b.object))
        }
        HeapObject::Generator(b) => {
            check_write(&b.object, task)?;
            Ok(named(&mut b.object))
        }
        HeapObject::TypedArray(a) => {
            check_write(&a.object, task)?;
            Ok(named(&mut a.object))
        }
        // A RegExp's `lastIndex` and a struct's fields are non-configurable.
        HeapObject::Form(f) if f.regex_cell().is_some() => Ok(if is_last_index(key) {
            DeleteOutcome::NonConfigurable
        } else {
            DeleteOutcome::Absent
        }),
        HeapObject::Form(f) => Ok(match key {
            Key::Name(name) if form::struct_own_property(f, name).is_some() => {
                DeleteOutcome::NonConfigurable
            }
            _ => DeleteOutcome::Absent,
        }),
        other => match not_ordinary(other, key, Access::Delete) {
            Some(fault) => Err(fault),
            None => Ok(DeleteOutcome::Absent),
        },
    })
    .unwrap_or(Err(Fault::Internal("property base was reclaimed")))
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
    let array = ArrayObject::with_prototype(heap, task, prototype);
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
            // Literal append bypasses descriptor checks and is only valid
            // on a newly created task-owned array, never a template array.
            if a.object().is_intrinsic() {
                return Err(immutable(&Key::Name(JsString::from("length"))));
            }
            check_write(a.object(), task)?;
            a.push(element)
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

    #[test]
    fn static_names_preserve_absence_ownership_and_byte_accounting() {
        let heap = Heap::with_byte_limit(2 * 1024 * 1024);
        let task = TaskId::from_raw(1);
        let id = heap
            .alloc(HeapObject::Plain(OrdinaryObject::new(&heap, Some(task))))
            .unwrap();
        let base = Value::Object(id);
        let name = FixedName::new("static-missing-probe");
        let used = heap.byte_budget().used();
        let keys = heap.keys().len();
        assert!(name.read(&heap, task, &base).ok().unwrap().is_none());
        assert_eq!(heap.keys().len(), keys);
        assert_eq!(heap.byte_budget().used(), used);
        assert!(matches!(
            name.read(&heap, TaskId::from_raw(2), &base),
            Err(Fault::ForeignObject { .. })
        ));

        let key = name.intern(&heap).ok().unwrap();
        assert!(
            heap.with_object_mut(id, |object| match object {
                HeapObject::Plain(object) => object.define(key, Value::Number(7.0)),
                _ => false,
            })
            .unwrap()
        );
        let used = heap.byte_budget().used();
        let releases = crate::value::string::cursor_release_counts().1;
        for _ in 0..100 {
            assert!(matches!(
                name.read(&heap, task, &base).ok().unwrap(),
                Some(Read::Value(Value::Number(7.0)))
            ));
        }
        assert_eq!(crate::value::string::cursor_release_counts().1, releases);
        assert_eq!(heap.byte_budget().used(), used);
    }

    /// A string index read makes a charged string; checks that only need
    /// the property's presence or attributes make none, so they still
    /// succeed with no byte headroom (light-cljs-492).
    #[test]
    fn string_wrapper_attribute_checks_need_no_byte_headroom() {
        #[track_caller]
        fn ok<T>(result: Result<T, Fault>) -> T {
            match result {
                Ok(value) => value,
                Err(Fault::HeapLimit) => panic!("unexpected HeapLimit"),
                Err(Fault::TypeError(message) | Fault::Unsupported(message)) => {
                    panic!("unexpected fault: {message}")
                }
                Err(_) => panic!("unexpected fault"),
            }
        }
        let heap = Heap::with_byte_limit(64 * 1024);
        let task = TaskId::from_raw(1);
        let s = JsString::from("xyz");
        let prototype = crate::realm::intrinsics(&heap).unwrap().string_prototype;
        let wrapper = heap
            .alloc(HeapObject::Plain(
                OrdinaryObject::with_prototype(&heap, task, prototype)
                    .with_primitive_data(crate::object::PrimitiveData::String(s.clone())),
            ))
            .unwrap();
        let key = Key::Index(0);
        // A definition interns its key first; that charge is not at issue.
        ok(key.intern(&heap));
        let budget = heap.byte_budget();
        let used = budget.used();
        budget.set_limit(used);
        for base in [Value::String(s.clone()), Value::Object(wrapper)] {
            assert!(matches!(
                read(&heap, task, &base, &key),
                Err(Fault::HeapLimit)
            ));
            assert!(ok(has_property(&heap, task, &base, &key)));
            assert!(ok(own_enumerable(&heap, task, &base, &key)));
            assert!(!ok(delete(&heap, task, &base, &Value::Number(0.0), false)));
        }
        assert!(matches!(
            string_own_property(&heap, &s, &key),
            Err(Fault::HeapLimit)
        ));
        let own = ok(get_own_attributes(&heap, task, wrapper, &key)).unwrap();
        assert!(own.flags.enumerable && !own.flags.writable && !own.flags.configurable);
        // Redefining the unchanged property validates without a charge.
        for desc in [
            PropertyDescriptor::default(),
            PropertyDescriptor {
                value: Some(Value::String("x".into())),
                ..PropertyDescriptor::default()
            },
        ] {
            ok(define_own_property(&heap, task, wrapper, &key, &desc));
        }
        assert!(
            define_own_property(
                &heap,
                task,
                wrapper,
                &key,
                &PropertyDescriptor {
                    value: Some(Value::String("q".into())),
                    ..PropertyDescriptor::default()
                },
            )
            .is_err()
        );
        assert_eq!(budget.used(), used);
    }

    #[test]
    fn fused_exotic_dispatch_rejects_foreign_owners() {
        use crate::module::namespace::Namespace;
        use crate::object::typed_array::{ElementType, TypedArray, View};
        let heap = Heap::default();
        let owner = TaskId::from_raw(1);
        let caller = TaskId::from_raw(2);
        let ordinary = heap
            .alloc(HeapObject::Plain(OrdinaryObject::new(&heap, Some(owner))))
            .unwrap();
        let namespace = heap
            .alloc(HeapObject::ModuleNamespace(Box::new(Namespace {
                owner,
                exports: Default::default(),
            })))
            .unwrap();
        let typed = heap
            .alloc(HeapObject::TypedArray(Box::new(TypedArray {
                object: OrdinaryObject::new(&heap, Some(owner)),
                view: View {
                    kind: ElementType::Uint8,
                    buffer: ordinary,
                    offset: 0,
                    length: 0,
                },
            })))
            .unwrap();
        let Value::Object(proxy) = super::super::proxy::construct(
            &heap,
            owner,
            &[Value::Object(ordinary), Value::Object(ordinary)],
        )
        .unwrap_or_else(|_| panic!("proxy fixture failed")) else {
            unreachable!()
        };
        for id in [ordinary, namespace, typed, proxy] {
            for key in [Key::Index(0), Key::Name("x".into())] {
                assert!(matches!(
                    read(&heap, caller, &Value::Object(id), &key),
                    Err(Fault::ForeignObject { .. })
                ));
                assert!(matches!(
                    get_own_property(&heap, caller, id, &key),
                    Err(Fault::ForeignObject { owner: Some(actual), caller: actual_caller })
                        if actual == owner && actual_caller == caller
                ));
                assert!(matches!(
                    prepare_put(
                        &heap,
                        caller,
                        &Value::Object(id),
                        &key,
                        Value::Undefined,
                        false
                    ),
                    Err(Fault::ForeignObject { .. })
                ));
                assert!(matches!(
                    check_inherited(&heap, caller, id, key.lookup(&heap), &key),
                    Err(Fault::ForeignObject { .. })
                ));
            }
            assert!(matches!(
                super::super::prototype::get_prototype_of(&heap, caller, id),
                Err(Fault::ForeignObject { .. })
            ));
        }
    }

    /// A native payload without JavaScript properties.
    #[derive(Debug)]
    struct Opaque;

    impl Trace for Opaque {
        fn trace(&self, _visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {}
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
                    read(&object, "call").is_ok_and(|v| matches!(v, Value::Object(_))),
                    read(&object, "bind").is_ok_and(|v| matches!(v, Value::Object(_))),
                    get_own_property(&heap, task, prototype_id(&object), &Key::Name(s("length")))
                        .is_ok_and(|p| p.is_some_and(|p| p.value == Value::Number(1.0))),
                    super::super::prototype::get_prototype_of(
                        &heap,
                        task,
                        prototype_id(&prototype),
                    )
                    .is_ok_and(|p| p.is_none()),
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
        let object = Value::Object(heap.alloc(HeapObject::host_object(&heap)).unwrap());
        assert!(matches!(
            to_property_key(&object),
            Err(Fault::Unsupported(_))
        ));
    }

    #[test]
    fn index_keys_of_ordinary_objects_are_their_names() {
        let heap = Heap::default();
        let task = TaskId::from_raw(1);
        let o = OrdinaryObject::new(&heap, Some(task));
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
            .alloc(HeapObject::Plain(OrdinaryObject::new(&heap, Some(task))))
            .unwrap();
        let desc = PropertyDescriptor {
            value: Some(Value::Number(1.0)),
            ..PropertyDescriptor::default()
        };
        ok(define_own_property(&heap, task, parent, &k, &desc));
        let child = OrdinaryObject::with_prototype(&heap, task, parent);
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
