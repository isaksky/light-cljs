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
//! | `Array.prototype.push(...items)` (clj-quickjs-155) | ToObject(receiver), ToLength(`length`), Set each index and then `length` with Throw=true, return the new length; over `2^53 - 1`: catchable `TypeError`; null/undefined: catchable `TypeError`; primitive wrappers and object-valued lengths: `UnsupportedOperand` |
//! | `Object.defineProperty(o, k, attributes)` | define or update the own data property `k` of `o` from the descriptor and return `o` (`crate::vm::descriptor`, clj-quickjs-119); a non-object `o` or descriptor, a descriptor with both `get`/`set` and `value`/`writable`, a define a non-configurable property forbids, a built-in object as `o`: catchable `TypeError`; an accessor descriptor: `UnsupportedOperand` (clj-quickjs-120) |
//! | `Object.getOwnPropertyDescriptor(o, k)` | `undefined` if `o` has no own property `k`, else a fresh object `{value, writable, enumerable, configurable}` (clj-quickjs-119); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: its wrapper's own properties without allocating a wrapper (string indices and `length`, clj-quickjs-142; none for numbers or booleans) |
//! | `Object.getPrototypeOf(v)` (clj-quickjs-121) | the `[[Prototype]]` of `v` (`crate::vm::prototype`), `null` at the end of a chain; `Function.prototype` for a function or built-in function (clj-quickjs-102); `undefined`/`null` `v`: catchable `TypeError`; a primitive or an error object: `UnsupportedOperand` (wrapper objects and `Error.prototype` do not exist yet) |
//! | `Object.setPrototypeOf(o, proto)` (clj-quickjs-121) | set the `[[Prototype]]` of the object `o` and return `o`; a primitive `o` is returned unchanged; `undefined`/`null` `o`, a `proto` that is neither an object nor `null`, a cycle, a built-in or non-extensible `o`: catchable `TypeError` |
//! | `Object.create(proto)` (clj-quickjs-121) | a new ordinary object owned by the running task with prototype `proto` (an object or `null`, else a catchable `TypeError`); a second argument other than `undefined`: `UnsupportedOperand` (`Object.defineProperties`) |
//! | `Object.preventExtensions(v)`, `Object.freeze(v)` (clj-quickjs-122) | make `v` non-extensible / frozen and return it (`crate::vm::integrity` has the table per kind); a primitive is returned unchanged; a built-in object: catchable `TypeError` (immutable); another task's object: `ForeignObject` |
//! | `Object.isExtensible(v)`, `Object.isFrozen(v)` (clj-quickjs-122) | the `[[Extensible]]` flag / TestIntegrityLevel(frozen); a primitive is not extensible and is frozen; vectors, maps and published functions are frozen (`crate::vm::integrity`) |
//! | `Object.keys(o)`, `Object.getOwnPropertyNames(o)` (clj-quickjs-123) | a fresh array of the own enumerable (`keys`) or all (`getOwnPropertyNames`) string keys of `o`, array indices ascending first, then the other names in creation order (`crate::vm::keys`); `undefined`/`null` `o`: catchable `TypeError`; a primitive `o`: its wrapper's own properties without allocating a wrapper (string indices and `length`, clj-quickjs-142; none for numbers or booleans); `getOwnPropertyNames` of `Object.prototype`: `UnsupportedOperand` (clj-quickjs-261) |
//! | `Number.prototype.toString(radix)`, `.valueOf()` (clj-quickjs-73) | thisNumberValue: a number receiver (a method call on a number primitive passes the number itself, no wrapper is allocated) or a Number wrapper (including `Number.prototype`, holding `+0`); `toString` gives its Number::toString (`crate::value::string::number_to_string`), with `radix` absent, `undefined` or 10; another radix: `UnsupportedOperand` (clj-quickjs-39); any other receiver: catchable `TypeError` |
//! | `String.prototype.toString()`, `.valueOf()` (clj-quickjs-73) | thisStringValue: a string receiver or a String wrapper (including `String.prototype`, holding `""`); any other receiver: catchable `TypeError` |
//! | `String.prototype.charAt`, `.charCodeAt`, `.codePointAt` (clj-quickjs-143) | RequireObjectCoercible, ToString, ToIntegerOrInfinity; UTF-16 unit access (a surrogate pair for `codePointAt`); out of bounds: empty string / NaN / undefined; object coercion uses a resumable native frame |
//! | `String.fromCharCode`, `.fromCodePoint` (clj-quickjs-143) | ToNumber then ToUint16 / integral code point validation; invalid point: catchable `RangeError`; object coercion uses a resumable native frame |
//! | `Number(...)`, `String(...)`, `new Number(...)`, `new String(...)` (clj-quickjs-72) | convert the argument (absent: 0 / ""); `new` allocates a task-owned ordinary object with [[NumberData]] / [[StringData]]; object arguments use resumable ToPrimitive with number / string hint |
//! | `Function.prototype(...)` (clj-quickjs-102) | `undefined`, whatever the receiver and arguments |
//! | `Function(...)` (clj-quickjs-102; `Function.prototype.constructor`) | `UnsupportedOperand` until the Function constructor exists (clj-quickjs-111) |
//! | `f.call(t, ...args)` (clj-quickjs-102) | forwards (above): calls the receiver `f` with `this` `t` and `args`; a receiver that is not callable: catchable `TypeError` |
//! | `f.apply(t, list)` (clj-quickjs-102) | forwards: calls `f` with `this` `t` and no arguments for an `undefined`/`null` `list`, else CreateListFromArrayLike(`list`): `length` through ToLength, then `list[0 .. length]`; a non-callable `f` or a primitive `list`: catchable `TypeError`; an object `length`: `UnsupportedOperand` (ToPrimitive); a list longer than the value-stack room left: `StackOverflow` (QuickJS: a RangeError) |
//! | `Reflect.apply(f, t, list)` (clj-quickjs-102) | forwards: `f.apply(t, list)` without reading `f.apply`, except that `list` must be an object (`undefined` is a catchable `TypeError`) |

