//! Built-in functions (clj-quickjs-117): the function objects of the realm
//! (`crate::realm`) whose behaviour is Rust code, after QuickJS's
//! `JS_CFUNC_DEF` functions (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8: `js_object_constructor`, `js_object_is`,
//! `js_object_toString`, `js_object_valueOf`; `js_function_proto`,
//! `js_function_call`, `js_function_apply`, `js_reflect_apply`,
//! `build_arg_list`); written fresh over this runtime's values.
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
//! Its `[[Prototype]]` is `Function.prototype` (clj-quickjs-102), itself a
//! built-in function ([`Builtin::FunctionPrototype`]) whose prototype is
//! `Object.prototype`, so `Object.call` and `Object.is.apply` are
//! inherited. A lookup that reaches `Function.prototype` without finding a
//! standard name it lacks so far (`bind`, `toString`, ...;
//! `crate::vm::function::lacks_inherited`) fails with `UnsupportedOperand`
//! instead of answering from `Object.prototype` (`crate::vm::property`).
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
//! # Forwarding built-ins (clj-quickjs-102)
//!
//! `Function.prototype.call`, `Function.prototype.apply` and
//! `Reflect.apply` do nothing but call another function with a receiver
//! and arguments they compute, and return its result unchanged. They are
//! therefore not run by [`call`]: [`forward`] computes the call they make
//! (validating the target and building the argument list, which reads
//! properties but runs no JavaScript), and the caller makes that call *in
//! place of* theirs (`crate::vm::dispatch::resolve`, a tail call): the
//! `Call`/`CallMethod` instruction (or a native's callback step) replaces
//! the forwarding call's operands with the target's and enters a new
//! task-owned VM frame for a function object exactly as a direct call
//! does, or runs a non-forwarding built-in to completion. No native frame
//! is needed, since nothing remains to do after the target returns, and no
//! Rust frame stands for the forwarding built-in, so the callee may yield,
//! wait, be collected around and migrate like any callee. A forwarding
//! target (`f.call.call(g)`) is followed in the same instruction, at most
//! the task's call-depth limit of hops (`StackOverflow` past it: `apply`
//! through an array that holds itself would otherwise never end).
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
//! | `Object.getPrototypeOf(v)` (clj-quickjs-121) | the `[[Prototype]]` of `v` (`crate::vm::prototype`), `null` at the end of a chain; `Function.prototype` for a function or built-in function (clj-quickjs-102); `undefined`/`null` `v`: catchable `TypeError`; a primitive or an error object: `UnsupportedOperand` (wrapper objects and `Error.prototype` do not exist yet) |
//! | `Object.setPrototypeOf(o, proto)` (clj-quickjs-121) | set the `[[Prototype]]` of the object `o` and return `o`; a primitive `o` is returned unchanged; `undefined`/`null` `o`, a `proto` that is neither an object nor `null`, a cycle, a built-in or non-extensible `o`: catchable `TypeError` |
//! | `Object.create(proto)` (clj-quickjs-121) | a new ordinary object owned by the running task with prototype `proto` (an object or `null`, else a catchable `TypeError`); a second argument other than `undefined`: `UnsupportedOperand` (`Object.defineProperties`) |
//! | `Object.preventExtensions(v)`, `Object.freeze(v)` (clj-quickjs-122) | make `v` non-extensible / frozen and return it (`crate::vm::integrity` has the table per kind); a primitive is returned unchanged; a built-in object: catchable `TypeError` (immutable); another task's object: `ForeignObject` |
//! | `Object.isExtensible(v)`, `Object.isFrozen(v)` (clj-quickjs-122) | the `[[Extensible]]` flag / TestIntegrityLevel(frozen); a primitive is not extensible and is frozen; vectors, maps and published functions are frozen (`crate::vm::integrity`) |
//! | `Object.keys(o)`, `Object.getOwnPropertyNames(o)` (clj-quickjs-123) | a fresh array of the own enumerable (`keys`) or all (`getOwnPropertyNames`) string keys of `o`, array indices ascending first, then the other names in creation order (`crate::vm::keys`); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: `UnsupportedOperand` (wrappers, clj-quickjs-72); `getOwnPropertyNames` of a function or of `Object.prototype`: `UnsupportedOperand` (clj-quickjs-101, -261) |
//! | `Number.prototype.toString(radix)`, `.valueOf()` (clj-quickjs-73) | thisNumberValue: a number receiver (a method call on a number primitive passes the number itself, no wrapper is allocated) or `Number.prototype` (`+0`); `toString` gives its Number::toString (`crate::value::string::number_to_string`), with `radix` absent, `undefined` or 10; another radix: `UnsupportedOperand` (clj-quickjs-39); any other receiver: catchable `TypeError` |
//! | `String.prototype.toString()`, `.valueOf()` (clj-quickjs-73) | thisStringValue: a string receiver or `String.prototype` (`""`); any other receiver: catchable `TypeError` |
//! | `Number(...)`, `String(...)` (clj-quickjs-73; global names that carry the prototypes) | `UnsupportedOperand` until the constructors exist (clj-quickjs-72) |
//! | `Function.prototype(...)` (clj-quickjs-102) | `undefined`, whatever the receiver and arguments |
//! | `Function(...)` (clj-quickjs-102; `Function.prototype.constructor`) | `UnsupportedOperand` until the Function constructor exists (clj-quickjs-111) |
//! | `f.call(t, ...args)` (clj-quickjs-102) | forwards (above): calls the receiver `f` with `this` `t` and `args`; a receiver that is not callable: catchable `TypeError` |
//! | `f.apply(t, list)` (clj-quickjs-102) | forwards: calls `f` with `this` `t` and no arguments for an `undefined`/`null` `list`, else CreateListFromArrayLike(`list`): `length` through ToLength, then `list[0 .. length]`; a non-callable `f` or a primitive `list`: catchable `TypeError`; an object `length`: `UnsupportedOperand` (ToPrimitive); a list longer than the value-stack room left: `StackOverflow` (QuickJS: a RangeError) |
//! | `Reflect.apply(f, t, list)` (clj-quickjs-102) | forwards: `f.apply(t, list)` without reading `f.apply`, except that `list` must be an object (`undefined` is a catchable `TypeError`) |

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
use crate::vm::property;
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
    /// `Function.prototype` itself (clj-quickjs-102): callable, accepts any
    /// arguments and returns `undefined` (ECMAScript §20.2.3).
    FunctionPrototype,
    /// `Function.prototype.call` (QuickJS `js_function_call`;
    /// clj-quickjs-102): a forwarding built-in ([`forward`]).
    FunctionPrototypeCall,
    /// `Function.prototype.apply` (QuickJS `js_function_apply`;
    /// clj-quickjs-102): a forwarding built-in.
    FunctionPrototypeApply,
    /// `Reflect.apply` (QuickJS `js_reflect_apply`; clj-quickjs-102): a
    /// forwarding built-in.
    ReflectApply,
    /// `Function` (clj-quickjs-102): the global name and
    /// `Function.prototype.constructor`; fails when called until the
    /// Function constructor exists (clj-quickjs-111).
    Function,
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
            Builtin::FunctionPrototype => "",
            Builtin::FunctionPrototypeCall => "call",
            Builtin::FunctionPrototypeApply | Builtin::ReflectApply => "apply",
            Builtin::Function => "Function",
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
            Builtin::FunctionPrototype => 0,
            Builtin::FunctionPrototypeCall | Builtin::Function => 1,
            Builtin::FunctionPrototypeApply => 2,
            Builtin::ReflectApply => 3,
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
        Builtin::FunctionPrototype => Ok(Value::Undefined),
        Builtin::Function => Err(Fault::Unsupported(
            "the Function constructor is not supported".to_string(),
        )),
        // Every call site resolves these with `forward` first.
        Builtin::FunctionPrototypeCall
        | Builtin::FunctionPrototypeApply
        | Builtin::ReflectApply => {
            Err(Fault::Internal("a forwarding built-in was called directly"))
        }
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

