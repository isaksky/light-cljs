//! The interpreter loop.
//!
//! [`execute`] runs a task's [`VmState`] over shared, validated [`Code`]
//! until the task returns, fails, or exhausts its execution budget. All state
//! it mutates is the task-owned `VmState`; it keeps no Rust references into
//! the value stack or the frame stack across instructions, and nothing is
//! held on the Rust stack that a later run would need (`docs/design.md`
//! §§3, 10). The only thing it keeps between instructions is an `Arc` of the
//! running frame's immutable code, refreshed whenever `Call` or `Return`
//! changes the frame. The instruction position is advanced before each
//! instruction executes, so the running frame's `pc` always names the next
//! instruction to run, and a caller's saved `pc` names the instruction after
//! its `Call`.
//!
//! **Calls.** `Call` and `Return` push and pop task-owned frames
//! (`crate::vm::frames`); the loop continues in the new frame instead of
//! recursing, so call depth never consumes Rust stack.
//! `CallMethod` is `Call` with the receiver below the callee. A built-in
//! function callee (`crate::vm::builtin`, clj-quickjs-117) pushes no frame:
//! it runs to completion inside the instruction and its result replaces
//! the operands. A forwarding built-in (`Function.prototype.call`, `apply`,
//! `Reflect.apply`, clj-quickjs-102) is a tail call instead ([`resolve`]):
//! the call it makes replaces its operands in the same instruction and
//! enters the target's frame (or runs a target built-in), so it too
//! consumes no Rust stack and leaves nothing behind across a wait.
//!
//! The loop never panics on its inputs: validation rules out bad indices and
//! stack underflow, and the loop still re-checks them, failing the task with
//! [`VmErrorKind::Internal`] if an invariant is ever broken.
//!
//! **Budget and yield.** The budget counts instructions (a `Call` or
//! `Return` is one). It is checked only *between* instructions: when it is
//! exhausted the loop returns [`Exit::Yield`] with the running frame's `pc`
//! naming the next instruction (possibly a branch target just jumped to, or
//! the first instruction of a function just called) and every frame's
//! operands and locals as their next instructions expect them. Every
//! instruction, including `HostEffect`, runs to completion within one run, so
//! a later run continues exactly where this one stopped and never replays an
//! executed instruction or its effect.
//!
//! **Safepoints.** At run entry and before each instruction [`polls`] selects
//! (backward branches, `Call`, `Return`, and every instruction that may
//! allocate) the loop calls `Heap::safepoint` with the task's `VmState`: if
//! a collection is requested on another thread, the run reports its frames
//! and parks until the collection has run (`crate::heap::gc`). Polls happen
//! only between instructions, so a value an instruction holds in a Rust
//! local (an allocation not yet pushed, an atom value not yet rooted) never
//! meets a collection, and a straight-line stretch without poll points is
//! bounded by the code length.
//!
//! **Exceptions.** A `Throw`, an `EndFinally` re-raising a saved exception,
//! or a failing instruction whose error is a catchable JavaScript error
//! raises an exception: [`VmState::raise`] unwinds the task-owned frames and
//! handler stack to the innermost handler and the loop continues there, in
//! the same run and budget. Raising never recurses and never keeps a Rust
//! frame alive. A caught exception is a poll point (after unwinding, when
//! every value is in task-owned state). Other failures, and exceptions no
//! handler catches, end the run with [`Exit::Fail`] (`crate::vm::exception`).
//! While a cancelled task runs its finally blocks, the loop leaves with
//! [`Exit::CleanupEnded`] when the last one finishes or when an exception or
//! `return` would override the cancellation (`crate::task`, "Cancellation
//! and finally blocks").
//!
//! **Dependency waits.** `Need` decodes its operands and leaves the loop with
//! [`Exit::Need`] (the instruction position already past it, the remaining
//! budget carried along): the dependency protocol, suspension and the
//! injection of the result belong to the run guard (`crate::task`, "Dependency
//! waits in the VM"), which either pushes the value and calls [`execute`]
//! again or ends the run. No Rust frame of this loop survives a wait.
//!
//! **Resumable natives.** `Reduce` and `NeedAll` push a task-owned native
//! frame (`crate::vm::native`) instead of running to completion. While one is
//! running (no call frame above it) the loop takes its next *step* in place
//! of an instruction: a trampoline. A step that calls a callback enters a VM
//! frame like `Call`, whose `Return` hands the result back to the native; a
//! step that demands a dependency leaves with [`Exit::Need`] exactly like a
//! `Need` instruction. Each step counts against the budget and is a poll
//! point, so a yield, a suspension or a collection may fall between any two
//! steps, and the next run continues with the pending step.

use std::sync::Arc;

use crate::collections::{BuilderError, CollectionError, object_name};
use crate::error::{VmError, VmErrorKind};
use crate::heap::ObjectId;
use crate::heap::{Heap, HeapObject};
use crate::host::{Host, HostCall, call_leaf};
use crate::ids::{DependencyId, ResultKind, TaskId};
use crate::object::{OrdinaryObject, PropertyFlags};
use crate::realm::{self, Global};
use crate::value::string::{number_to_string, string_to_number};
use crate::value::{JsString, Value};
use crate::vm::builtin::{self, Builtin};
use crate::vm::exception::{
    Completion, ErrorObject, Exception, HandlerKind, describe_thrown, error_object,
};
use crate::vm::executable::Code;
use crate::vm::frames::{Raised, Returned, VmState};
use crate::vm::function::{BindingCell, FunctionObject};
use crate::vm::instr::{Instr, Opcode, ThrowKind};
use crate::vm::intrinsics;
use crate::vm::keys;
use crate::vm::native;
use crate::vm::property;
use crate::vm::prototype;

/// How a run of the interpreter loop ended.
#[derive(Debug)]
pub(crate) enum Exit {
    Return(Value),
    Fail(VmError),
    /// The budget ran out; the task can be run again.
    Yield,
    /// A `Need` instruction at `pc` asks for result `kind` of `dependency`;
    /// its operands are popped and the running frame's `pc` is past it. Or a
    /// running native frame started by the instruction at `pc` demands it.
    /// The caller pushes the result and continues with `budget`, or suspends.
    Need {
        dependency: DependencyId,
        kind: ResultKind,
        pc: usize,
        budget: Option<u64>,
    },
    /// Cleanup for the task's consumer cancellation is over: its last
    /// finally block finished, or an exception or `return` would have
    /// overridden the cancellation (`crate::task`, "Cancellation and finally
    /// blocks").
    CleanupEnded,
}

/// A failure before diagnostic context is attached.
pub(crate) enum Fault {
    Unsupported(String),
    Internal(&'static str),
    /// The host reported an error from a leaf effect.
    Host(String),
    /// The heap could not allocate.
    OutOfMemory(&'static str),
    /// A collection intrinsic rejected its operands.
    Collection(CollectionError),
    /// A builder operation was rejected by the builder lifecycle.
    Builder(BuilderError),
    /// A `ThrowError` instruction ran.
    Throw(ThrowKind),
    /// A `Call` callee that is not a function object; names what it was.
    NotCallable(&'static str),
    /// A `Call` would exceed the task's `VmLimits`.
    StackOverflow(String),
    /// A binding-cell instruction on another task's cell.
    ForeignBinding {
        owner: TaskId,
        caller: TaskId,
    },
    /// An atom intrinsic rejected its operands; the message names why.
    Atom(crate::AtomErrorKind, String),
    /// A `Throw` instruction raised this value.
    Thrown(Value),
    /// An `EndFinally` re-raises the exception its finally block saved.
    Rethrow(Exception),
    /// A JavaScript TypeError with this message (property access,
    /// `crate::vm::property`).
    TypeError(String),
    /// A property instruction on an ordinary object the running task does
    /// not own.
    ForeignObject {
        owner: Option<TaskId>,
        caller: TaskId,
    },
    /// An invalid array length (clj-quickjs-154).
    RangeError(String),
    SyntaxError(String),
}

/// What the loop does after an instruction (or a native step).
pub(crate) enum Flow {
    Next,
    /// `Call` or `Return` changed the running frame.
    FrameChanged,
    /// The entry frame returned: the task completes with this value.
    Complete(Value),
    /// A `Need` popped its operands: the run guard reads the dependency.
    Need(DependencyId, ResultKind),
    /// The cancellation cleanup is over ([`Exit::CleanupEnded`]).
    CleanupEnded,
}

/// Allocate a function object for `code` with its captured values, created
/// by `owner`. Never known shareable at allocation, even when every captured
/// value is: until a publication check proves it shareable (which makes it
/// immutable), its creator may still give it own properties
/// (`crate::vm::function`, "Own properties").
fn new_function(
    heap: &Heap,
    code: Arc<Code>,
    captures: Box<[Value]>,
    owner: TaskId,
    home_object: Option<ObjectId>,
) -> Result<Value, Fault> {
    let mut function = FunctionObject::new(code, captures, owner);
    function.home_object = home_object;
    let object = HeapObject::Function(function);
    heap.alloc(object)
        .map(Value::Object)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))
}

fn numbers(op: Opcode, a: &Value, b: &Value) -> Result<(f64, f64), Fault> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok((*x, *y)),
        _ => Err(Fault::Unsupported(format!(
            "{} supports number operands only, got {} and {}",
            op.name(),
            a.type_name(),
            b.type_name()
        ))),
    }
}

/// JavaScript ToInt32 of a Number: NaN and ±Infinity are 0; otherwise the
/// value truncated toward zero, modulo 2^32, as a two's-complement 32-bit
/// integer. ToUint32 is the same bits read as `u32`.
pub(crate) fn to_int32(x: f64) -> i32 {
    if !x.is_finite() {
        return 0;
    }
    // `trunc` is an integer and `rem_euclid` of an integer by 2^32 is exact,
    // so the cast sees an integer in [0, 2^32).
    x.trunc().rem_euclid(4_294_967_296.0) as u32 as i32
}

/// JavaScript Number::exponentiate (`x ** y` and `Math.pow`). IEEE 754 `pow` (which
/// `f64::powf` is) agrees with it except where `pow` returns 1 although
/// JavaScript requires NaN: a non-finite exponent (NaN or ±Infinity) with
/// base `1` or `-1`. Every other NaN exponent already gives NaN from `pow`,
/// and `NaN ** ±0` is 1 in both. This is QuickJS's `js_pow`.
pub(super) fn exponentiate(x: f64, y: f64) -> f64 {
    if !y.is_finite() && x.abs() == 1.0 {
        f64::NAN
    } else {
        x.powf(y)
    }
}