use crate::atom::same_value;
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::ids::TaskId;
use crate::object::{OrdinaryObject, PrimitiveData};
use crate::realm;
use crate::value::string::number_to_string;
use crate::value::{JsString, Value};
use crate::vm::descriptor;
use crate::vm::dispatch::{Fault, to_js_string, to_number};
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
    /// `Number` (clj-quickjs-72): ToNumber, or a wrapper when constructed.
    Number,
    /// `Number.prototype.toString` (QuickJS `js_number_toString`;
    /// clj-quickjs-73), radix 10 only.
    NumberPrototypeToString,
    /// `Number.prototype.valueOf` (QuickJS `js_number_valueOf`;
    /// clj-quickjs-73).
    NumberPrototypeValueOf,
    /// `String` (clj-quickjs-72): ToString, or a wrapper when constructed.
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
    /// `Function.prototype.bind` (clj-quickjs-103).
    FunctionPrototypeBind,
    /// `Array.prototype.push` (QuickJS `js_array_push`, clj-quickjs-155).
    ArrayPrototypePush,
    /// UTF-16 access and construction (clj-quickjs-143).
    StringPrototypeCharAt,
    StringPrototypeCharCodeAt,
    StringPrototypeCodePointAt,
    StringFromCharCode,
    StringFromCodePoint,
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
            Builtin::FunctionPrototypeBind => "bind",
            Builtin::ArrayPrototypePush => "push",
            Builtin::StringPrototypeCharAt => "charAt",
            Builtin::StringPrototypeCharCodeAt => "charCodeAt",
            Builtin::StringPrototypeCodePointAt => "codePointAt",
            Builtin::StringFromCharCode => "fromCharCode",
            Builtin::StringFromCodePoint => "fromCodePoint",
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
            Builtin::FunctionPrototypeBind => 1,
            Builtin::ArrayPrototypePush => 1,
            Builtin::StringPrototypeCharAt
            | Builtin::StringPrototypeCharCodeAt
            | Builtin::StringPrototypeCodePointAt
            | Builtin::StringFromCharCode
            | Builtin::StringFromCodePoint => 1,
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
        Builtin::ArrayPrototypePush => array_push(heap, task, this, args),
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
        Builtin::FunctionPrototypeBind => crate::vm::function::bind(heap, task, this, args),
        Builtin::Function => Err(Fault::Unsupported(
            "the Function constructor is not supported".to_string(),
        )),
        // Every call site resolves these with `forward` first.
        Builtin::FunctionPrototypeCall
        | Builtin::FunctionPrototypeApply
        | Builtin::ReflectApply => {
            Err(Fault::Internal("a forwarding built-in was called directly"))
        }
        Builtin::Number | Builtin::String => primitive_constructor(heap, task, which, args, false),
        Builtin::NumberPrototypeToString => {
            let n = this_number_value(heap, task, this, "toString")?;
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
            this_number_value(heap, task, this, "valueOf").map(Value::Number)
        }
        Builtin::StringPrototypeToString | Builtin::StringPrototypeValueOf => {
            this_string_value(heap, task, this, which.name()).map(Value::String)
        }
        Builtin::StringPrototypeCharAt
        | Builtin::StringPrototypeCharCodeAt
        | Builtin::StringPrototypeCodePointAt => string_access(which, this, &arg(0)),
        Builtin::StringFromCharCode | Builtin::StringFromCodePoint => {
            string_from_codes(which, args)
        }
    }
}