/// The call a forwarding built-in (`Function.prototype.call`,
/// `Function.prototype.apply`, `Reflect.apply`) makes in its place (module
/// docs, "Forwarding built-ins").
#[derive(Debug)]
pub(crate) struct Forward {
    pub(crate) callee: Value,
    pub(crate) this: Value,
    pub(crate) args: Vec<Value>,
}

/// If `which` forwards, the call it makes when called with receiver `this`
/// and arguments `args`: `None` for every other built-in. The target is
/// checked to be callable before an argument list is read (ECMAScript
/// order). At most `max_args` arguments are accepted from an array-like;
/// a longer one fails with `StackOverflow` (QuickJS: "too many
/// arguments", a RangeError) before anything is read past its `length`.
pub(crate) fn forward(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    this: &Value,
    args: &[Value],
    max_args: usize,
) -> Result<Option<Forward>, Fault> {
    let arg = |i: usize| args.get(i).cloned().unwrap_or(Value::Undefined);
    let (callee, this, args) = match which {
        // `call(thisArg, ...args)`.
        Builtin::FunctionPrototypeCall => {
            callable(heap, this)?;
            (this.clone(), arg(0), args.get(1..).unwrap_or(&[]).to_vec())
        }
        // `apply(thisArg, argArray)`: no arguments for `undefined`/`null`.
        Builtin::FunctionPrototypeApply => {
            callable(heap, this)?;
            let list = match arg(1) {
                Value::Undefined | Value::Null => Vec::new(),
                array => list_from_array_like(heap, task, &array, max_args)?,
            };
            (this.clone(), arg(0), list)
        }
        // `Reflect.apply(target, thisArgument, argumentsList)`.
        Builtin::ReflectApply => {
            let target = arg(0);
            callable(heap, &target)?;
            let list = list_from_array_like(heap, task, &arg(2), max_args)?;
            (target, arg(1), list)
        }
        _ => return Ok(None),
    };
    Ok(Some(Forward { callee, this, args }))
}