/// JavaScript ToNumber for primitives: NaN for `undefined`, 0 for `null`, 0
/// or 1 for booleans, and StringToNumber ([`string_to_number`]) for a
/// string. VM instructions first convert objects through native::Convert;
/// leaf built-ins must reject object arguments until they become resumable.
pub(crate) fn to_number(v: &Value) -> Result<f64, Fault> {
    match v {
        Value::Undefined => Ok(f64::NAN),
        Value::Null => Ok(0.0),
        Value::Bool(b) => Ok(if *b { 1.0 } else { 0.0 }),
        Value::Number(x) => Ok(*x),
        Value::String(s) => Ok(string_to_number(s)),
        Value::Symbol(_) => Err(Fault::TypeError("cannot convert symbol to number".into())),
        Value::Object(_) => Err(Fault::Unsupported(
            "converting an object to a number (ToPrimitive) is not supported".to_string(),
        )),
    }
}

/// JavaScript ToString for primitives (QuickJS `JS_ToStringInternal`):
/// `"undefined"`, `"null"`, `"true"` / `"false"`, Number::toString in radix
/// 10 ([`number_to_string`]), and a string itself (the same body). Objects
/// are converted with hint string by native::Convert before this helper.
pub(crate) fn to_js_string(v: &Value) -> Result<JsString, Fault> {
    Ok(match v {
        Value::String(s) => s.clone(),
        Value::Symbol(_) => return Err(Fault::TypeError("cannot convert symbol to string".into())),
        Value::Undefined => JsString::from("undefined"),
        Value::Null => JsString::from("null"),
        Value::Bool(b) => JsString::from(if *b { "true" } else { "false" }),
        Value::Number(x) => JsString::from(number_to_string(*x).as_str()),
        Value::Object(_) => {
            return Err(Fault::Internal("object reached primitive ToString"));
        }
    })
}

/// JavaScript binary `+` (ApplyStringOrNumericBinaryOperator for `+`, QuickJS
/// `js_add_slow`) on primitives: when either operand is a string, the
/// ToString of both concatenate; otherwise the ToNumber of both add.
/// native::Convert handles objects with hint default before this helper.
fn plus(a: &Value, b: &Value) -> Result<Value, Fault> {
    match (a, b) {
        (Value::Number(x), Value::Number(y)) => Ok(Value::Number(x + y)),
        (Value::Object(_), _) | (_, Value::Object(_)) => {
            Err(Fault::Internal("object reached primitive plus"))
        }
        (Value::String(_), _) | (_, Value::String(_)) => to_js_string(a)?
            .concat(&to_js_string(b)?)
            .map(Value::String)
            .ok_or(Fault::OutOfMemory("string too long")),
        _ => Ok(Value::Number(to_number(a)? + to_number(b)?)),
    }
}

/// ECMAScript IsLessThan(x, y) on primitives (QuickJS `js_relational_slow`):
/// `None` stands for its `undefined` result, a NaN operand. Two strings
/// compare lexicographically by UTF-16 code units (QuickJS
/// `js_string_compare`), a proper prefix being smaller; any other pair
/// compares the ToNumber of both. native::Convert has already converted
/// objects in source order with hint number, before operands are swapped.
fn is_less_than(x: &Value, y: &Value) -> Result<Option<bool>, Fault> {
    Ok(match (x, y) {
        (Value::Object(_), _) | (_, Value::Object(_)) => {
            return Err(Fault::Internal("object reached primitive comparison"));
        }
        (Value::String(s), Value::String(t)) => Some(s < t),
        _ => {
            let (n, m) = (to_number(x)?, to_number(y)?);
            n.partial_cmp(&m).map(|order| order.is_lt())
        }
    })
}

/// JavaScript `typeof v` (ECMAScript TypeofOperator, QuickJS
/// `js_operator_typeof`): a function object or a built-in function, the
/// callable kinds, is `"function"`; every other heap object is `"object"`
/// (no other kind is callable, and none emulates `undefined`). It inspects the object only
/// under the heap read lock and calls no JavaScript.
fn type_of(heap: &Heap, v: &Value) -> Result<&'static str, Fault> {
    Ok(match v {
        Value::Undefined => "undefined",
        Value::Null => "object",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Symbol(_) => "symbol",
        Value::Object(id) => heap
            .read(*id, |object| match object {
                HeapObject::Function(_) | HeapObject::BoundFunction(_) | HeapObject::Builtin(_) => {
                    "function"
                }
                HeapObject::Plain(_)
                | HeapObject::Native(_)
                | HeapObject::Vector(_)
                | HeapObject::Map(_)
                | HeapObject::Occurrence(_)
                | HeapObject::Builder(_)
                | HeapObject::Cell(_)
                | HeapObject::Atom(_)
                | HeapObject::Array(_) => "object",
            })
            // A frame keeps its operands alive; a stale id is reported.
            .ok_or(Fault::Internal("typeof operand was reclaimed"))?,
    })
}

/// JavaScript `==` (IsLooselyEqual, QuickJS `js_eq_slow`) for the supported
/// value types. `null` and `undefined` equal each other and nothing else;
/// two values of one type compare strictly (two strings by content). A
/// string compared with a number, and a boolean compared with a number or
/// string, compare as numbers: the boolean converts to 0 or 1 and the string
/// by StringToNumber, so `"1" == true` and `" 1 " == 1` hold, and a string
/// that is not numeric equals no number (its value is NaN). An object
/// compared with a non-nullish primitive has already passed through
/// native::Convert with hint default.
fn loose_equals(a: &Value, b: &Value) -> Result<bool, Fault> {
    use Value::*;
    Ok(match (a, b) {
        (Undefined | Null, Undefined | Null) => true,
        (Undefined | Null, _) | (_, Undefined | Null) => false,
        (Object(_), Object(_))
        | (Number(_), Number(_))
        | (Bool(_), Bool(_))
        | (String(_), String(_))
        | (Symbol(_), Symbol(_)) => a.strict_equals(b),
        (Number(_) | Bool(_) | String(_), Number(_) | Bool(_) | String(_)) => {
            to_number(a)? == to_number(b)?
        }
        (Object(_), _) | (_, Object(_)) => {
            return Err(Fault::Internal("unconverted object reached loose equality"));
        }
        (Symbol(_), _) | (_, Symbol(_)) => false,
    })
}

/// Failure message of a `ThrowError`, naming the binding when the code has
/// source text.
fn throw_message(code: &Code, pc: usize, kind: ThrowKind) -> String {
    let name = code.location(pc).and_then(|at| code.source_text(at));
    match (kind, name) {
        (ThrowKind::Unresolvable, Some(n)) => format!("{n} is not defined"),
        (ThrowKind::Unresolvable, None) => "reference to an undeclared name".to_string(),
        (ThrowKind::Uninitialized, Some(n)) => {
            format!("cannot access '{n}' before initialization")
        }
        (ThrowKind::Uninitialized, None) => "binding accessed before initialization".to_string(),
        (ThrowKind::ConstAssignment, Some(n)) => {
            format!("assignment to constant variable '{n}'")
        }
        (ThrowKind::ConstAssignment, None) => "assignment to a constant variable".to_string(),
    }
}

/// The property name a `GetField`, `PutField` or `DefineField` operand
/// names: a string constant (checked at load time).
fn name_constant(code: &Code, i: u16) -> Result<JsString, Fault> {
    match code.constants.get(usize::from(i)) {
        Some(Value::String(name)) => Ok(name.clone()),
        _ => Err(Fault::Internal("property name is not a string constant")),
    }
}

/// What a callable value runs.
pub(crate) enum Callee {
    /// A function object: its code runs in a new VM frame.
    Code(ObjectId, Arc<Code>),
    /// A built-in function: Rust code run inside the call instruction
    /// (`crate::vm::builtin`).
    Builtin(Builtin),
    /// A bound target, resolved iteratively before entering a VM frame.
    Bound(builtin::Forward),
}

/// The callee of a `Call`/`CallMethod` (or of a native's callback): what it
/// runs, or the name of what was found instead.
pub(crate) fn callee(heap: &Heap, callee: &Value) -> Result<Callee, Fault> {
    let Value::Object(id) = *callee else {
        return Err(Fault::NotCallable(callee.type_name()));
    };
    heap.read(id, |object| match object {
        HeapObject::Function(f) => Ok(Callee::Code(id, Arc::clone(&f.code))),
        HeapObject::Builtin(f) => Ok(Callee::Builtin(f.which)),
        HeapObject::BoundFunction(f) => Ok(Callee::Bound(builtin::Forward {
            callee: f.target.clone(),
            this: f.this.clone(),
            args: f.args.to_vec(),
        })),
        other => Err(Fault::NotCallable(object_name(other))),
    })
    // A frame keeps its operands alive; a stale id is reported, not trusted.
    .unwrap_or(Err(Fault::Internal("callee was reclaimed")))
}

/// `Call argc` (`method`: false) or `CallMethod argc` (true): the receiver
/// (method calls only), the callee and the arguments are the top operands.
/// A function object enters a new frame whose `this` is the receiver
/// (`undefined` for a plain call, clj-quickjs-116); a built-in runs to
/// completion and its result replaces the operands.
fn call(state: &mut VmState, heap: &Heap, argc: u16, method: bool) -> Result<Flow, Fault> {
    let argc = usize::from(argc);
    let task = state.diag.task;
    let operands = top_operands(state, argc + 1 + usize::from(method))?;
    let (this, rest) = match operands.split_first() {
        Some((this, rest)) if method => (this.clone(), rest),
        _ => (Value::Undefined, operands),
    };
    let [f, args @ ..] = rest else {
        return Err(Fault::Internal("operand stack underflow"));
    };
    let popped = argc + 1 + usize::from(method);
    if let Callee::Code(id, code) = callee(heap, f)? {
        if method {
            state.remove_operand(argc + 1)?;
        }
        state.enter(code, id, this, argc)?;
        return Ok(Flow::FrameChanged);
    }
    let (max_args, max_hops) = state.forward_limits(state.stack_len() - popped);
    let forward = builtin::Forward {
        callee: f.clone(),
        this,
        args: args.to_vec(),
    };
    match resolve(heap, task, forward, max_args, max_hops)? {
        (Callee::Code(id, code), forward) => {
            let keep = state.stack_len() - popped;
            state.truncate_operands(keep)?;
            let argc = forward.args.len();
            state.push(forward.callee);
            forward.args.into_iter().for_each(|a| state.push(a));
            state.enter(code, id, forward.this, argc)?;
            Ok(Flow::FrameChanged)
        }
        (Callee::Builtin(which), forward) => {
            if super::native::start_builtin(
                heap,
                state,
                which,
                &forward.this,
                &forward.args,
                false,
                popped,
            )? {
                return Ok(Flow::Next);
            }
            let result = builtin::call(heap, task, which, &forward.this, &forward.args)?;
            state.replace(popped, result)?;
            Ok(Flow::Next)
        }
        (Callee::Bound(_), _) => Err(Fault::Internal("unresolved bound function")),
    }
}