/// RequireObjectCoercible, ToString, then ToIntegerOrInfinity. These methods
/// accept any primitive receiver; thisStringValue would be too restrictive.
/// Object operands have already passed through native::Convert.
fn string_access(which: Builtin, this: &Value, pos: &Value) -> Result<Value, Fault> {
    if matches!(this, Value::Undefined | Value::Null) {
        return Err(Fault::TypeError(format!(
            "String.prototype.{} called on {}",
            which.name(),
            this.type_name()
        )));
    }
    let string = crate::vm::dispatch::to_js_string(this)?;
    let number = crate::vm::dispatch::to_number(pos)?;
    let index = if number.is_nan() { 0.0 } else { number.trunc() };
    // Check before casting: infinity and huge indices must not saturate or wrap.
    let unit = if index >= 0.0 && index < string.len() as f64 {
        string.code_units().get(index as usize).copied()
    } else {
        None
    };
    Ok(match (which, unit) {
        (Builtin::StringPrototypeCharAt, Some(unit)) => {
            Value::String(JsString::from_code_units(vec![unit]).expect("one code unit fits"))
        }
        (Builtin::StringPrototypeCharAt, None) => Value::String(JsString::from("")),
        (Builtin::StringPrototypeCharCodeAt, None) => Value::Number(f64::NAN),
        (Builtin::StringPrototypeCodePointAt, Some(high @ 0xd800..=0xdbff)) => {
            let point = match string.code_units().get(index as usize + 1).copied() {
                Some(low @ 0xdc00..=0xdfff) => {
                    0x10000 + ((u32::from(high) - 0xd800) << 10) + u32::from(low) - 0xdc00
                }
                _ => u32::from(high),
            };
            Value::Number(f64::from(point))
        }
        (_, Some(unit)) => Value::Number(f64::from(unit)),
        (_, None) => Value::Undefined,
    })
}

/// Leaf construction: O(args + total primitive-string conversion work),
/// at most two output units per argument. The stack limit bounds args but
/// does not bound aggregate string scans; resumable coercion is clj-quickjs-265.
/// No JavaScript conversion is attempted on this Rust stack.
fn string_from_codes(which: Builtin, args: &[Value]) -> Result<Value, Fault> {
    let mut units = Vec::new();
    for arg in args {
        let n = crate::vm::dispatch::to_number(arg)?;
        if which == Builtin::StringFromCharCode {
            // ToUint16: truncate, modulo 2^16, non-finite values become zero.
            let unit = if n.is_finite() {
                n.trunc() % 65536.0
            } else {
                0.0
            };
            let unit = if unit < 0.0 { unit + 65536.0 } else { unit };
            units.push(unit as u16);
        } else {
            validate_code_point(n)?;
            let point = n as u32;
            if point <= 0xffff {
                units.push(point as u16);
            } else {
                let offset = point - 0x10000;
                units.push(0xd800 | (offset >> 10) as u16);
                units.push(0xdc00 | (offset & 0x3ff) as u16);
            }
        }
        if units.len() > crate::value::string::MAX_STRING_LEN {
            return Err(Fault::OutOfMemory("string too long"));
        }
    }
    JsString::from_code_units(units)
        .map(Value::String)
        .ok_or(Fault::OutOfMemory("string too long"))
}