/// IsCallable, failing with the `… is not a function` TypeError.
fn callable(heap: &Heap, v: &Value) -> Result<(), Fault> {
    crate::vm::dispatch::callee(heap, v).map(drop)
}

/// CreateListFromArrayLike (ECMAScript §7.3.18; QuickJS `build_arg_list`):
/// `TypeError` for a non-object; otherwise ToLength of its `length`, then
/// its elements `0 .. length` read with `[[Get]]` (a hole reads through the
/// prototype chain). ToLength of an object needs ToPrimitive and fails
/// with `UnsupportedOperand`.
fn list_from_array_like(
    heap: &Heap,
    task: TaskId,
    array: &Value,
    max_args: usize,
) -> Result<Vec<Value>, Fault> {
    if !matches!(array, Value::Object(_)) {
        return Err(Fault::TypeError(format!(
            "an argument list must be an object, got {}",
            array.type_name()
        )));
    }
    let length = property::get_field(heap, task, array, &JsString::from("length"))?;
    let length = crate::vm::dispatch::to_number(&length)?;
    // ToLength: NaN and negatives are 0, the rest truncated (and at most
    // 2^53 - 1, far above any accepted count).
    let length = if length.is_nan() || length <= 0.0 {
        0.0
    } else {
        length.trunc()
    };
    if length > max_args as f64 {
        return Err(Fault::StackOverflow(format!(
            "too many arguments: an argument list of length {length} exceeds the limit of {max_args}"
        )));
    }
    // `length <= max_args`, a `usize`, and an integer: the cast is exact.
    let length = length as usize;
    let mut list = Vec::with_capacity(length);
    for index in 0..length {
        let key = match u32::try_from(index) {
            Ok(index) if index != u32::MAX => property::Key::Index(index),
            _ => property::Key::Name(JsString::from(index.to_string().as_str())),
        };
        list.push(property::get(heap, task, array, &key)?);
    }
    Ok(list)
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