/// The call a forwarding built-in makes (`crate::vm::builtin`, "Forwarding
/// built-ins"), followed through further forwarding built-ins
/// (`f.call.call(g, 1)`) to what finally runs: a function object, entered
/// as a VM frame by the caller, or a built-in that runs to completion.
/// Each hop is bounded by `max_hops` (a cycle through an array that holds
/// itself would otherwise never end) and fails with `StackOverflow`, as
/// the nested native frames it stands for would.
pub(crate) fn resolve(
    heap: &Heap,
    task: TaskId,
    mut forward: builtin::Forward,
    max_args: usize,
    max_hops: usize,
) -> Result<(Callee, builtin::Forward), Fault> {
    for hops in 0..=max_hops {
        match callee(heap, &forward.callee)? {
            Callee::Bound(mut bound) => {
                if hops == max_hops {
                    break;
                }
                prepend_args(&mut bound.args, forward.args, max_args)?;
                forward = bound;
            }
            Callee::Builtin(which) => {
                match builtin::forward(heap, task, which, &forward.this, &forward.args, max_args)? {
                    Some(next) => {
                        if hops == max_hops {
                            break;
                        }
                        forward = next;
                    }
                    None => return Ok((Callee::Builtin(which), forward)),
                }
            }
            code => return Ok((code, forward)),
        }
    }
    Err(Fault::StackOverflow(format!(
        "maximum call depth of {max_hops} frames exceeded by forwarding calls"
    )))
}

fn prepend_args(bound: &mut Vec<Value>, args: Vec<Value>, max_args: usize) -> Result<(), Fault> {
    if args.len() > max_args.saturating_sub(bound.len()) || bound.len() > max_args {
        return Err(Fault::StackOverflow("too many bound arguments".to_string()));
    }
    bound.extend(args);
    Ok(())
}

/// `Construct argc` (clj-quickjs-71; ECMAScript EvaluateNew steps 7-8,
/// OrdinaryCreateFromConstructor; QuickJS `JS_CallConstructorInternal` and
/// `js_get_prototype_from_ctor`, revision 7d1c13c5d8f8): the constructor and
/// the arguments are the top operands. Number/String construct wrappers
/// (clj-quickjs-72); Array constructs an array (clj-quickjs-153), and Error
/// constructs an error (clj-quickjs-228). Ordinary functions use the rule below.
/// Anything else is a TypeError. For ordinary functions, the
/// new object inherits from `f.prototype` when that is an object, else from
/// `Object.prototype`, and becomes the `this` of a construct frame entered
/// like a call; `VmState::leave` applies the return-override rule.
fn construct(state: &mut VmState, heap: &Heap, argc: u16) -> Result<Flow, Fault> {
    let argc = usize::from(argc);
    let task = state.diag.task;
    let [f, args @ ..] = top_operands(state, argc + 1)? else {
        return Err(Fault::Internal("operand stack underflow"));
    };
    let mut f = f.clone();
    let mut args = args.to_vec();
    let (max_args, max_hops) = state.forward_limits(state.stack_len() - argc - 1);
    let mut hops = 0;
    while let Ok(Callee::Bound(mut bound)) = callee(heap, &f) {
        if hops >= max_hops {
            return Err(Fault::StackOverflow(
                "maximum bound constructor depth exceeded".to_string(),
            ));
        }
        hops += 1;
        prepend_args(&mut bound.args, args, max_args)?;
        args = bound.args;
        f = bound.callee;
    }
    let (id, code) = match callee(heap, &f) {
        Ok(Callee::Code(id, code)) if code.constructor => (id, code),
        Ok(Callee::Builtin(Builtin::Array)) => {
            let result = builtin::array_constructor(heap, task, &args)?;
            state.replace(argc + 1, result)?;
            return Ok(Flow::Next);
        }
        Ok(Callee::Builtin(
            which @ (Builtin::Number | Builtin::String | Builtin::Error | Builtin::NativeError(_)),
        )) => {
            if super::native::start_builtin(
                heap,
                state,
                which,
                &Value::Undefined,
                &args,
                true,
                argc + 1,
            )? {
                return Ok(Flow::Next);
            }
            let result = builtin::primitive_constructor(heap, task, which, &args, true)?;
            state.replace(argc + 1, result)?;
            return Ok(Flow::Next);
        }
        Ok(_) => return Err(not_a_constructor(&f, "function")),
        Err(Fault::NotCallable(what)) => return Err(not_a_constructor(&f, what)),
        Err(fault) => return Err(fault),
    };
    // Reading `prototype` may create the lazy prototype object and build
    // the realm; nothing is held across it but `f`, which is an operand.
    let prototype = match property::get_field(heap, task, &f, &JsString::from("prototype"))? {
        Value::Object(prototype) => prototype,
        _ => {
            realm::intrinsics(heap)
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
                .object_prototype
        }
    };
    // The new object goes straight into the new frame's traced `this` in
    // this instruction: no collection can run in between.
    let object = heap
        .alloc(HeapObject::Plain(OrdinaryObject::with_prototype(
            task, prototype,
        )))
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    let keep = state.stack_len() - argc - 1;
    state.truncate_operands(keep)?;
    let argc = args.len();
    state.push(f);
    args.into_iter().for_each(|a| state.push(a));
    state.enter(code, id, Value::Object(object), argc)?;
    state.top_mut().construct = true;
    Ok(Flow::FrameChanged)
}

/// The TypeError of `new v` for a `v` that is not a constructor; `what`
/// names a non-number value's kind.
fn not_a_constructor(v: &Value, what: &str) -> Fault {
    let what = match v {
        Value::Number(_) => describe(v),
        _ => what.to_string(),
    };
    Fault::TypeError(format!("{what} is not a constructor"))
}

/// Lazily materialize and root the global object before returning it.
fn global_object(state: &mut VmState, heap: &Heap) -> Result<Value, Fault> {
    if let Some(id) = state.global_object {
        return Ok(Value::Object(id));
    }
    let id = realm::new_global_object(heap, state.diag.task)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    state.global_object = Some(id);
    Ok(Value::Object(id))
}

/// The receiver of an ordinary function. Sloppy non-object substitution
/// remains explicitly unsupported; script `this` uses `GetGlobalObject`.
fn this_value(state: &VmState) -> Result<Value, Fault> {
    let frame = state.top();
    match &frame.this {
        this @ Value::Object(_) => Ok(this.clone()),
        this if frame.code.strict => Ok(this.clone()),
        this => Err(Fault::Unsupported(format!(
            "`this` of sloppy-mode code called with {} as its receiver is not supported (it needs the global object or a primitive wrapper)",
            describe(this)
        ))),
    }
}

/// The top `n` operands of the running frame, oldest first.
fn top_operands(state: &VmState, n: usize) -> Result<&[Value], Fault> {
    let operands = state.operands();
    operands
        .len()
        .checked_sub(n)
        .map(|at| &operands[at..])
        .ok_or(Fault::Internal("operand stack underflow"))
}

/// Run `f` on the binding of `cell` for the running task: fails if `cell` is
/// not a binding cell, or belongs to another task.
fn with_cell<R>(
    heap: &Heap,
    cell: &Value,
    task: TaskId,
    f: impl FnOnce(&mut Option<Value>) -> Result<R, Fault>,
) -> Result<R, Fault> {
    let not_cell =
        |found: &str| Fault::Unsupported(format!("expected a binding cell, got {found}"));
    let Value::Object(id) = *cell else {
        return Err(not_cell(cell.type_name()));
    };
    heap.with_object_mut(id, |object| match object {
        HeapObject::Cell(c) if c.owner == task => f(&mut c.value),
        HeapObject::Cell(c) => Err(Fault::ForeignBinding {
            owner: c.owner,
            caller: task,
        }),
        other => Err(not_cell(object_name(other))),
    })
    // A frame keeps its operands alive; a stale id is reported, not trusted.
    .unwrap_or(Err(Fault::Internal("binding cell was reclaimed")))
}

/// The operands of `Need`: a definition id (an integer number in
/// `0..=2^53-1`, exactly representable) and a result kind (an integer in
/// `0..=u32::MAX`). Anything else is rejected, never rounded.
fn need_operands(dependency: &Value, kind: &Value) -> Result<(DependencyId, ResultKind), Fault> {
    match (dependency_id(dependency), result_kind(kind)) {
        (Some(d), Some(k)) => Ok((d, k)),
        _ => Err(Fault::Unsupported(format!(
            "need takes a definition id and a result kind as non-negative integer numbers, got {} and {}",
            describe(dependency),
            describe(kind)
        ))),
    }
}

/// `v` as an integer number in `0..=max`, never rounded.
fn integer(v: &Value, max: f64) -> Option<f64> {
    match *v {
        Value::Number(n) if n.fract() == 0.0 && (0.0..=max).contains(&n) => Some(n),
        _ => None,
    }
}

/// A definition id operand: an integer number in `0..=2^53-1`.
pub(crate) fn dependency_id(v: &Value) -> Option<DependencyId> {
    const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;
    integer(v, MAX_SAFE_INTEGER).map(|d| DependencyId::new(d as u64))
}

/// A result kind operand: an integer number in `0..=u32::MAX`.
pub(crate) fn result_kind(v: &Value) -> Option<ResultKind> {
    integer(v, f64::from(u32::MAX)).map(|k| ResultKind::new(k as u32))
}

/// A value for a diagnostic: numbers as themselves, other values by type.
pub(crate) fn describe(v: &Value) -> String {
    match v {
        Value::Number(n) => n.to_string(),
        other => other.type_name().to_string(),
    }
}

/// Return `v` from the running frame, through its finally blocks.
fn return_flow(state: &mut VmState, v: Value) -> Result<Flow, Fault> {
    Ok(match state.return_value(v)? {
        Returned::Complete(result) => Flow::Complete(result),
        Returned::Left | Returned::Finally => Flow::FrameChanged,
        Returned::Cancelled => Flow::CleanupEnded,
    })
}