fn validate_code_point(n: f64) -> Result<(), Fault> {
    if !n.is_finite() || !(0.0..=0x10ffff as f64).contains(&n) || n.trunc() != n {
        return Err(Fault::RangeError("invalid code point".to_string()));
    }
    Ok(())
}

/// Finish each primitive conversion before native::Convert visits the next
/// operand. In particular, invalid code points must throw before later
/// arguments' valueOf/toString callbacks can run.
pub(crate) fn string_builtin_operand(
    which: Builtin,
    index: usize,
    value: &Value,
) -> Result<Value, Fault> {
    match which {
        Builtin::StringPrototypeCharAt
        | Builtin::StringPrototypeCharCodeAt
        | Builtin::StringPrototypeCodePointAt
            if index == 0 =>
        {
            crate::vm::dispatch::to_js_string(value).map(Value::String)
        }
        _ => {
            let n = crate::vm::dispatch::to_number(value)?;
            if which == Builtin::StringFromCodePoint {
                validate_code_point(n)?;
            }
            Ok(Value::Number(n))
        }
    }
}

/// thisNumberValue (ECMAScript §21.1.3; QuickJS `js_thisNumberValue`): a
/// number receiver is itself; a Number wrapper yields its [[NumberData]],
/// including `Number.prototype`'s +0. Other receivers throw a TypeError.
fn this_number_value(heap: &Heap, task: TaskId, this: &Value, method: &str) -> Result<f64, Fault> {
    if let Value::Number(n) = this {
        return Ok(*n);
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::Number(n)) => Ok(n),
        _ => Err(Fault::TypeError(format!(
            "Number.prototype.{method} requires that 'this' be a Number, not {}",
            describe_receiver(this)
        ))),
    }
}

/// thisStringValue (ECMAScript §22.1.3; QuickJS `js_thisStringValue`): a
/// string receiver is itself; a String wrapper yields its [[StringData]],
/// including `String.prototype`'s "". Other receivers throw a TypeError.
fn this_string_value(
    heap: &Heap,
    task: TaskId,
    this: &Value,
    method: &str,
) -> Result<JsString, Fault> {
    if let Value::String(s) = this {
        return Ok(s.clone());
    }
    match wrapper_data(heap, task, this)? {
        Some(PrimitiveData::String(s)) => Ok(s),
        _ => Err(Fault::TypeError(format!(
            "String.prototype.{method} requires that 'this' be a String, not {}",
            describe_receiver(this)
        ))),
    }
}

/// Inspect the receiver's own internal slots, never its prototype's slots.
/// Owned wrappers obey the same read boundary as their ordinary properties.
fn wrapper_data(heap: &Heap, task: TaskId, this: &Value) -> Result<Option<PrimitiveData>, Fault> {
    let Value::Object(id) = this else {
        return Ok(None);
    };
    heap.read(*id, |object| match object {
        HeapObject::Plain(o) => {
            if let Some(owner) = o.owner()
                && owner != task
            {
                return Err(Fault::ForeignObject {
                    owner: Some(owner),
                    caller: task,
                });
            }
            Ok(o.primitive_data().cloned())
        }
        _ => Ok(None),
    })
    .unwrap_or(Err(Fault::Internal("wrapper receiver was reclaimed")))
}