/// The error kind and message of a fault that is not an exception value.
fn fault_error(fault: Fault, code: &Code, pc: usize) -> (VmErrorKind, String) {
    match fault {
        Fault::SyntaxError(m) => (VmErrorKind::SyntaxError, m),
        Fault::RangeError(m) => (VmErrorKind::RangeError, m),
        Fault::Unsupported(m) => (VmErrorKind::UnsupportedOperand, m),
        Fault::Internal(m) => (VmErrorKind::Internal, m.to_string()),
        Fault::Host(m) => (VmErrorKind::HostEffect, m),
        Fault::OutOfMemory(m) => (VmErrorKind::OutOfMemory, m.to_string()),
        Fault::Collection(e) => (VmErrorKind::Collection(e), e.to_string()),
        Fault::Builder(e) => (VmErrorKind::Builder(e), e.to_string()),
        Fault::NotCallable(what) => (VmErrorKind::TypeError, format!("{what} is not a function")),
        Fault::StackOverflow(m) => (VmErrorKind::StackOverflow, m),
        Fault::Atom(kind, m) => (VmErrorKind::Atom(kind), m),
        Fault::ForeignBinding { owner, caller } => (
            VmErrorKind::ForeignBinding { owner, caller },
            format!(
                "binding is owned by {owner}, not {caller}: a closure over mutable bindings is task-owned"
            ),
        ),
        Fault::TypeError(m) => (VmErrorKind::TypeError, m),
        Fault::ForeignObject { owner, caller } => {
            let owner_name = match owner {
                Some(task) => task.to_string(),
                None => "the embedder".to_string(),
            };
            (
                VmErrorKind::ForeignObject { owner, caller },
                format!(
                    "object is owned by {owner_name}, not {caller}: an ordinary object is task-owned"
                ),
            )
        }
        Fault::Throw(kind) => {
            let error_kind = match kind {
                ThrowKind::Unresolvable | ThrowKind::Uninitialized => VmErrorKind::ReferenceError,
                ThrowKind::ConstAssignment => VmErrorKind::TypeError,
            };
            (error_kind, throw_message(code, pc, kind))
        }
        // Exception values are raised by the caller, never described here.
        Fault::Thrown(_) | Fault::Rethrow(_) => (
            VmErrorKind::Internal,
            "an exception reached the failure path".to_string(),
        ),
    }
}

/// Run-time failures that are JavaScript errors, raised as catchable
/// exceptions (`crate::vm::exception`). Exhaustive, so a new kind must decide.
fn catchable(kind: VmErrorKind) -> bool {
    match kind {
        VmErrorKind::ReferenceError
        | VmErrorKind::SyntaxError
        | VmErrorKind::RangeError
        | VmErrorKind::TypeError
        | VmErrorKind::StackOverflow
        | VmErrorKind::DependencyFailed(_)
        | VmErrorKind::DependencyCancelled(_) => true,
        VmErrorKind::UnsupportedOperand
        | VmErrorKind::Internal
        | VmErrorKind::HostEffect
        | VmErrorKind::OutOfMemory
        | VmErrorKind::Collection(_)
        | VmErrorKind::Builder(_)
        | VmErrorKind::ForeignBinding { .. }
        | VmErrorKind::ForeignObject { .. }
        | VmErrorKind::Atom(_)
        | VmErrorKind::Thrown => false,
    }
}

/// The error an exception with value `v`, raised by `Throw` at `pc`, reports
/// if nothing catches it: an error object's own error, otherwise `Thrown`.
fn thrown_error(state: &VmState, heap: &Heap, v: &Value, pc: usize) -> VmError {
    match error_object(heap, v) {
        Some((original, _)) => original,
        None => {
            let description = super::native::error::diagnostic(heap, state.diag.task, v)
                .unwrap_or_else(|| describe_thrown(v));
            let message = format!("uncaught exception: {description}");
            state.failure(VmErrorKind::Thrown, message, pc)
        }
    }
}

/// Raise `exception` in task-owned state (`VmState::raise`): `None` when a
/// handler took it and execution continues at the handler's target;
/// otherwise how the run ends: failing with the exception's error, or
/// ending a cancellation cleanup the exception would override.
pub(crate) fn raise(state: &mut VmState, exception: Exception) -> Option<Exit> {
    match state.raise(exception) {
        Raised::Caught => None,
        Raised::Uncaught(exception) => Some(Exit::Fail(*exception.error)),
        Raised::Cancelled => Some(Exit::CleanupEnded),
        Raised::Broken(message) => {
            let at = state.top().pc;
            let error = state.failure(VmErrorKind::Internal, message.to_string(), at);
            Some(Exit::Fail(error))
        }
    }
}

/// Raise the run-time failure `error` of the running frame as a JavaScript
/// exception if it is catchable and some handler could see it: its value is
/// a fresh error object with `cause` (`crate::vm::exception`). Otherwise, or
/// if the heap is exhausted, the run fails with the error; see [`raise`].
///
/// `cause` must stay alive without this call rooting it (a published
/// dependency result is held by its cell); the caller is a registered
/// mutator with no safepoint between here and the object being stored in
/// task-owned state.
pub(crate) fn raise_failure(
    state: &mut VmState,
    heap: &Heap,
    error: VmError,
    cause: Value,
) -> Option<Exit> {
    if !catchable(error.kind) || !state.has_handlers() {
        return Some(Exit::Fail(error));
    }
    let pc = error.diagnostic.instruction;
    // The error object is stored by `raise` (pushed for a catch block or
    // kept in a pending completion) before any poll.
    let object = HeapObject::Native(Box::new(ErrorObject {
        stack: state.error_stack(pc),
        error: error.clone(),
        cause,
    }));
    let Some(id) = heap.alloc(object) else {
        let message = "heap object table exhausted".to_string();
        return Some(Exit::Fail(state.failure(
            VmErrorKind::OutOfMemory,
            message,
            pc,
        )));
    };
    raise(
        state,
        Exception {
            value: Value::Object(id),
            error: Box::new(error),
        },
    )
}

/// Build the unmapped arguments object before parameter defaults can change
/// the locals. This whole instruction holds the mutator guard: the new
/// object's edges reach the stack before any collection can run.
#[inline(never)]
fn new_arguments(state: &mut VmState, heap: &Heap) -> Result<Value, Fault> {
    let prototype = realm::intrinsics(heap)
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?
        .object_prototype;
    let args = state
        .top()
        .arguments
        .as_ref()
        .ok_or(Fault::Internal("arguments already materialized"))?;
    let mut object = OrdinaryObject::with_prototype(state.diag.task, prototype);
    let intern = |name: &str| {
        heap.keys()
            .intern(&JsString::from(name))
            .ok_or(Fault::OutOfMemory("property key table exhausted"))
    };
    object.define_with(
        intern("length")?,
        Value::Number(args.len() as f64),
        PropertyFlags::METHOD,
    );
    for (i, value) in args.iter().enumerate() {
        object.define(intern(&i.to_string())?, value.clone());
    }
    let id = heap
        .alloc(HeapObject::Plain(object))
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    state.clear_arguments();
    Ok(Value::Object(id))
}

/// Internal GetTemplateObject. The instruction's data operands are checked
/// even for hand-loaded bytecode: no task-owned edge may enter these arrays.
fn template_object(state: &mut VmState, heap: &Heap, n: u16) -> Result<(), Fault> {
    let n = usize::from(n);
    if n == 0 || state.operands().len() < 2 * n {
        return Err(Fault::Internal("invalid template object arity"));
    }
    let code = Arc::clone(&state.top().code);
    let pc = state.top().pc.saturating_sub(1);
    let key = (Arc::as_ptr(&code) as usize, pc);
    let realm = realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    // No JS or safepoint runs under this leaf metadata lock. Serializing
    // first installation gives concurrent tasks exactly one site object.
    // GC traces this lock before taking the object table lock.
    let mut templates = realm
        .templates
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, id)) = templates.get(&key) {
        let id = *id;
        drop(templates);
        return state.replace(2 * n, Value::Object(id));
    }
    let prototype = realm.array_prototype;
    let mut cooked = crate::object::array::ArrayObject::template(prototype);
    let mut raw = crate::object::array::ArrayObject::template(prototype);
    let operands = state.operands();
    for pair in operands[operands.len() - 2 * n..].chunks_exact(2) {
        if !matches!(pair[0], Value::String(_) | Value::Undefined)
            || !matches!(pair[1], Value::String(_))
        {
            return Err(Fault::Internal("template parts must be string primitives"));
        }
        cooked.push(Some(pair[0].clone()));
        raw.push(Some(pair[1].clone()));
    }
    raw.freeze();
    let raw = heap
        .alloc(HeapObject::Array(Box::new(raw)))
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    let raw_key = heap
        .keys()
        .intern(&JsString::from("raw"))
        .ok_or(Fault::OutOfMemory("property key table exhausted"))?;
    cooked.object_mut().define_with(
        raw_key,
        Value::Object(raw),
        PropertyFlags {
            writable: false,
            enumerable: false,
            configurable: false,
        },
    );
    cooked.freeze();
    let id = heap
        .alloc(HeapObject::Array(Box::new(cooked)))
        .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
    // The realm roots the cooked array and its raw edge before any
    // collection can run; shared executable code has no heap edges.
    templates.insert(key, (code, id));
    drop(templates);
    state.replace(2 * n, Value::Object(id))
}

/// Execute one instruction of `code`, the running frame's code.
pub(crate) fn step(
    state: &mut VmState,
    code: &Code,
    heap: &Heap,
    host: &mut dyn Host,
    instr: Instr,
) -> Result<Flow, Fault> {
    if native::start_conversion(state, instr)? {
        return Ok(Flow::Next);
    }
    if native::is_resumable(instr) {
        // A resumable native: its frame is pushed and its first step runs
        // next (`crate::vm::native`). `pc` was advanced past the instruction.
        let pc = state.top().pc.saturating_sub(1);
        native::start(state, heap, instr, pc)?;
        return Ok(Flow::Next);
    }
    // Everything below, leaf natives included, runs to completion here.
    match instr {
        Instr::Const(i) => {
            let v = code
                .constants
                .get(usize::from(i))
                .cloned()
                .ok_or(Fault::Internal("constant index out of range"))?;
            state.push(v);
        }
        Instr::Undefined => state.push(Value::Undefined),
        Instr::TemplateObject(n) => template_object(state, heap, n)?,
        Instr::GetLocal(i) => {
            let v = state.local(i)?.clone();
            state.push(v);
        }
        Instr::SetLocal(i) => {
            let v = state.pop()?;
            *state.local(i)? = v;
        }
        Instr::Pop => {
            state.pop()?;
        }
        Instr::Dup => {
            let v = state
                .operands()
                .last()
                .cloned()
                .ok_or(Fault::Internal("operand stack underflow"))?;
            state.push(v);
        }
        Instr::Add
        | Instr::Sub
        | Instr::Mul
        | Instr::Lt
        | Instr::Div
        | Instr::Rem
        | Instr::Gt
        | Instr::Le
        | Instr::Ge => {
            let b = state.pop()?;
            let a = state.pop()?;
            let (x, y) = numbers(instr.opcode(), &a, &b)?;
            state.push(match instr {
                Instr::Add => Value::Number(x + y),
                Instr::Sub => Value::Number(x - y),
                Instr::Mul => Value::Number(x * y),
                Instr::Div => Value::Number(x / y),
                // Rust's `%` on f64 is C `fmod`: the result has the sign of
                // the dividend, as JavaScript's `%` requires.
                Instr::Rem => Value::Number(x % y),
                Instr::Lt => Value::Bool(x < y),
                Instr::Gt => Value::Bool(x > y),
                Instr::Le => Value::Bool(x <= y),
                _ => Value::Bool(x >= y),
            });
        }
        Instr::BitAnd | Instr::BitOr | Instr::BitXor | Instr::Shl | Instr::Sar | Instr::UShr => {
            let b = state.pop()?;
            let a = state.pop()?;
            let (x, y) = numbers(instr.opcode(), &a, &b)?;
            let (i, j) = (to_int32(x), to_int32(y));
            // Shift counts are ToUint32(y) & 31; masking the bits of
            // ToInt32(y) gives the same count.
            let count = (j as u32) & 31;
            // Every result is an integer in i32 or u32 range, converted
            // exactly; none is -0.
            state.push(Value::Number(match instr {
                Instr::BitAnd => f64::from(i & j),
                Instr::BitOr => f64::from(i | j),
                Instr::BitXor => f64::from(i ^ j),
                Instr::Shl => f64::from(i << count),
                Instr::Sar => f64::from(i >> count),
                _ => f64::from((i as u32) >> count),
            }));
        }
        Instr::BitNot => match state.pop()? {
            Value::Number(x) => state.push(Value::Number(f64::from(!to_int32(x)))),
            v => {
                return Err(Fault::Unsupported(format!(
                    "bit_not supports number operands only, got {}",
                    v.type_name()
                )));
            }
        },
        Instr::Pow => {
            let b = state.pop()?;
            let a = state.pop()?;
            let (x, y) = numbers(instr.opcode(), &a, &b)?;
            state.push(Value::Number(exponentiate(x, y)));
        }
        Instr::Plus => {
            let b = state.pop()?;
            let a = state.pop()?;
            state.push(plus(&a, &b)?);
        }
        Instr::CompareLt | Instr::CompareGt | Instr::CompareLe | Instr::CompareGe => {
            let b = state.pop()?;
            let a = state.pop()?;
            // IsLessThan gives true, false or undefined (a NaN operand);
            // undefined makes every relational operator false.
            state.push(Value::Bool(match instr {
                Instr::CompareLt => is_less_than(&a, &b)? == Some(true),
                Instr::CompareGt => is_less_than(&b, &a)? == Some(true),
                Instr::CompareLe => is_less_than(&b, &a)? == Some(false),
                _ => is_less_than(&a, &b)? == Some(false),
            }));
        }
        Instr::Typeof => {
            let v = state.pop()?;
            state.push(Value::String(JsString::from(type_of(heap, &v)?)));
        }
        Instr::ToString => {
            let v = state.pop()?;
            state.push(Value::String(to_js_string(&v)?));
        }
        Instr::GetField(i) => {
            let name = name_constant(code, i)?;
            let base = state.pop()?;
            let read = property::read(
                heap,
                state.diag.task,
                &base,
                &property::Key::from_name(&name),
            )?;
            super::native::read_result(state, read, base)?;
        }
        Instr::PutField(i)
        | Instr::DefineField(i)
        | Instr::DefineGetter(i)
        | Instr::DefineSetter(i) => {
            // The value stays on the stack as the result (`o.x = v` is `v`;
            // a literal keeps its object for the next property).
            let name = name_constant(code, i)?;
            let v = state.pop()?;
            let object = state.pop()?;
            let task = state.diag.task;
            if let Instr::PutField(_) = instr {
                let setter = property::prepare_put(
                    heap,
                    task,
                    &object,
                    &property::Key::from_name(&name),
                    v.clone(),
                    code.strict,
                )?;
                super::native::write_result(state, setter, object, v)?;
            } else if matches!(instr, Instr::DefineGetter(_) | Instr::DefineSetter(_)) {
                property::define_accessor(
                    heap,
                    task,
                    &object,
                    &name,
                    v,
                    matches!(instr, Instr::DefineGetter(_)),
                )?;
                state.push(object);
            } else {
                property::define_field(heap, task, &object, &name, v)?;
                state.push(object);
            }
        }
        Instr::GetElem => {
            let key = state.pop()?;
            let base = state.pop()?;
            let key = property::element_key(&base, &key, false)?;
            let read = property::read(heap, state.diag.task, &base, &key)?;
            super::native::read_result(state, read, base)?;
        }
        Instr::PutElem => {
            // ToPropertyKey runs here, after the right-hand side; the value
            // stays on the stack as the result (`o[k] = v` is `v`).
            let v = state.pop()?;
            let key = state.pop()?;
            let base = state.pop()?;
            let key = property::element_key(&base, &key, true)?;
            let setter =
                property::prepare_put(heap, state.diag.task, &base, &key, v.clone(), code.strict)?;
            super::native::write_result(state, setter, base, v)?;
        }
        Instr::Dup2 => {
            let ops = state.operands();
            let start = ops
                .len()
                .checked_sub(2)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            let (a, b) = (ops[start].clone(), ops[start + 1].clone());
            state.push(a);
            state.push(b);
        }
        Instr::ToPropertyKey => {
            // `o[k]` read and then written (`o[k]++`, clj-quickjs-76): the
            // base check and the key conversion run once, before the read.
            // The converted key re-converts to itself in `GetElem` and
            // `PutElem` (an index as a number, any other name as a string),
            // so the conversion stays the only observable one.
            let key = state.pop()?;
            let base = state.operands().last().cloned();
            let base = base.ok_or(Fault::Internal("operand stack underflow"))?;
            let key = match property::element_key(&base, &key, false)? {
                property::Key::Index(index) => Value::Number(f64::from(index)),
                property::Key::Name(name) => Value::String(name),
                property::Key::Symbol(s) => Value::Symbol(s),
            };
            state.push(key);
        }
        Instr::NewArray => {
            // Pushed in the same instruction, like `NewObject`: no
            // collection can run in between.
            let array = property::new_array(heap, state.diag.task)?;
            state.push(array);
        }
        Instr::AppendElement | Instr::AppendHole => {
            // The array stays on the stack for the next element.
            let element = match instr {
                Instr::AppendElement => Some(state.pop()?),
                _ => None,
            };
            let array = state.pop()?;
            property::append(heap, state.diag.task, &array, element)?;
            state.push(array);
        }
        Instr::SetProto => {
            // The literal stays on the stack for the next property.
            let v = state.pop()?;
            let object = state.pop()?;
            prototype::set_literal_proto(heap, state.diag.task, &object, &v)?;
            state.push(object);
        }
        Instr::Instanceof => {
            native::start_instanceof(state, heap)?;
        }
        Instr::Delete => {
            let key = state.pop()?;
            let base = state.pop()?;
            let deleted = property::delete(heap, state.diag.task, &base, &key, code.strict)?;
            state.push(Value::Bool(deleted));
        }
        Instr::In => {
            // Both operands stay on the stack (reachable) while the key is
            // looked up, which may build the realm.
            let [key, base] = top_operands(state, 2)? else {
                return Err(Fault::Internal("operand stack underflow"));
            };
            let result = property::in_operator(heap, state.diag.task, key, base)?;
            state.replace(2, Value::Bool(result))?;
        }
        Instr::ForInStart => {
            // The enumerator is pushed in the same instruction that
            // allocates it, like `NewObject`.
            let v = state.pop()?;
            let iterator = keys::for_in_start(heap, state.diag.task, &v)?;
            state.push(iterator);
        }
        Instr::ForInNext => {
            let iterator = state.pop()?;
            match keys::for_in_next(heap, state.diag.task, &iterator)? {
                Some(key) => {
                    state.push(Value::String(key));
                    state.push(Value::Bool(true));
                }
                None => {
                    state.push(Value::Undefined);
                    state.push(Value::Bool(false));
                }
            }
        }
        Instr::StrictEq => {
            let b = state.pop()?;
            let a = state.pop()?;
            state.push(Value::Bool(a.strict_equals(&b)));
        }
        Instr::Not => {
            let v = state.pop()?;
            state.push(Value::Bool(!v.truthy()));
        }
        Instr::Neg => match state.pop()? {
            Value::Number(x) => state.push(Value::Number(-x)),
            v => {
                return Err(Fault::Unsupported(format!(
                    "neg supports number operands only, got {}",
                    v.type_name()
                )));
            }
        },
        Instr::ToNumber => {
            let v = state.pop()?;
            state.push(Value::Number(to_number(&v)?));
        }
        Instr::ToNumberPair => {
            let b = state.pop()?;
            let a = state.pop()?;
            let x = to_number(&a)?;
            let y = to_number(&b)?;
            state.push(Value::Number(x));
            state.push(Value::Number(y));
        }
        Instr::LooseEq => {
            let b = state.pop()?;
            let a = state.pop()?;
            state.push(Value::Bool(loose_equals(&a, &b)?));
        }
        Instr::ThrowError(kind) => return Err(Fault::Throw(kind)),
        Instr::Jump(target) => state.top_mut().pc = target as usize,
        Instr::JumpIfFalse(target) => {
            if !state.pop()?.truthy() {
                state.top_mut().pc = target as usize;
            }
        }
        Instr::Return => {
            let v = state.pop()?;
            return return_flow(state, v);
        }
        Instr::Throw => return Err(Fault::Thrown(state.pop()?)),
        Instr::PushCatch(target) => state.push_handler(HandlerKind::Catch { target })?,
        Instr::PushFinally(target) => state.push_handler(HandlerKind::Finally { target })?,
        Instr::PopCatch => state.pop_catch()?,
        Instr::PopFinally => state.pop_finally()?,
        Instr::EndFinally => match state.end_finally()? {
            Completion::Normal => {}
            Completion::Return(v) => return return_flow(state, v),
            Completion::Throw(exception) => return Err(Fault::Rethrow(exception)),
            // `end_finally` already continued at the resume point.
            Completion::Jump(_) => {}
            // Cancellation unwinding goes on to the next finally block.
            Completion::Cancel if state.unwind_cancel() => return Ok(Flow::FrameChanged),
            Completion::Cancel => return Ok(Flow::CleanupEnded),
        },
        Instr::CallFinally => state.call_finally()?,
        // A jump out of a cleanup finally block ends the cleanup.
        Instr::DropCompletion => {
            if !state.drop_completion()? {
                return Ok(Flow::CleanupEnded);
            }
        }
        Instr::NewObject => {
            // The new object is pushed in the same instruction; no collection
            // can run in between because this run holds a mutator guard. It
            // is owned by the running task (`crate::object`); its prototype
            // is null.
            let object = OrdinaryObject::new(Some(state.diag.task));
            let id = heap
                .alloc(HeapObject::Plain(object))
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            state.push(Value::Object(id));
        }
        Instr::NewObjectLiteral => {
            // As `NewObject`, but inheriting from `Object.prototype`
            // (`crate::realm`, built on first use).
            let object = builtin::new_object(heap, state.diag.task)?;
            state.push(object);
        }
        Instr::NewArguments => {
            let object = new_arguments(state, heap)?;
            state.push(object);
        }
        Instr::HostEffect(op) => {
            let v = state.pop()?;
            let arg = v.to_primitive().ok_or_else(|| {
                Fault::Unsupported(format!(
                    "host_effect takes a primitive argument, got {}",
                    v.type_name()
                ))
            })?;
            let call = HostCall {
                task: state.diag.task,
                op,
                arg,
            };
            // No reference into the task's VM storage is live across the
            // host call.
            let result = call_leaf(host, call).map_err(Fault::Host)?;
            state.push(Value::from(result));
        }
        Instr::NewVector(_)
        | Instr::NewMap(_)
        | Instr::Count
        | Instr::Nth
        | Instr::Get
        | Instr::GetOr
        | Instr::Assoc
        | Instr::Dissoc
        | Instr::Conj
        | Instr::PopLast
        | Instr::NewVectorBuilder
        | Instr::NewMapBuilder
        | Instr::BuilderConj
        | Instr::BuilderAssoc
        | Instr::Freeze
        | Instr::AtomNew
        | Instr::AtomLoad
        | Instr::AtomCas => intrinsics::step(state, heap, instr)?,
        Instr::Function(i, n) | Instr::Method(i, n) => {
            let child = code
                .functions
                .get(usize::from(i))
                .cloned()
                .ok_or(Fault::Internal("function index out of range"))?;
            if child.captures != n {
                return Err(Fault::Internal("capture count differs from the child code"));
            }
            // The captured values stay on the operand stack (rooted) until
            // the closure holding them replaces them, in the same instruction
            // and under this run's mutator guard.
            let n = usize::from(n);
            let home = if matches!(instr, Instr::Method(..)) {
                let operands = top_operands(state, n + 1)?;
                let Value::Object(home) = operands[0] else {
                    return Err(Fault::Internal("method home object is not an object"));
                };
                Some(home)
            } else {
                None
            };
            let captures: Box<[Value]> = top_operands(state, n)?.into();
            let f = new_function(heap, child, captures, state.diag.task, home)?;
            state.replace(n, f)?;
        }
        Instr::GetCapture(i) => {
            let captured = state.top().callee.and_then(|id| {
                heap.read(id, |object| match object {
                    HeapObject::Function(f) => f.captures.get(usize::from(i)).cloned(),
                    _ => None,
                })
                .flatten()
            });
            // Validation bounds `i` by the code's declared captures, which
            // every closure over the code carries; only code with captures
            // run directly as a task entry (no closure) gets here.
            let v = captured.ok_or(Fault::Internal(
                "captured value read in a frame that is not a closure over this code",
            ))?;
            state.push(v);
        }
        Instr::NewCell => {
            let cell = BindingCell {
                owner: state.diag.task,
                value: None,
            };
            // Pushed in the same instruction, under this run's mutator guard.
            let id = heap
                .alloc(HeapObject::Cell(cell))
                .ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            state.push(Value::Object(id));
        }
        Instr::CellInit | Instr::CellSet => {
            let [cell, v] = top_operands(state, 2)? else {
                return Err(Fault::Internal("operand stack underflow"));
            };
            with_cell(heap, cell, state.diag.task, |binding| {
                if instr == Instr::CellSet && binding.is_none() {
                    return Err(Fault::Throw(ThrowKind::Uninitialized));
                }
                *binding = Some(v.clone());
                Ok(())
            })?;
            state.pop()?;
            state.pop()?;
        }
        Instr::CellGet => {
            let [cell] = top_operands(state, 1)? else {
                return Err(Fault::Internal("operand stack underflow"));
            };
            let v = with_cell(heap, cell, state.diag.task, |binding| {
                binding
                    .clone()
                    .ok_or(Fault::Throw(ThrowKind::Uninitialized))
            })?;
            state.replace(1, v)?;
        }
        Instr::CurrentFunction => {
            let f = match state.top().callee {
                Some(id) => Value::Object(id),
                None => {
                    // The entry frame has no function object until asked for
                    // one; it is kept in the frame (a root) from now on.
                    let f = new_function(
                        heap,
                        Arc::clone(&state.top().code),
                        Box::new([]),
                        state.diag.task,
                        None,
                    )?;
                    if let Value::Object(id) = f {
                        state.top_mut().callee = Some(id);
                    }
                    f
                }
            };
            state.push(f);
        }
        Instr::Reduce | Instr::NeedAll => {
            return Err(Fault::Internal("resumable native on the leaf path"));
        }
        Instr::Need => {
            let kind = state.pop()?;
            let dependency = state.pop()?;
            let (dependency, kind) = need_operands(&dependency, &kind)?;
            return Ok(Flow::Need(dependency, kind));
        }
        Instr::Call(argc) => return call(state, heap, argc, false),
        Instr::CallMethod(argc) => return call(state, heap, argc, true),
        Instr::Construct(argc) => return construct(state, heap, argc),
        Instr::GetThis => {
            let this = this_value(state)?;
            state.push(this);
        }
        Instr::GetGlobalObject => {
            let global = global_object(state, heap)?;
            state.push(global);
        }
        Instr::GetGlobal(i) | Instr::GetGlobalOrUndefined(i) => {
            let name = name_constant(code, i)?;
            let global = global_object(state, heap)?;
            let found = property::read(
                heap,
                state.diag.task,
                &global,
                &property::Key::from_name(&name),
            )?;
            if found.is_none() && !matches!(instr, Instr::GetGlobalOrUndefined(_)) {
                return Err(Fault::Throw(ThrowKind::Unresolvable));
            }
            native::read_result(state, found, global)?;
        }
        Instr::PutGlobal(i) => {
            let name = name_constant(code, i)?;
            let global = global_object(state, heap)?;
            if code.strict
                && !property::has_property(
                    heap,
                    state.diag.task,
                    &global,
                    &property::Key::from_name(&name),
                )?
            {
                return Err(Fault::Throw(ThrowKind::Unresolvable));
            }
            let value = state.pop()?;
            let setter = property::prepare_put(
                heap,
                state.diag.task,
                &global,
                &property::Key::from_name(&name),
                value.clone(),
                code.strict,
            )?;
            native::write_result(state, setter, global, value)?;
        }
        Instr::DeclareGlobalVar(i) | Instr::DeclareGlobalFunction(i) => {
            let name = name_constant(code, i)?;
            let Value::Object(id) = global_object(state, heap)? else {
                return Err(Fault::Internal("missing global object"));
            };
            realm::declare_global(
                heap,
                id,
                &name,
                matches!(instr, Instr::DeclareGlobalFunction(_)),
            )?;
        }
        Instr::GetBuiltinGlobal(i) => {
            // Validation checked the operand. Builds the realm on first use
            // (a poll point); its objects are roots from then on.
            let global = Global::from_u16(i).ok_or(Fault::Internal("unknown built-in global"))?;
            let intrinsics =
                realm::intrinsics(heap).ok_or(Fault::OutOfMemory("heap object table exhausted"))?;
            state.push(intrinsics.global(global));
        }
    }
    Ok(Flow::Next)
}