/// Finish a Number/String call or construction after resumable ToPrimitive
/// (when needed) has produced a primitive. Arguments remain traced on the
/// calling frame's operand stack or in its native continuation. The result
/// replaces them before any safepoint, so allocation needs no temporary root.
pub(crate) fn primitive_constructor(
    heap: &Heap,
    task: TaskId,
    which: Builtin,
    args: &[Value],
    construct: bool,
) -> Result<Value, Fault> {
    let value = match which {
        Builtin::Number => Value::Number(match args.first() {
            Some(value) => to_number(value)?,
            None => 0.0,
        }),
        Builtin::String => Value::String(match args.first() {
            Some(value) => to_js_string(value)?,
            None => JsString::from(""),
        }),
        _ => return Err(Fault::Internal("not a primitive constructor")),
    };
    if !construct {
        return Ok(value);
    }
    let exhausted = || Fault::OutOfMemory("heap object table exhausted");
    let realm = realm::intrinsics(heap).ok_or_else(exhausted)?;
    let (prototype, data) = match value {
        Value::Number(n) => (realm.number_prototype, PrimitiveData::Number(n)),
        Value::String(s) => (realm.string_prototype, PrimitiveData::String(s)),
        _ => return Err(Fault::Internal("not a wrapper primitive")),
    };
    let object = OrdinaryObject::with_prototype(task, prototype).with_primitive_data(data);
    heap.alloc(HeapObject::Plain(object))
        .map(Value::Object)
        .ok_or_else(exhausted)
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

/// `Array.prototype.push`: Set with Throw=true, including the final length
/// even for zero items. Earlier successful writes survive a later failure.
/// Indexed Set uses the array's dense Vec append path without scanning its
/// elements; absent indices still consult the prototype chain. The loop is
/// bounded by the task's operand-stack argument limit, not the array length.
/// Object-valued lengths require resumable ToPrimitive and fail explicitly.
fn array_push(heap: &Heap, task: TaskId, this: &Value, args: &[Value]) -> Result<Value, Fault> {
    match this {
        Value::Undefined | Value::Null => {
            return Err(Fault::TypeError(format!(
                "cannot convert {} to object",
                this.type_name()
            )));
        }
        Value::Object(_) => {}
        _ => {
            return Err(Fault::Unsupported(
                "Array.prototype.push on a primitive needs a wrapper object, which is not supported"
                    .to_string(),
            ));
        }
    }
    const MAX_LENGTH: u64 = (1 << 53) - 1;
    let length_key = JsString::from("length");
    let length = property::get_field(heap, task, this, &length_key)?;
    let length = crate::vm::dispatch::to_number(&length)?;
    // ToLength: clamp ToIntegerOrInfinity to [0, 2^53 - 1].
    let length = if length.is_nan() || length <= 0.0 {
        0
    } else {
        length.trunc().min(MAX_LENGTH as f64) as u64
    };
    if args.len() as u64 > MAX_LENGTH - length {
        return Err(Fault::TypeError(
            "array-like length exceeds 2^53 - 1".to_string(),
        ));
    }
    for (offset, item) in args.iter().enumerate() {
        let index = length + offset as u64;
        let key = if index < u64::from(u32::MAX) {
            property::Key::Index(index as u32)
        } else {
            property::Key::Name(JsString::from(index.to_string().as_str()))
        };
        property::put(heap, task, this, &key, item.clone(), true)?;
    }
    let new_length = Value::Number((length + args.len() as u64) as f64);
    property::put_field(heap, task, this, &length_key, new_length.clone(), true)?;
    Ok(new_length)
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
                HeapObject::Function(_) | HeapObject::BoundFunction(_) | HeapObject::Builtin(_) => {
                    Ok("Function")
                }
                HeapObject::Array(_) => Ok("Array"),
                HeapObject::Native(payload) => {
                    let any: &dyn std::any::Any = &**payload;
                    if any.is::<ErrorObject>() {
                        unsupported("an error object")
                    } else {
                        Ok("Object")
                    }
                }
                HeapObject::Plain(o) => Ok(match o.primitive_data() {
                    Some(PrimitiveData::Number(_)) => "Number",
                    Some(PrimitiveData::String(_)) => "String",
                    None => "Object",
                }),
                HeapObject::Vector(_)
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

    #[test]
    fn wrapper_internal_data_checks_the_owner() {
        let heap = Heap::default();
        let owner = TaskId::from_raw(1);
        let caller = TaskId::from_raw(2);
        for (constructor, method, argument) in [
            (
                Builtin::Number,
                Builtin::NumberPrototypeValueOf,
                Value::Number(7.0),
            ),
            (
                Builtin::String,
                Builtin::StringPrototypeToString,
                Value::String(JsString::from("abc")),
            ),
        ] {
            let wrapper = primitive_constructor(&heap, owner, constructor, &[argument], true)
                .unwrap_or_else(|_| panic!("construction failed"));
            assert!(matches!(
                call(&heap, caller, method, &wrapper, &[]),
                Err(Fault::ForeignObject { owner: Some(id), caller: task }) if id == owner && task == caller
            ));
            assert!(call(&heap, owner, method, &wrapper, &[]).is_ok());
        }
    }
}