/// True when the loop polls for a requested collection before running
/// `instr` at `pc` (the VM poll points of `crate::heap::gc`): backward
/// branches (every loop), calls and returns, and every instruction that may
/// allocate. Each is an instruction boundary, where every value the run
/// holds is in its frames. Exhaustive, so a new instruction must decide.
fn polls(instr: Instr, pc: usize) -> bool {
    match instr {
        Instr::Jump(target) | Instr::JumpIfFalse(target) => target as usize <= pc,
        // `CallFinally` may enter a finally block before it, and every cycle
        // through a jump completion's resume point passes one.
        Instr::Call(_) | Instr::CallMethod(_) | Instr::Return | Instr::CallFinally
        | Instr::Construct(_) => true,
        // Builds the realm on first use (`crate::realm`).
        Instr::GetBuiltinGlobal(_) | Instr::GetGlobalObject | Instr::GetGlobal(_)
        | Instr::GetGlobalOrUndefined(_) | Instr::PutGlobal(_)
        | Instr::DeclareGlobalVar(_) | Instr::DeclareGlobalFunction(_) => true,
        // On a function object these may allocate its property record and
        // its lazy `prototype` object, and build the realm
        // (`crate::vm::function`, clj-quickjs-125).
        Instr::GetField(_) | Instr::PutField(_) | Instr::GetElem | Instr::PutElem
        // Deleting initial metadata creates the property record (clj-quickjs-101).
        | Instr::Delete
        // Reads `f.prototype` (clj-quickjs-75).
        | Instr::Instanceof
        // Looks the key up through the chain, functions included
        // (clj-quickjs-74).
        | Instr::In => true,
        Instr::NewObject
        | Instr::TemplateObject(_)
        | Instr::NewObjectLiteral
        | Instr::NewArguments
        | Instr::NewArray
        | Instr::ForInStart
        | Instr::NewVector(_)
        | Instr::NewMap(_)
        | Instr::Assoc
        | Instr::Dissoc
        | Instr::Conj
        | Instr::PopLast
        | Instr::NewVectorBuilder
        | Instr::NewMapBuilder
        | Instr::BuilderConj
        | Instr::BuilderAssoc
        | Instr::Freeze
        | Instr::Function(..)
        | Instr::Method(..)
        | Instr::CurrentFunction
        | Instr::NewCell
        | Instr::AtomNew
        | Instr::AtomLoad
        | Instr::AtomCas => true,
        Instr::Const(_)
        | Instr::Undefined
        | Instr::GetThis
        | Instr::GetLocal(_)
        | Instr::SetLocal(_)
        | Instr::Pop
        | Instr::Dup
        | Instr::Add
        | Instr::Sub
        | Instr::Mul
        | Instr::Lt
        | Instr::StrictEq
        | Instr::Not
        | Instr::HostEffect(_)
        | Instr::Count
        | Instr::Nth
        | Instr::Get
        | Instr::GetOr
        | Instr::Div
        | Instr::Rem
        | Instr::Neg
        | Instr::ToNumber
        | Instr::ToNumberPair
        | Instr::Gt
        | Instr::Le
        | Instr::Ge
        | Instr::LooseEq
        | Instr::BitAnd
        | Instr::BitOr
        | Instr::BitXor
        | Instr::Shl
        | Instr::Sar
        | Instr::UShr
        | Instr::BitNot
        | Instr::Pow
        // A string result lives outside the heap: no allocation to poll for.
        | Instr::Plus
        | Instr::CompareLt
        | Instr::CompareGt
        | Instr::CompareLe
        | Instr::CompareGe
        | Instr::Typeof
        | Instr::ToString
        // Property storage grows outside the object table: no heap object
        // is allocated, so no poll (`crate::vm::property`).
        | Instr::DefineField(_)
        | Instr::DefineGetter(_)
        | Instr::DefineSetter(_)
        | Instr::AppendElement
        | Instr::AppendHole
        // Copy or convert operands only (clj-quickjs-76).
        | Instr::Dup2
        | Instr::ToPropertyKey
        // Re-checks keys only; the key string lives outside the heap
        // (clj-quickjs-85).
        | Instr::ForInNext
        | Instr::SetProto
        | Instr::ThrowError(_)
        | Instr::GetCapture(_)
        | Instr::CellInit
        | Instr::CellGet
        | Instr::CellSet
        // A caught exception polls after unwinding (see `execute`).
        | Instr::Throw
        | Instr::PushCatch(_)
        | Instr::PushFinally(_)
        | Instr::PopCatch
        | Instr::PopFinally
        | Instr::EndFinally
        | Instr::DropCompletion
        // Allocates nothing; each step of the native it starts polls.
        | Instr::Reduce
        | Instr::NeedAll
        // Leaves the loop; continuing after it re-enters `execute`, which
        // polls at run entry.
        | Instr::Need => false,
    }
}

/// Run until the task returns or fails, or until `budget` instructions have
/// executed (`None`: unlimited).
pub(crate) fn execute(
    state: &mut VmState,
    heap: &Heap,
    host: &mut dyn Host,
    mut budget: Option<u64>,
) -> Exit {
    native::assert_no_borrow("reentering the dispatch loop");
    let mut code = Arc::clone(&state.top().code);
    // Run entry is a poll point.
    heap.safepoint(state);
    loop {
        if let Some(left) = &mut budget {
            if *left == 0 {
                return Exit::Yield;
            }
            *left -= 1;
        }
        // A running native frame (no call frame above it) takes the next
        // step instead of the running frame's next instruction; `pc` is then
        // its starting instruction, which a failure or a need reports.
        let (pc, result) = match native::running(state) {
            Some(pc) => {
                // Every step is a poll point: the native's values are in its
                // frame.
                heap.safepoint(state);
                (pc, native::step(state, heap, host))
            }
            None => {
                let pc = state.top().pc;
                let instr = code.instrs.get(pc).copied();
                if instr.is_some_and(|i| polls(i, pc)) {
                    heap.safepoint(state);
                }
                let result = match instr {
                    Some(instr) => {
                        state.top_mut().pc = pc + 1;
                        step(state, &code, heap, host, instr)
                    }
                    None => Err(Fault::Internal("instruction position out of range")),
                };
                (pc, result)
            }
        };
        let raised = match result {
            Ok(Flow::Next) => continue,
            Ok(Flow::FrameChanged) => {
                code = Arc::clone(&state.top().code);
                continue;
            }
            Ok(Flow::Complete(value)) => return Exit::Return(value),
            Ok(Flow::CleanupEnded) => return Exit::CleanupEnded,
            Ok(Flow::Need(dependency, kind)) => {
                native::assert_no_borrow("leaving the loop for a dependency (may suspend)");
                return Exit::Need {
                    dependency,
                    kind,
                    pc,
                    budget,
                };
            }
            Err(Fault::Thrown(v)) => {
                let error = Box::new(thrown_error(state, heap, &v, pc));
                raise(state, Exception { value: v, error })
            }
            Err(Fault::Rethrow(exception)) => raise(state, exception),
            Err(fault) => {
                // A failed instruction leaves the running frame as it was (a
                // failing `Call` pushes no frame), so the failure names it
                // and its callers.
                let (kind, message) = fault_error(fault, &code, pc);
                let error = state.failure(kind, message, pc);
                raise_failure(state, heap, error, Value::Undefined)
            }
        };
        match raised {
            None => {
                code = Arc::clone(&state.top().code);
                // Every value is back in task-owned state: a poll point, so
                // a loop through handlers cannot starve a collection.
                heap.safepoint(state);
            }
            Some(exit) => return exit,
        }
    }
}

/// Early VM tests (hand-assembled bytecode) of the frame stack itself; the
/// embedding-level tests are in `tests/calls.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NoHost;
    use crate::ids::{TaskId, WorldId};
    use crate::value::Primitive;
    use crate::vm::frames::{DiagnosticContext, VmLimits};
    use crate::vm::{Assembler, Executable};

    fn state(code: &Executable, task: u64) -> VmState {
        let diag = DiagnosticContext {
            task: TaskId::from_raw(task),
            world: WorldId::INITIAL,
        };
        VmState::new(&code.0, &[], diag, VmLimits::DEFAULT)
    }

    /// Early VM test: hand-loaded code cannot put mutable edges or invalid
    /// primitive types into the shared realm template registry.
    #[test]
    fn template_object_rejects_nonprimitive_parts() {
        let mut asm = Assembler::new();
        asm.get_local(0)
            .get_local(1)
            .op(Opcode::TemplateObject)
            .raw(1)
            .raw(0)
            .op(Opcode::Return);
        let code = asm.finish("template", 2, 2).unwrap();
        let heap = Heap::default();
        let object = heap
            .alloc(HeapObject::Plain(OrdinaryObject::new(Some(
                TaskId::from_raw(1),
            ))))
            .unwrap();
        for position in 0..2 {
            for invalid in [
                Value::Object(object),
                Value::Bool(true),
                Value::Number(0.0),
                Value::Null,
                Value::Undefined,
            ] {
                if position == 0 && invalid == Value::Undefined {
                    continue;
                }
                let mut state = state(&code, 1);
                *state.local(0).unwrap_or_else(|_| panic!("validated local")) =
                    Value::String(JsString::from("x"));
                *state.local(1).unwrap_or_else(|_| panic!("validated local")) =
                    Value::String(JsString::from("x"));
                *state
                    .local(position)
                    .unwrap_or_else(|_| panic!("validated local")) = invalid;
                let Exit::Fail(error) = execute(&mut state, &heap, &mut NoHost, None) else {
                    panic!("invalid template operand accepted");
                };
                assert_eq!(error.kind, VmErrorKind::Internal);
                assert!(
                    realm::intrinsics(&heap)
                        .unwrap()
                        .templates
                        .lock()
                        .unwrap()
                        .is_empty()
                );
            }
        }
        let mut state = state(&code, 1);
        *state.local(0).unwrap_or_else(|_| panic!("validated local")) = Value::Undefined;
        *state.local(1).unwrap_or_else(|_| panic!("validated local")) =
            Value::String(JsString::from("x"));
        assert!(matches!(
            execute(&mut state, &heap, &mut NoHost, None),
            Exit::Return(Value::Object(_))
        ));
        assert_eq!(
            realm::intrinsics(&heap)
                .unwrap()
                .templates
                .lock()
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn two_tasks_share_function_code_but_not_frames() {
        // function child() { 1; 2; return 3; }  function parent() { return child(); }
        let mut asm = Assembler::new();
        asm.constant(Primitive::Number(1.0)).op(Opcode::Pop);
        asm.constant(Primitive::Number(2.0)).op(Opcode::Pop);
        asm.constant(Primitive::Number(3.0)).op(Opcode::Return);
        let child = asm.finish("child", 0, 0).unwrap();
        let mut asm = Assembler::new();
        asm.function(&child).call(0).op(Opcode::Return);
        let parent = asm.finish("parent", 0, 0).unwrap();
        let heap = Heap::default();
        let (mut a, mut b) = (state(&parent, 1), state(&parent, 2));
        let yielded = |exit| matches!(exit, Exit::Yield);
        assert!(yielded(execute(&mut a, &heap, &mut NoHost, Some(3))));
        assert!(yielded(execute(&mut b, &heap, &mut NoHost, Some(5))));
        let (ca, cb) = (a.frame_code(), b.frame_code());
        assert_eq!((ca.len(), cb.len()), (2, 2), "both are inside child");
        assert!(Arc::ptr_eq(&ca[0], &parent.0) && Arc::ptr_eq(&cb[0], &parent.0));
        assert!(Arc::ptr_eq(&ca[1], &child.0) && Arc::ptr_eq(&cb[1], &child.0));
        // Same code, different saved positions and operands.
        let (pa, oa, _) = a.snapshot();
        let (pb, ob, _) = b.snapshot();
        assert_eq!((pa, oa), (1, vec![Value::Number(1.0)]));
        assert_eq!((pb, ob), (3, vec![Value::Number(2.0)]));
        assert_eq!(a.depth(), 2);
        // Each finishes independently, back in its entry frame.
        for s in [&mut a, &mut b] {
            match execute(s, &heap, &mut NoHost, None) {
                Exit::Return(v) => assert_eq!(v, Value::Number(3.0)),
                other => panic!("{other:?}"),
            }
            assert_eq!(s.depth(), 1);
        }
    }

    #[test]
    fn handler_targets_are_stack_checked_and_round_trip() {
        // A catch target starts one deeper than its `push_catch`.
        let mut asm = Assembler::new();
        let (catch, finally) = (asm.label(), asm.label());
        asm.push_finally(finally).push_catch(catch);
        asm.op(Opcode::Undefined).op(Opcode::Throw);
        asm.bind(catch).op(Opcode::Pop).op(Opcode::PopFinally);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.constant(Primitive::Number(5.0)).op(Opcode::Return);
        let exe = asm.finish("t", 0, 0).unwrap();
        let again = Executable::load(exe.to_spec()).unwrap();
        assert_eq!(again.0.instrs, exe.0.instrs);
        let heap = Heap::default();
        match execute(&mut state(&exe, 1), &heap, &mut NoHost, None) {
            Exit::Return(v) => assert_eq!(v, Value::Number(5.0)),
            other => panic!("{other:?}"),
        }
        // Falling into a catch target at the try block's depth is a mismatch.
        let mut asm = Assembler::new();
        let catch = asm.label();
        asm.push_catch(catch).op(Opcode::PopCatch);
        asm.bind(catch).op(Opcode::Undefined).op(Opcode::Return);
        let e = asm.finish("t", 0, 0).unwrap_err();
        assert!(
            matches!(
                e.kind,
                crate::ValidationErrorKind::StackDepthMismatch { .. }
            ),
            "{e}"
        );
    }

    #[test]
    fn jump_completions_resume_after_call_finally() {
        // `call_finally` runs the finally block, whose `end_finally`
        // continues after the `call_finally`; `drop_completion` discards a
        // pending completion (clj-quickjs-79).
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally).op(Opcode::CallFinally);
        asm.constant(Primitive::Number(5.0)).op(Opcode::Return);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.op(Opcode::Undefined).op(Opcode::Return);
        let exe = asm.finish("t", 0, 0).unwrap();
        let again = Executable::load(exe.to_spec()).unwrap();
        assert_eq!(again.0.instrs, exe.0.instrs);
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally).op(Opcode::PopFinally);
        asm.bind(finally).op(Opcode::DropCompletion);
        asm.constant(Primitive::Number(7.0)).op(Opcode::Return);
        let dropped = asm.finish("t", 0, 0).unwrap();
        let heap = Heap::default();
        for (exe, expected) in [(exe, 5.0), (dropped, 7.0)] {
            let mut s = state(&exe, 1);
            match execute(&mut s, &heap, &mut NoHost, None) {
                Exit::Return(v) => assert_eq!(v, Value::Number(expected)),
                other => panic!("{other:?}"),
            }
            assert!(!s.has_handlers());
        }
    }

    #[test]
    fn mismatched_jump_completions_fail_as_internal_errors() {
        let mut programs = Vec::new();
        // `call_finally` without a finally handler, or with operands above
        // its height; `end_finally` of a jump with operands above it;
        // `drop_completion` without a pending completion.
        programs.push(
            Assembler::new()
                .op(Opcode::CallFinally)
                .op(Opcode::Undefined)
                .op(Opcode::Return)
                .finish("t", 0, 0)
                .unwrap(),
        );
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally)
            .op(Opcode::Undefined)
            .op(Opcode::CallFinally)
            .op(Opcode::Return);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.op(Opcode::Undefined).op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally).op(Opcode::CallFinally);
        asm.op(Opcode::Undefined).op(Opcode::Return);
        asm.bind(finally)
            .op(Opcode::Undefined)
            .op(Opcode::EndFinally)
            .op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally).op(Opcode::DropCompletion);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.op(Opcode::Undefined).op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        let heap = Heap::default();
        for exe in programs {
            match execute(&mut state(&exe, 1), &heap, &mut NoHost, None) {
                Exit::Fail(e) => assert_eq!(e.kind, VmErrorKind::Internal, "{e}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn mismatched_handler_instructions_fail_as_internal_errors() {
        // Pops with no matching entry, and a handler whose frame's operands
        // dropped below its height before the throw (hand-assembled code).
        let mut programs = Vec::new();
        programs.push(
            Assembler::new()
                .op(Opcode::PopCatch)
                .op(Opcode::Undefined)
                .op(Opcode::Return)
                .finish("t", 0, 0)
                .unwrap(),
        );
        let mut asm = Assembler::new();
        let finally = asm.label();
        asm.push_finally(finally).op(Opcode::PopCatch);
        asm.bind(finally).op(Opcode::EndFinally);
        asm.op(Opcode::Undefined).op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        programs.push(
            Assembler::new()
                .op(Opcode::EndFinally)
                .op(Opcode::Undefined)
                .op(Opcode::Return)
                .finish("t", 0, 0)
                .unwrap(),
        );
        let mut asm = Assembler::new();
        let catch = asm.label();
        asm.op(Opcode::Undefined).push_catch(catch).op(Opcode::Pop);
        asm.op(Opcode::Undefined).op(Opcode::Throw);
        asm.bind(catch).op(Opcode::Return);
        programs.push(asm.finish("t", 0, 0).unwrap());
        let heap = Heap::default();
        for exe in programs {
            match execute(&mut state(&exe, 1), &heap, &mut NoHost, None) {
                Exit::Fail(e) => assert_eq!(e.kind, VmErrorKind::Internal, "{e}"),
                other => panic!("{other:?}"),
            }
        }
    }

    #[test]
    fn broken_call_invariants_fail_instead_of_panicking() {
        let child = Assembler::new()
            .op(Opcode::Undefined)
            .op(Opcode::Return)
            .finish("child", 0, 0)
            .unwrap();
        // Code that skipped validation: a call with no callee, a function
        // index past the table, and a call with fewer operands than its count.
        let programs: [(Vec<Instr>, Vec<Arc<Code>>); 3] = [
            (vec![Instr::Call(0)], vec![]),
            (vec![Instr::Function(0, 0)], vec![]),
            (
                vec![Instr::Function(0, 0), Instr::Call(2)],
                vec![Arc::clone(&child.0)],
            ),
        ];
        let heap = Heap::default();
        for (instrs, functions) in programs {
            let code = Code {
                name: "corrupt".into(),
                params: 0,
                function_length: 0,
                function_name: crate::value::JsString::from(""),
                locals: 0,
                constants: Box::new([]),
                functions: functions.into(),
                captures: 0,
                strict: true,
                constructor: false,
                uses_arguments: false,
                offsets: (0..instrs.len() as u32).collect(),
                instrs: instrs.clone().into(),
                max_stack: 0,
                debug: None,
            };
            let mut s = state(&Executable(Arc::new(code)), 1);
            match execute(&mut s, &heap, &mut NoHost, None) {
                Exit::Fail(e) => assert_eq!(e.kind, VmErrorKind::Internal, "{instrs:?}"),
                other => panic!("{instrs:?}: {other:?}"),
            }
        }
    }

    /// `typeof` (clj-quickjs-69) classifies every heap object kind: a
    /// function object is "function", each other kind "object". Early VM
    /// test (hand-built bytecode, for the kinds JavaScript source cannot
    /// make), not JavaScript source support.
    #[test]
    fn typeof_classifies_every_object_kind() {
        let heap = Heap::default();
        let type_of_result = |asm: &mut Assembler| {
            asm.op(Opcode::Typeof).op(Opcode::Return);
            let exe = asm.finish("t", 0, 0).unwrap();
            match execute(&mut state(&exe, 1), &heap, &mut NoHost, None) {
                Exit::Return(Value::String(s)) => s.to_string(),
                other => panic!("{other:?}"),
            }
        };
        let mut asm = Assembler::new();
        asm.op(Opcode::Undefined).op(Opcode::Return);
        let child = asm.finish("child", 0, 0).unwrap();
        let mut asm = Assembler::new();
        asm.function(&child);
        assert_eq!(type_of_result(&mut asm), "function");
        // A built-in function (clj-quickjs-117).
        let mut asm = Assembler::new();
        let object = crate::realm::Global::Object as u16;
        asm.op(Opcode::GetBuiltinGlobal)
            .raw(object.to_le_bytes()[0])
            .raw(object.to_le_bytes()[1]);
        assert_eq!(type_of_result(&mut asm), "function");
        type Produce = fn(&mut Assembler);
        let producers: [(&str, Produce); 8] = [
            ("plain object", |asm| {
                asm.op(Opcode::NewObject);
            }),
            ("vector", |asm| {
                asm.new_vector(0);
            }),
            ("map", |asm| {
                asm.new_map(0);
            }),
            ("vector builder", |asm| {
                asm.op(Opcode::NewVectorBuilder);
            }),
            ("map builder", |asm| {
                asm.op(Opcode::NewMapBuilder);
            }),
            ("binding cell", |asm| {
                asm.op(Opcode::NewCell);
            }),
            ("atom", |asm| {
                asm.op(Opcode::Undefined).op(Opcode::AtomNew);
            }),
            // A caught VM error is a native error object.
            ("error object", |asm| {
                let catch = asm.label();
                asm.push_catch(catch);
                let kind = (ThrowKind::Unresolvable as u16).to_le_bytes();
                asm.op(Opcode::ThrowError).raw(kind[0]).raw(kind[1]);
                asm.bind(catch);
            }),
        ];
        for (kind, produce) in producers {
            let mut asm = Assembler::new();
            produce(&mut asm);
            assert_eq!(type_of_result(&mut asm), "object", "{kind}");
        }
        // A syntax occurrence has no producing instruction.
        let provenance = crate::collections::Provenance {
            source: 0,
            line: 1,
            column: 1,
        };
        let occurrence =
            crate::collections::new_occurrence(&heap, Value::Undefined, provenance).unwrap();
        assert_eq!(type_of(&heap, &occurrence).ok(), Some("object"));
        // Primitives.
        for (v, expected) in [
            (Value::Undefined, "undefined"),
            (Value::Null, "object"),
            (Value::Bool(false), "boolean"),
            (Value::Number(f64::NAN), "number"),
            (Value::String(JsString::from("s")), "string"),
            (Value::Symbol(crate::value::JsSymbol::new(None)), "symbol"),
        ] {
            assert_eq!(type_of(&heap, &v).ok(), Some(expected));
        }
    }
}
