//! Resumable native operations: task-owned native continuations driven by
//! the interpreter loop (`docs/design.md` §§5, 10–11).
//!
//! # Leaf and resumable natives
//!
//! A native operation is Rust code the VM runs on behalf of an instruction.
//! There are two kinds; coercing instructions choose their path from their operands:
//!
//! * **Leaf natives** cannot call JavaScript and cannot demand a
//!   dependency: the collection, builder and atom intrinsics
//!   ([`crate::vm::intrinsics`]) and `HostEffect` ([`crate::host`]). Each is
//!   an ordinary Rust call made inside its instruction, which runs to
//!   completion in one step of the loop; no state is saved for it and no
//!   continuation object exists. That is the fast path, and it stays one.
//! * **Resumable natives** may call JavaScript callbacks and demand
//!   dependencies, so the task may yield, suspend, be collected around and
//!   migrate to another thread in the middle of one. They include `Reduce`
//!   (`__rt.reduce`), `NeedAll` (`__rt.needAll`) and object operand
//!   conversion (`start_conversion`), accessor calls, descriptor reads,
//!   apply/push and bind metadata. Their starting operation only
//!   validates and pops its operands into a [`NativeFrame`] pushed on the
//!   task's VM state; the loop then drives that frame one *step* at a time.
//!
//! [`is_resumable`] classifies unconditional native instructions;
//! [`start_conversion`] selects the object slow path of coercing instructions.
//!
//! # The protocol
//!
//! A step receives what the previous action produced and returns the next
//! action. Inputs:
//!
//! * `Start`: the first step, right after the instruction.
//! * `CallResult(v)`: the JavaScript callback the previous step called
//!   returned `v`.
//! * `Dependency(v)`: the dependency the previous step demanded is ready
//!   with the shared value `v`.
//!
//! Actions:
//!
//! * `Call { callee, this, args }`: invoke a JavaScript function. The trampoline
//!   (the dispatch loop) pushes the callee and arguments in the native's
//!   operand area and enters a new task-owned VM frame for it, exactly as a
//!   `Call` instruction does; the loop then runs the callback's
//!   instructions. Its `Return` delivers the result into the native's
//!   operand area and the loop resumes the native with `CallResult`. A
//!   built-in function callee (`crate::vm::builtin`, clj-quickjs-117) runs
//!   to completion or pushes its own conversion continuation (Number/String
//!   constructors and string code-unit built-ins with object operands);
//!   its result is delivered the same way, with
//!   the supplied `this` (undefined for reduce); a forwarding built-in (`Reflect.apply`,
//!   clj-quickjs-102) is first followed to the call it makes
//!   (`crate::vm::dispatch::resolve`), whose frame then delivers the result.
//! * `Need { dependency, kind }`: demand result `kind` of `dependency` in
//!   the task's world. The loop leaves with `Exit::Need`, like a `Need`
//!   instruction, and the run guard performs the dependency protocol
//!   (`crate::task`, "Dependency waits in the VM"): a ready result is pushed
//!   into the native's operand area at once; a pending one suspends the
//!   whole task, and the run that resumes it pushes the result first. Either
//!   way the next step gets `Dependency`.
//! * `Return(v)`: the operation is complete. The frame is popped and `v` is
//!   pushed onto the calling frame's operands, as the instruction's result;
//!   the calling frame continues after the instruction.
//! * `Resume { instr, operands }`: conversion is complete. Pop the native,
//!   restore the converted operands and execute the original leaf instruction.
//!
//! Failures are delivered too (clj-quickjs-28, "Errors" below): when an
//! exception unwinds through a pending native, the native receives
//! `Failure::Callback` (the callback it called raised, and nothing inside the
//! callback caught it), `Failure::Dependency` (the dependency it demanded
//! failed or was cancelled: the producer's failure) or `Failure::Step` (its own
//! step failed), and answers with the action `Unwind`: release the frame
//! and let the exception go on to the handler below it. No current
//! native recovers from a failure; any other answer is refused as an
//! internal error, so a native can never resume after its frame was
//! partially unwound.
//!
//! No Rust frame represents a pending native: between steps everything it
//! needs is in its `NativeFrame` (a trampoline, not a callback stack), so a
//! callback that suspends the task, or a budget that runs out between two
//! steps, ends the run like any other instruction boundary, and any thread
//! may continue it.
//!
//! # Native frames
//!
//! A [`NativeFrame`] holds, in task-owned storage:
//!
//! * the operation's arguments (the callback, the collection, a result
//!   kind), its progress index and length, and its intermediate value (the
//!   accumulator, or the results vector so far);
//! * what it waits for (a callback result, a dependency, or nothing) and the
//!   result destination: the value-stack slot `base`, the native's own
//!   operand area just above the calling frame's operands, where a
//!   callback's `Return` or the dependency protocol puts the completion;
//! * its diagnostic context: the index of the instruction that started it
//!   in the calling frame's code, which failures and a suspended task's
//!   [`source_location`](crate::Task::source_location) report (the task and
//!   world are the task's, in `VmState::diag`: a native never changes
//!   world, and its dependencies are read in the task's world).
//!
//! Native frames are stacked beside the call frames: each records the call
//! depth of the frame that started it, and it is *running* when no call
//! frame is above that depth. A callback may itself start a resumable
//! native, so frames nest. A coercing built-in callback can push a
//! conversion at the same call depth; its parent awaits its return. Only
//! the innermost frame at that depth runs. These pushes check the native
//! count against the task's call-depth limit.
//!
//! # Rooting, locks and safepoints
//!
//! Native frames are part of the task's VM state, so the collector traces
//! them wherever it traces the task's frames (`RootCategory::TaskFrame`):
//! through the task's root source while it is suspended or yielded, and
//! from the runner at a VM safepoint while it runs. A value held only by a
//! pending native (the collection being folded, its unvisited elements, the
//! accumulator) stays alive; when the native returns, an exception or a
//! cancellation unwinds through it ("Errors" below), or the task finishes
//! (completes, fails or is cancelled), its frame is dropped and those values
//! become garbage unless something else holds them.
//!
//! The loop polls for a requested collection before every step, at which
//! point every value the native holds is in its frame. Within a step,
//! values are read in place: a delivered result stays in the operand area
//! (a root) until the step has stored it in the frame, and anything a step
//! allocates is stored in the frame before the step returns. A step reads
//! one element at a time through [`crate::collections`], which takes and
//! releases the heap's lock inside the call; no heap lock, `Heap::read`
//! closure or Rust reference into `VmState` is held across a step boundary,
//! so none is held across a callback (reentry), stack growth, a collection
//! or a suspension.
//!
//! # Budget
//!
//! Each step counts as one instruction against the run's [`Budget`](crate::Budget),
//! checked between steps like instructions, so a long fold over a vector
//! whose callbacks are cheap still yields. The starting instruction is one
//! more.
//!
//! # Errors (clj-quickjs-28)
//!
//! A failure in a step (an invalid definition id, a failed or cancelled
//! dependency) is reported at the starting instruction; a failure inside a
//! callback is reported there, with the calling frame in its call chain at
//! the starting instruction. Whether it is an exception follows the usual
//! rule (`crate::vm::exception`): a `throw`, a catchable run-time error or a
//! failed or cancelled dependency is; anything else ends the run at once.
//!
//! An exception unwinds through native frames like through call frames
//! ([`VmState::raise`](super::frames::VmState::raise)):
//!
//! * a `try` inside a callback catches what the callback raises, and the
//!   fold continues with the value its `catch` returns;
//! * otherwise, when the innermost handler belongs to the frame that started
//!   a pending native or to one of its callers, the exception passes that
//!   native: the callback frames above it are popped with their operands,
//!   the native receives the failure (`Failure::Callback`, `Failure::Dependency` or
//!   `Failure::Step`, by what it was waiting for), answers `Unwind`, and its
//!   frame is popped ([`unwind`]): the collection, the callback, the
//!   accumulator or results so far, and the operand area are released, so
//!   nothing the native held is a root any more. Nested natives are passed
//!   innermost first. The handler then runs as for any exception: a catch
//!   block sees the exception value (for a dependency, the error object
//!   whose cause is the producer's published value), a finally block runs
//!   with the exception pending;
//! * a failure of the starting instruction itself (a callee that is not a
//!   function, an operand that is not a vector) happens before the native
//!   frame exists and is caught like any failing instruction's.
//!
//! A dependency demanded by `needAll` that fails after the task suspended
//! is delivered by the resuming run at the starting instruction (the
//! pending need is cleared first), so a task that waited inside a native
//! and resumes on another thread or worker unwinds exactly as one that
//! found the failure at once.
//!
//! **Cancellation** ([`crate::Task::cancel`], `crate::task`, "Cancellation
//! and finally blocks") is not an exception and gives a native no step: the
//! observing run leaves the cell the task waits on (so the producer and its
//! other consumers are unaffected), and unwinding to the next finally
//! handler drops every native frame it passes without delivering anything
//! ([`discard`]): no native code runs for a cancelled task, and nothing it
//! held stays a root. A finally block inside a callback runs without
//! popping the native below it; when that block ends, the cancellation goes
//! on to the next handler, dropping the native then. If no finally block is
//! left, terminal cleanup drops the whole VM state, native frames included.
//!
//! # Safepoint contract and debug borrow checks
//!
//! `docs/design.md` §10: "Do not hold Rust references into mutable VM
//! storage across operations that may reenter, collect, grow storage, or
//! suspend." A step and a failure delivery run while holding a **scoped
//! borrow** ([`ScopedBorrow`], taken by the trampoline): a `&mut` into the
//! task's native frame, and possibly heap object reads. The operations that
//! reenter, collect, grow storage or suspend check that the running thread
//! holds no scoped borrow ([`assert_no_borrow`]): entering the dispatch loop
//! or starting a task run (reentry), a VM safepoint (collection), pushing a
//! value, a call frame or a native frame (storage growth), and leaving the
//! loop for a dependency or parking (suspension). A native that does any of
//! them from inside its step, instead of returning the corresponding action
//! to the trampoline, violates the contract: debug builds reject it with a
//! panic naming the operation; release builds compile the checks away (the
//! counter does not exist), so the contract is then upheld by construction
//! only: a step receives `&mut self` and `&Heap`, never the `VmState`.
//! Native code that needs its own scoped borrow takes one with
//! [`scoped_borrow`].
//!
//! # QuickJS
//!
//! QuickJS's `js_call_c_function` (`research/quickjs/quickjs.c`, revision
//! 7d1c13c5d8f8) calls a C function on the C stack, which reenters
//! `JS_CallInternal` for callbacks; that recursion is what this protocol
//! replaces. Its `JSAsyncFunctionState` (saved frame, arguments, locals,
//! and resume position) is the inventory of state a pending native must
//! keep, but no promise or job is involved here.

use std::marker::PhantomData;
mod string_raw;

use super::builtin;
use super::dispatch::{self, Callee, Fault, Flow, dependency_id, describe, result_kind};
use super::frames::VmState;
use super::instr::Instr;
use crate::collections as coll;
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId};
use crate::host::Host;
use crate::ids::{DependencyId, ResultKind, TaskId};
use crate::value::{JsString, Value};
use crate::vm::{descriptor, property, prototype};

pub(crate) mod error;

/// What a native frame is waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Awaiting {
    /// Nothing: the next step is the first (`Start`).
    Nothing,
    /// The return value of the callback its last step called.
    CallResult,
    /// The value of the dependency its last step demanded.
    Dependency,
}

/// The completion delivered to a step.
#[derive(Clone, Debug)]
enum Input {
    Start,
    CallResult(Value),
    Dependency(Value),
}

/// A failure delivered to a native frame an exception unwinds through
/// (module docs, "Errors"), named by what the native was waiting for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Failure {
    /// The callback its last step called raised an exception that nothing
    /// inside the callback caught (or calling it failed).
    Callback,
    /// The dependency its last step demanded failed or was cancelled: the
    /// exception is the error object for the producer's failure.
    Dependency,
    /// Its own step raised a catchable failure.
    Step,
}

/// What a step asks the trampoline to do next.
#[derive(Debug)]
enum Action {
    /// Keep this native frame runnable for another budgeted step.
    Continue,

    ToNumber(Value),
    /// Limit leaf StringToNumber after observable ToPrimitive completes.
    ToNumberLimited(Value, usize),
    ToString(Value),
    /// Replace a completed conversion with the remaining built-in work.
    Builtin {
        which: builtin::Builtin,
        args: Vec<Value>,
    },
    /// Call `callee` with these arguments in a new VM frame.
    Call {
        callee: Value,
        this: Value,
        args: Vec<Value>,
    },
    /// Run the original instruction with its converted operands. This is
    /// one leaf instruction, never interpreter re-entry.
    Resume {
        instr: Instr,
        operands: Vec<Value>,
    },
    /// Read a dependency in the task's world, suspending if it is pending.
    Need {
        dependency: DependencyId,
        kind: ResultKind,
    },
    /// Complete with this value.
    Return(Value),
    /// The answer to a [`Failure`]: release the frame and let the exception
    /// unwind on to the handler below it. Never the result of a step.
    Unwind,
}

/// ToPrimitive's preferred primitive type. Number and default share the
/// ordinary method order; they remain distinct for future exotic methods.
#[derive(Clone, Copy, Debug)]
enum Hint {
    Default,
    Number,
    String,
}

/// What to finish after OrdinaryToPrimitive has converted its operands.
#[derive(Clone, Copy, Debug)]
enum ConvertTarget {
    SymbolDescription,
    SymbolRegistryKey,
    StringSplit {
        separator_undefined: bool,
        limit_undefined: bool,
    },
    NumberValue,
    StringValue,
    Instruction(Instr),
    PrimitiveConstructor {
        which: builtin::Builtin,
        construct: bool,
    },
    StringBuiltin {
        which: builtin::Builtin,
        access: bool,
        // Running concat length: reject overflow before coercing a later argument.
        concat_len: usize,
    },
    NumberPredicate(builtin::Builtin),
    NumberToString {
        number: f64,
    },
    NumberParse {
        integer: bool,
    },
    PropertyBuiltin {
        which: builtin::Builtin,
    },
    Math {
        which: builtin::Builtin,
        // Largest absolute operand, gathered during ToNumber for hypot.
        magnitude: f64,
    },
    NumberValueLimited(usize),
}

/// The operation-specific state of a native frame.
#[derive(Debug)]
enum NativeState {
    StringSplit {
        split: Box<crate::value::string::StringSplit>,
        out: Value,
        remaining: u32,
    },
    Error(error::ErrorState),
    Bind {
        target: Value,
        args: Vec<Value>,
        length: Value,
        stage: u8,
    },
    Apply {
        callee: Value,
        this: Value,
        list: Value,
        args: Vec<Value>,
        len: Option<usize>,
        max_args: usize,
        called: bool,
    },
    Push {
        this: Value,
        args: Vec<Value>,
        len: Option<u64>,
        next: usize,
        finishing: bool,
    },
    Instanceof {
        value: Value,
        target: Value,
    },
    DefineProperty(descriptor::DefineProperty),
    /// One property callback; a setter discards its return value and
    /// completes with the assignment's original right-hand side.
    PropertyCall {
        callee: Value,
        this: Value,
        assigned: Option<Value>,
    },
    /// `reduce(f, init, vec)`: `acc = f(acc, x)` for each element `x` of
    /// `vec` in order, starting from `init`.
    Reduce {
        f: Value,
        vec: Value,
        /// The accumulator: `init`, then each callback's result.
        acc: Value,
        /// The element whose callback is in flight, or the next to visit.
        next: usize,
        len: usize,
    },
    /// `needAll(defs, kind)`: the results of needing `kind` of each
    /// definition in the vector `defs`, in order, as a new vector.
    NeedAll {
        defs: Value,
        kind: ResultKind,
        /// The results so far (a persistent vector once started).
        out: Value,
        /// The element whose dependency is demanded, or the next to demand.
        next: usize,
        len: usize,
    },
    /// OrdinaryToPrimitive, left to right, before completing `target`.
    /// The original receiver stays in `operands[next]` until its method
    /// returns a primitive; completed operands are never converted again.
    Convert {
        target: ConvertTarget,
        operands: Vec<Value>,
        next: usize,
        end: usize,
        hint: Hint,
        method: usize,
        method_get: bool,
    },
    StringSearch(crate::value::string::StringSearch),
    StringBuild(crate::value::string::StringBuild),
    NumberParse(Box<crate::value::string::NumberParse>),
    /// All operands have undergone ToNumber. Sum scaled squares with Kahan
    /// compensation, one operand per budgeted step; Infinity takes precedence
    /// over NaN only after every argument's conversion has run.
    MathHypot {
        operands: Vec<Value>,
        next: usize,
        magnitude: f64,
        sum: f64,
        correction: f64,
    },
    Join(Box<ArrayJoin>),
    StringRaw(Box<string_raw::StringRaw>),
    ArrayToString {
        this: Value,
        stage: u8,
    },
    Json {
        text: Value,
        reviver: Value,
        converted: bool,
        parser: Option<Box<super::json::Parser>>,
    },
    JsonStringify(Box<super::json::Stringifier>),
}

#[derive(Debug)]
enum JoinStage {
    Length,
    LengthValue,
    Separator,
    SeparatorValue,
    Element,
    ElementValue,
    ElementString,
}

/// Each step visits at most one element. String bodies are retained as
/// immutable parts, then copied by StringBuild in 256-unit slices.
#[derive(Debug)]
struct ArrayJoin {
    this: Value,
    separator: Value,
    stage: JoinStage,
    len: u64,
    next: u64,
    parts: Vec<(JsString, usize, usize)>,
    units: usize,
}

impl ArrayJoin {
    fn append(&mut self, text: JsString) -> Result<(), Fault> {
        let len = text.len();
        self.units = self
            .units
            .checked_add(len)
            .filter(|n| *n <= crate::value::string::MAX_STRING_LEN)
            .ok_or_else(|| Fault::RangeError("string too long".into()))?;
        if len != 0 {
            self.parts.push((text, 0, len));
        }
        Ok(())
    }

    fn element(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Object(_)) {
            self.stage = JoinStage::ElementString;
            return Ok(Action::ToString(value));
        }
        if !matches!(value, Value::Undefined | Value::Null) {
            self.append(dispatch::to_js_string(&value)?)?;
        }
        self.next += 1;
        self.stage = JoinStage::Element;
        Ok(Action::Continue)
    }

    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self.stage {
            JoinStage::Length => {
                self.stage = JoinStage::LengthValue;
                match property::read(
                    heap,
                    task,
                    &self.this,
                    &property::Key::Name(JsString::from("length")),
                )? {
                    Some(property::Read::Getter(callee)) => Ok(Action::Call {
                        callee,
                        this: self.this.clone(),
                        args: vec![],
                    }),
                    value => self.length(match value {
                        Some(property::Read::Value(v)) => v,
                        _ => Value::Undefined,
                    }),
                }
            }
            JoinStage::LengthValue => {
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("join expected length"));
                };
                self.length(value)
            }
            JoinStage::Separator => {
                self.stage = JoinStage::SeparatorValue;
                if matches!(self.separator, Value::Undefined) {
                    self.separator = Value::String(JsString::from(","));
                }
                if matches!(self.separator, Value::Object(_)) {
                    return Ok(Action::ToString(self.separator.clone()));
                }
                self.separator = Value::String(dispatch::to_js_string(&self.separator)?);
                self.stage = JoinStage::Element;
                Ok(Action::Continue)
            }
            JoinStage::SeparatorValue => {
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("join expected separator"));
                };
                self.separator = value;
                self.stage = JoinStage::Element;
                Ok(Action::Continue)
            }
            JoinStage::Element => {
                if self.next > 0 {
                    let Value::String(separator) = &self.separator else {
                        return Err(Fault::Internal("join separator is not a string"));
                    };
                    self.append(separator.clone())?;
                }
                self.stage = JoinStage::ElementValue;
                match property::read(heap, task, &self.this, &index_key(self.next))? {
                    Some(property::Read::Getter(callee)) => Ok(Action::Call {
                        callee,
                        this: self.this.clone(),
                        args: vec![],
                    }),
                    Some(property::Read::Value(value)) => self.element(value),
                    None => self.element(Value::Undefined),
                }
            }
            JoinStage::ElementValue => {
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("join expected element"));
                };
                self.element(value)
            }
            JoinStage::ElementString => {
                let Input::CallResult(Value::String(text)) = input else {
                    return Err(Fault::Internal("join expected element string"));
                };
                self.append(text)?;
                self.next += 1;
                self.stage = JoinStage::Element;
                Ok(Action::Continue)
            }
        }
    }

    fn length(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Object(_)) {
            return Ok(Action::ToNumber(value));
        }
        self.len = to_length(&value)?;
        self.stage = JoinStage::Separator;
        Ok(Action::Continue)
    }
}

/// A pending resumable native operation. See the module docs.
#[derive(Debug)]
pub(crate) struct NativeFrame {
    /// Call depth of the frame that started it; the native is running when
    /// the task's depth equals this.
    depth: usize,
    /// Value-stack index of the native's operand area: the destination of a
    /// callback's result or a dependency's value.
    base: usize,
    /// Index of the starting instruction in the calling frame's code.
    pc: usize,
    awaiting: Awaiting,
    state: NativeState,
}

impl Trace for NativeFrame {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match &self.state {
            NativeState::StringSplit { out, .. } => out.trace(visit),
            NativeState::JsonStringify(stringifier) => stringifier.trace(visit),
            NativeState::Json {
                text,
                reviver,
                parser,
                ..
            } => {
                text.trace(visit);
                reviver.trace(visit);
                if let Some(parser) = parser {
                    parser.trace(visit);
                }
            }
            NativeState::StringRaw(raw) => raw.trace(visit),
            NativeState::Join(join) => {
                join.this.trace(visit);
                join.separator.trace(visit);
            }
            NativeState::ArrayToString { this, .. } => this.trace(visit),
            NativeState::Error(error) => error.trace(visit),
            NativeState::Reduce { f, vec, acc, .. } => {
                for v in [f, vec, acc] {
                    v.trace(visit);
                }
            }
            NativeState::NeedAll { defs, out, .. } => {
                defs.trace(visit);
                out.trace(visit);
            }
            NativeState::Convert { operands, .. } | NativeState::MathHypot { operands, .. } => {
                for v in operands {
                    v.trace(visit);
                }
            }
            NativeState::StringSearch(_)
            | NativeState::StringBuild(_)
            | NativeState::NumberParse(_) => {} // No heap edges.

            NativeState::PropertyCall {
                callee,
                this,
                assigned,
            } => {
                callee.trace(visit);
                this.trace(visit);
                if let Some(value) = assigned {
                    value.trace(visit);
                }
            }
            NativeState::DefineProperty(desc) => desc.trace(visit),
            NativeState::Instanceof { value, target } => {
                value.trace(visit);
                target.trace(visit);
            }
            NativeState::Apply {
                callee,
                this,
                list,
                args,
                ..
            } => {
                callee.trace(visit);
                this.trace(visit);
                list.trace(visit);
                for v in args {
                    v.trace(visit);
                }
            }
            NativeState::Push { this, args, .. } => {
                this.trace(visit);
                for v in args {
                    v.trace(visit);
                }
            }
            NativeState::Bind {
                target,
                args,
                length,
                ..
            } => {
                target.trace(visit);
                length.trace(visit);
                for v in args {
                    v.trace(visit);
                }
            }
        }
    }
}

/// True for the instructions that start a resumable native; every other
/// instruction (leaf natives included) runs to completion in one step.
/// Exhaustive, so a new instruction must decide.
pub(crate) fn is_resumable(instr: Instr) -> bool {
    use Instr::*;
    match instr {
        Reduce | NeedAll => true,
        Const(_) | Undefined | GetLocal(_) | SetLocal(_) | Pop | Dup | Add | Sub | Mul | Lt
        | StrictEq | Not | Jump(_) | JumpIfFalse(_) | Return | NewObject | HostEffect(_)
        | NewVector(_) | NewMap(_) | TemplateObject(_) | Count | Nth | Get | GetOr | Assoc
        | Dissoc | Conj | PopLast | NewVectorBuilder | NewMapBuilder | BuilderConj
        | BuilderAssoc | Freeze | Div | Rem | Neg | ToNumber | ToNumberPair | Gt | Le | Ge
        | LooseEq | ThrowError(_) | Function(..) | Call(_) | CurrentFunction | GetCapture(_)
        | NewCell | CellInit | CellGet | CellSet | AtomNew | AtomLoad | AtomCas | Need | Throw
        | PushCatch(_) | PushFinally(_) | PopCatch | PopFinally | EndFinally | BitAnd | BitOr
        | BitXor | Shl | Sar | UShr | BitNot | Pow | CallFinally | DropCompletion | Plus
        | CompareLt | CompareGt | CompareLe | CompareGe | Typeof | GetField(_) | PutField(_)
        | DefineField(_) | GetBuiltinGlobal(_) | CallMethod(_) | NewObjectLiteral | ToString
        | GetThis | NewArray | AppendElement | AppendHole | GetElem | PutElem | Dup2
        | ToPropertyKey | Construct(_) | SetProto | Instanceof | Delete | In | ForInStart
        | ForInNext | NewArguments | Method(..) | DefineGetter(_) | DefineSetter(_) => false,
        GetGlobalObject
        | GetGlobal(_)
        | GetGlobalOrUndefined(_)
        | PutGlobal(_)
        | DeclareGlobalVar(_)
        | DeclareGlobalFunction(_) => false,
    }
}

/// The top `N` operands of the running frame, oldest first, not popped.
fn operands<const N: usize>(state: &VmState) -> Result<[Value; N], Fault> {
    let ops = state.operands();
    ops.len()
        .checked_sub(N)
        .map(|at| std::array::from_fn(|i| ops[at + i].clone()))
        .ok_or(Fault::Internal("operand stack underflow"))
}

/// Start a continuation only when an instruction actually needs to
/// convert an object. Primitive operands retain their single-step path.
/// Property operations retain their base checks before observable key
/// conversion; equality never converts two objects or a nullish pair.
pub(crate) fn start_conversion(state: &mut VmState, instr: Instr) -> Result<bool, Fault> {
    use Instr::*;
    let (count, first, end, hint) = match instr {
        ToNumber => (1, 0, 1, Hint::Number),
        ToString => (1, 0, 1, Hint::String),
        Plus => (2, 0, 2, Hint::Default),
        ToNumberPair | CompareLt | CompareGt | CompareLe | CompareGe => (2, 0, 2, Hint::Number),
        LooseEq => {
            let [a, b] = operands::<2>(state)?;
            if matches!(a, Value::Undefined | Value::Null)
                || matches!(b, Value::Undefined | Value::Null)
                || matches!((&a, &b), (Value::Object(_), Value::Object(_)))
            {
                return Ok(false);
            }
            (2, 0, 2, Hint::Default)
        }
        GetElem | ToPropertyKey | Delete | PutElem => {
            let count = if matches!(instr, PutElem) { 3 } else { 2 };
            let ops = state.operands();
            let base = ops
                .get(
                    ops.len()
                        .checked_sub(count)
                        .ok_or(Fault::Internal("operand stack underflow"))?,
                )
                .ok_or(Fault::Internal("operand stack underflow"))?;
            if matches!(base, Value::Undefined | Value::Null) {
                return Ok(false);
            }
            (count, 1, 2, Hint::String)
        }
        In => {
            let [_, base] = operands::<2>(state)?;
            if !matches!(base, Value::Object(_)) {
                return Ok(false);
            }
            (2, 0, 1, Hint::String)
        }
        _ => return Ok(false),
    };
    let ops = state.operands();
    let at = ops
        .len()
        .checked_sub(count)
        .ok_or(Fault::Internal("operand stack underflow"))?;
    if !ops[at + first..at + end]
        .iter()
        .any(|v| matches!(v, Value::Object(_)))
    {
        return Ok(false);
    }
    let operands = ops[at..].to_vec();
    for _ in 0..count {
        state.pop()?;
    }
    assert_no_borrow("starting a conversion frame (growing native storage)");
    state.natives.push(NativeFrame {
        depth: state.depth(),
        base: state.stack_len(),
        pc: state.top().pc.saturating_sub(1),
        awaiting: Awaiting::Nothing,
        state: NativeState::Convert {
            target: ConvertTarget::Instruction(instr),
            operands,
            next: first,
            end,
            hint,
            method: 0,
            method_get: false,
        },
    });
    Ok(true)
}

/// Built-in object operands use the same resumable method lookup and callback
/// protocol as coercing instructions. Forwarding is resolved before entry;
/// only operands needed after argument evaluation are retained in the frame.
pub(crate) fn start_builtin(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
    popped: usize,
) -> Result<bool, Fault> {
    use builtin::Builtin;
    let error = match which {
        Builtin::Error => Some(error::ErrorState::constructor(
            heap,
            state.diag.task,
            args,
            None,
            state.error_stack(state.top().pc.saturating_sub(1)),
        )?),
        Builtin::NativeError(kind) => Some(error::ErrorState::constructor(
            heap,
            state.diag.task,
            args,
            Some(kind),
            state.error_stack(state.top().pc.saturating_sub(1)),
        )?),
        Builtin::ErrorPrototypeToString if !construct => Some(error::ErrorState::to_string(this)?),
        _ => None,
    };
    if let Some(native) = error {
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::Error(native))?;
        return Ok(true);
    }
    if !construct {
        let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
        let native = match which {
            Builtin::JsonStringify => {
                if !matches!(arg(1), Value::Undefined) {
                    return Err(Fault::TypeError(
                        "JSON.stringify replacer is not supported".into(),
                    ));
                }
                if !matches!(arg(2), Value::Undefined) {
                    return Err(Fault::TypeError(
                        "JSON.stringify space is not supported".into(),
                    ));
                }
                Some(NativeState::JsonStringify(Box::new(
                    super::json::Stringifier::new(arg(0)),
                )))
            }
            Builtin::JsonParse => Some(NativeState::Json {
                text: arg(0),
                reviver: arg(1),
                converted: false,
                parser: None,
            }),
            Builtin::StringRaw => Some(NativeState::StringRaw(Box::new(
                string_raw::StringRaw::new(args)?,
            ))),
            Builtin::ArrayPrototypeJoin => Some(NativeState::Join(Box::new(ArrayJoin {
                this: builtin::array_string_receiver(heap, state.diag.task, this)?,
                separator: arg(0),
                stage: JoinStage::Length,
                len: 0,
                next: 0,
                parts: vec![],
                units: 0,
            }))),
            Builtin::ArrayPrototypeToString => Some(NativeState::ArrayToString {
                this: builtin::array_string_receiver(heap, state.diag.task, this)?,
                stage: 0,
            }),
            Builtin::FunctionPrototypeApply | Builtin::ReflectApply => {
                let (callee, receiver, list) = if which == Builtin::ReflectApply {
                    (arg(0), arg(1), arg(2))
                } else {
                    (this.clone(), arg(0), arg(1))
                };
                Some(NativeState::Apply {
                    callee,
                    this: receiver,
                    len: if which == Builtin::FunctionPrototypeApply
                        && matches!(list, Value::Undefined | Value::Null)
                    {
                        Some(0)
                    } else {
                        None
                    },
                    list,
                    args: vec![],
                    max_args: state.forward_limits(state.stack_len() - popped).0,
                    called: false,
                })
            }
            Builtin::ArrayPrototypePush => Some(NativeState::Push {
                this: this.clone(),
                args: args.to_vec(),
                len: None,
                next: 0,
                finishing: false,
            }),
            Builtin::FunctionPrototypeBind => Some(NativeState::Bind {
                target: this.clone(),
                args: args.to_vec(),
                length: Value::Undefined,
                stage: 0,
            }),
            _ => None,
        };
        if let Some(native) = native {
            state.truncate_operands(state.stack_len() - popped)?;
            push_native(state, native)?;
            return Ok(true);
        }
    }
    if which == Builtin::ObjectDefineProperty
        && !construct
        && !matches!(args.get(1), Some(Value::Object(_)))
    {
        let desc = descriptor::DefineProperty::new(args)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::DefineProperty(desc))?;
        return Ok(true);
    }
    let (target, operands, hint) = match which {
        Builtin::NumberPrototypeToString if !construct => {
            // thisNumberValue must reject an invalid receiver before any
            // observable radix conversion, even through call/apply/bind.
            let number = builtin::this_number_value(heap, state.diag.task, this, "toString")?;
            let Some(radix @ Value::Object(_)) = args.first() else {
                return Ok(false);
            };
            (
                ConvertTarget::NumberToString { number },
                vec![radix.clone()],
                Hint::Number,
            )
        }
        Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathAbs
        | Builtin::MathImul
        | Builtin::MathPow
        | Builtin::MathFround
        | Builtin::MathSqrt
        | Builtin::MathLog2
        | Builtin::MathHypot
            if !construct =>
        {
            let count = match which {
                Builtin::MathHypot => args.len(),
                Builtin::MathImul | Builtin::MathPow => args.len().min(2),
                _ => args.len().min(1),
            };
            let operands = &args[..count];
            // hypot always takes the budgeted path, including primitives.
            if which != Builtin::MathHypot
                && !operands.iter().any(|v| matches!(v, Value::Object(_)))
            {
                return Ok(false);
            }
            (
                ConvertTarget::Math {
                    which,
                    magnitude: 0.0,
                },
                operands.to_vec(),
                Hint::Number,
            )
        }
        Builtin::GlobalIsNaN | Builtin::GlobalIsFinite if !construct => {
            let Some(value @ Value::Object(_)) = args.first() else {
                return Ok(false);
            };
            (
                ConvertTarget::NumberPredicate(which),
                vec![value.clone()],
                Hint::Number,
            )
        }
        Builtin::ParseInt | Builtin::ParseFloat if !construct => {
            let integer = which == Builtin::ParseInt;
            let mut operands = vec![args.first().cloned().unwrap_or(Value::Undefined)];
            if integer {
                operands.push(args.get(1).cloned().unwrap_or(Value::Undefined));
            }
            (
                ConvertTarget::NumberParse { integer },
                operands,
                Hint::String,
            )
        }
        Builtin::SymbolFor if !construct => {
            let Some(value @ Value::Object(_)) = args.first() else {
                return Ok(false);
            };
            (
                ConvertTarget::SymbolRegistryKey,
                vec![value.clone()],
                Hint::String,
            )
        }
        Builtin::Number | Builtin::String | Builtin::Symbol => {
            if which == Builtin::Symbol && construct {
                return Ok(false);
            }
            let Some(value @ Value::Object(_)) = args.first() else {
                return Ok(false);
            };
            (
                if which == Builtin::Symbol {
                    ConvertTarget::SymbolDescription
                } else {
                    ConvertTarget::PrimitiveConstructor { which, construct }
                },
                vec![value.clone()],
                if matches!(which, Builtin::String | Builtin::Symbol) {
                    Hint::String
                } else {
                    Hint::Number
                },
            )
        }
        Builtin::ObjectDefineProperty | Builtin::ObjectGetOwnPropertyDescriptor
            if !construct && matches!(args.get(1), Some(Value::Object(_))) =>
        {
            if which == Builtin::ObjectGetOwnPropertyDescriptor
                && matches!(args.first(), None | Some(Value::Null | Value::Undefined))
            {
                return Err(Fault::TypeError(
                    "cannot convert null or undefined to object".into(),
                ));
            }
            if which == Builtin::ObjectDefineProperty
                && !matches!(args.first(), Some(Value::Object(_)))
            {
                return Err(Fault::TypeError(
                    "Object.defineProperty target must be an object".into(),
                ));
            }
            (
                ConvertTarget::PropertyBuiltin { which },
                args.to_vec(),
                Hint::String,
            )
        }
        Builtin::StringPrototypeCharAt
        | Builtin::StringPrototypeCharCodeAt
        | Builtin::StringPrototypeCodePointAt
            if !construct =>
        {
            // RequireObjectCoercible precedes any position conversion. Leave
            // null/undefined to the leaf's TypeError path without callbacks.
            if matches!(this, Value::Null | Value::Undefined) {
                return Ok(false);
            }
            let pos = args.first().cloned().unwrap_or(Value::Undefined);
            if !matches!(this, Value::Object(_)) && !matches!(pos, Value::Object(_)) {
                return Ok(false);
            }
            (
                ConvertTarget::StringBuiltin {
                    which,
                    access: true,
                    concat_len: 0,
                },
                vec![this.clone(), pos],
                Hint::String,
            )
        }
        Builtin::StringPrototypeSplit if !construct => {
            if matches!(this, Value::Null | Value::Undefined) {
                return Err(Fault::TypeError(format!(
                    "String.prototype.split called on {}",
                    this.type_name()
                )));
            }
            let separator = args.first().cloned().unwrap_or(Value::Undefined);
            let limit = args.get(1).cloned().unwrap_or(Value::Undefined);
            (
                ConvertTarget::StringSplit {
                    separator_undefined: matches!(separator, Value::Undefined),
                    limit_undefined: matches!(limit, Value::Undefined),
                },
                vec![this.clone(), limit, separator],
                Hint::String,
            )
        }
        Builtin::StringPrototypeIndexOf | Builtin::StringPrototypeLastIndexOf if !construct => {
            if matches!(this, Value::Null | Value::Undefined) {
                return Err(Fault::TypeError(format!(
                    "String.prototype.{} called on {}",
                    which.name(),
                    this.type_name()
                )));
            }
            // Always retain a native frame, including primitive-only calls:
            // prefix construction and long string scans must be budgeted too.
            (
                ConvertTarget::StringBuiltin {
                    which,
                    access: true,
                    concat_len: 0,
                },
                vec![
                    this.clone(),
                    args.first().cloned().unwrap_or(Value::Undefined),
                    args.get(1).cloned().unwrap_or(Value::Undefined),
                ],
                Hint::String,
            )
        }
        Builtin::StringFromCharCode | Builtin::StringFromCodePoint if !construct => {
            if !args.iter().any(|v| matches!(v, Value::Object(_))) {
                return Ok(false);
            }
            (
                ConvertTarget::StringBuiltin {
                    which,
                    access: false,
                    concat_len: 0,
                },
                args.to_vec(),
                Hint::Number,
            )
        }
        _ if !construct && builtin::is_string_build(which) => {
            if matches!(this, Value::Null | Value::Undefined) {
                return Err(Fault::TypeError(format!(
                    "String.prototype.{} called on {}",
                    which.name(),
                    this.type_name()
                )));
            }
            let mut operands = vec![this.clone()];
            if which == Builtin::StringPrototypeConcat {
                operands.extend_from_slice(args);
            } else {
                operands.push(args.first().cloned().unwrap_or(Value::Undefined));
                // An omitted/undefined end or filler differs from an object
                // whose ToPrimitive returns undefined. Only omit the former.
                if let Some(value) = args.get(1).filter(|v| !matches!(v, Value::Undefined)) {
                    operands.push(value.clone());
                }
            }
            (
                ConvertTarget::StringBuiltin {
                    which,
                    access: true,
                    concat_len: 0,
                },
                operands,
                Hint::String,
            )
        }
        _ => return Ok(false),
    };
    // A built-in callback can start another native at the same call depth.
    // Bound recursion through such callbacks must still have a finite limit.
    let limit = state.forward_limits(state.stack_len() - popped).1;
    if state.natives.len() >= limit {
        return Err(Fault::StackOverflow(format!(
            "maximum native call depth of {limit} frames exceeded"
        )));
    }
    let (first, end) = if matches!(target, ConvertTarget::PropertyBuiltin { .. }) {
        (1, 2)
    } else {
        (0, operands.len())
    };
    let pc = running(state).unwrap_or_else(|| state.top().pc.saturating_sub(1));
    state.truncate_operands(state.stack_len() - popped)?;
    assert_no_borrow("starting a built-in conversion frame (growing native storage)");
    state.natives.push(NativeFrame {
        depth: state.depth(),
        base: state.stack_len(),
        pc,
        awaiting: Awaiting::Nothing,
        state: NativeState::Convert {
            target,
            operands,
            next: first,
            end,
            hint,
            method: 0,
            method_get: false,
        },
    });
    Ok(true)
}

/// Execute the starting instruction `instr` at `pc` of the running frame:
/// validate its operands, pop them into a new native frame and push it. The
/// operation's first step runs next. Allocates nothing.
pub(crate) fn start(
    state: &mut VmState,
    heap: &Heap,
    instr: Instr,
    pc: usize,
) -> Result<(), Fault> {
    let (popped, native) = match instr {
        Instr::Reduce => {
            let [f, init, vec] = operands::<3>(state)?;
            let len = coll::vector_count(heap, vec.clone())?;
            // Checked once, up front, even for an empty vector.
            dispatch::callee(heap, &f)?;
            let native = NativeState::Reduce {
                f,
                vec,
                acc: init,
                next: 0,
                len,
            };
            (3, native)
        }
        Instr::NeedAll => {
            let [defs, kind] = operands::<2>(state)?;
            let len = coll::vector_count(heap, defs.clone())?;
            let kind = result_kind(&kind).ok_or_else(|| {
                Fault::Unsupported(format!(
                    "need_all takes a result kind as a non-negative integer number, got {}",
                    describe(&kind)
                ))
            })?;
            let native = NativeState::NeedAll {
                defs,
                kind,
                out: Value::Undefined,
                next: 0,
                len,
            };
            (2, native)
        }
        _ => return Err(Fault::Internal("not a resumable native instruction")),
    };
    for _ in 0..popped {
        state.pop()?;
    }
    assert_no_borrow("starting a native frame (growing native storage)");
    let frame = NativeFrame {
        depth: state.depth(),
        base: state.stack_len(),
        pc,
        awaiting: Awaiting::Nothing,
        state: native,
    };
    state.natives.push(frame);
    Ok(())
}

/// An exception unwinds to a handler of frame index `frame` (0: the entry
/// frame): deliver the failure to every pending native it passes, innermost
/// first (the natives started by `frame` or by a frame above it), and pop
/// each one that answers `Unwind`, releasing what it held (module docs,
/// "Errors"). An `Err` names a native that refused; the natives not yet
/// passed are left as they were.
pub(crate) fn unwind(natives: &mut Vec<NativeFrame>, frame: usize) -> Result<(), &'static str> {
    // A native started by frame index `i` has depth `i + 1`; depths never
    // decrease along the stack (built-in conversions can share a depth).
    while let Some(native) = natives.last_mut().filter(|n| frame < n.depth) {
        let failure = match native.awaiting {
            Awaiting::CallResult => Failure::Callback,
            Awaiting::Dependency => Failure::Dependency,
            Awaiting::Nothing => Failure::Step,
        };
        let action = {
            let _borrow = scoped_borrow();
            native.state.fail(failure)
        };
        match action {
            Action::Unwind => {
                natives.pop();
            }
            Action::Continue
            | Action::Call { .. }
            | Action::ToNumber(_)
            | Action::ToNumberLimited(_, _)
            | Action::ToString(_)
            | Action::Builtin { .. }
            | Action::Need { .. }
            | Action::Return(_)
            | Action::Resume { .. } => {
                return Err("a native frame did not unwind from a failure");
            }
        }
    }
    Ok(())
}

/// Cancellation unwinds to a finally handler of frame index `frame`: drop
/// every pending native it passes without running any native code (module
/// docs, "Errors", cancellation).
pub(crate) fn discard(natives: &mut Vec<NativeFrame>, frame: usize) {
    let kept = natives.partition_point(|n| n.depth <= frame);
    natives.truncate(kept);
}

/// The starting instruction of the running native frame, if one is running
/// (no call frame above it).
pub(crate) fn running(state: &VmState) -> Option<usize> {
    state
        .natives
        .last()
        .filter(|n| n.depth == state.depth())
        .map(|n| n.pc)
}

/// Run one step of the running native frame and apply its action (module
/// docs, "The protocol").
pub(crate) fn step(state: &mut VmState, heap: &Heap, host: &mut dyn Host) -> Result<Flow, Fault> {
    let depth = state.depth();
    let frame = state
        .natives
        .last()
        .filter(|n| n.depth == depth)
        .ok_or(Fault::Internal("no running native frame"))?;
    let (base, awaiting) = (frame.base, frame.awaiting);
    // The completion is read in place: it stays rooted in the operand area
    // until the step has stored it in the frame.
    let delivered = match awaiting {
        Awaiting::Nothing => None,
        Awaiting::CallResult | Awaiting::Dependency => Some(state.stack_value(base)?),
    };
    let expected = base + usize::from(delivered.is_some());
    if state.stack_len() != expected {
        return Err(Fault::Internal("native operand area out of step"));
    }
    let input = match (awaiting, delivered) {
        (Awaiting::CallResult, Some(v)) => Input::CallResult(v),
        (Awaiting::Dependency, Some(v)) => Input::Dependency(v),
        _ => Input::Start,
    };
    let frame = state
        .natives
        .last_mut()
        .ok_or(Fault::Internal("no running native frame"))?;
    frame.awaiting = Awaiting::Nothing;
    let action = {
        let _borrow = scoped_borrow();
        frame.state.step(heap, state.diag.task, input)?
    };
    state.truncate_operands(base)?;
    match action {
        Action::ToNumber(_) | Action::ToNumberLimited(_, _) | Action::ToString(_) => {
            let (value, target, hint) = match action {
                Action::ToNumber(value) => (value, ConvertTarget::NumberValue, Hint::Number),
                Action::ToNumberLimited(value, limit) => (
                    value,
                    ConvertTarget::NumberValueLimited(limit),
                    Hint::Number,
                ),
                Action::ToString(value) => (value, ConvertTarget::StringValue, Hint::String),
                _ => unreachable!(),
            };
            set_awaiting(state, Awaiting::CallResult)?;
            push_native(
                state,
                NativeState::Convert {
                    target,
                    operands: vec![value],
                    next: 0,
                    end: 1,
                    hint,
                    method: 0,
                    method_get: false,
                },
            )?;
            Ok(Flow::Next)
        }
        Action::Builtin { which, args } => {
            state.natives.pop();
            if !start_builtin(heap, state, which, &Value::Undefined, &args, false, 0)? {
                let result = builtin::call(heap, state.diag.task, which, &Value::Undefined, &args)?;
                state.push(result);
            }
            Ok(Flow::Next)
        }
        Action::Call { callee, this, args } => {
            // A forwarding built-in callback (`Reflect.apply`) is followed
            // to the call it makes (`dispatch::resolve`).
            let (max_args, max_hops) = state.forward_limits(state.stack_len());
            let call = builtin::Forward { callee, this, args };
            let task = state.diag.task;
            let (id, code, call) = match dispatch::resolve(heap, task, call, max_args, max_hops)? {
                (Callee::Bound(_), _) => return Err(Fault::Internal("unresolved bound function")),
                (Callee::Code(id, code), call) => (id, code, call),
                (Callee::Builtin(which), call) => {
                    set_awaiting(state, Awaiting::CallResult)?;
                    if start_builtin(heap, state, which, &call.this, &call.args, false, 0)? {
                        return Ok(Flow::Next);
                    }
                    // Runs to completion (`crate::vm::builtin`); no safepoint
                    // falls between here and the push that roots its result.
                    // The next step receives it like a callback's return.
                    let result = builtin::call(heap, task, which, &call.this, &call.args)?;
                    state.push(result);
                    return Ok(Flow::Next);
                }
            };
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(call.callee);
            let argc = call.args.len();
            call.args.into_iter().for_each(|a| state.push(a));
            state.enter(code, id, call.this, argc)?;
            Ok(Flow::FrameChanged)
        }
        Action::Continue => Ok(Flow::Next),
        Action::Need { dependency, kind } => {
            set_awaiting(state, Awaiting::Dependency)?;
            Ok(Flow::Need(dependency, kind))
        }
        Action::Return(v) => {
            state.natives.pop();
            state.push(v);
            Ok(Flow::Next)
        }
        Action::Resume { instr, operands } => {
            state.natives.pop();
            for v in operands {
                state.push(v);
            }
            let code = std::sync::Arc::clone(&state.top().code);
            dispatch::step(state, &code, heap, host, instr)
        }
        Action::Unwind => Err(Fault::Internal("a native step unwound without a failure")),
    }
}

fn set_awaiting(state: &mut VmState, awaiting: Awaiting) -> Result<(), Fault> {
    let frame = state
        .natives
        .last_mut()
        .ok_or(Fault::Internal("no running native frame"))?;
    frame.awaiting = awaiting;
    Ok(())
}

fn push_native(state: &mut VmState, native: NativeState) -> Result<(), Fault> {
    let limit = state.forward_limits(state.stack_len()).1;
    if state.natives.len() >= limit {
        return Err(Fault::StackOverflow(format!(
            "maximum native call depth of {limit} frames exceeded"
        )));
    }
    let pc = running(state).unwrap_or_else(|| state.top().pc.saturating_sub(1));
    assert_no_borrow("starting a property continuation");
    state.natives.push(NativeFrame {
        depth: state.depth(),
        base: state.stack_len(),
        pc,
        awaiting: Awaiting::Nothing,
        state: native,
    });
    Ok(())
}

fn index_key(index: u64) -> property::Key {
    if index < u64::from(u32::MAX) {
        property::Key::Index(index as u32)
    } else {
        property::Key::Name(JsString::from(index.to_string().as_str()))
    }
}

fn to_length(value: &Value) -> Result<u64, Fault> {
    let number = dispatch::to_number(value)?;
    Ok(if number.is_nan() || number <= 0.0 {
        0
    } else {
        number.trunc().min(((1_u64 << 53) - 1) as f64) as u64
    })
}

pub(crate) fn read_result(
    state: &mut VmState,
    read: Option<property::Read>,
    this: Value,
) -> Result<(), Fault> {
    match read.unwrap_or(property::Read::Value(Value::Undefined)) {
        property::Read::Value(value) => state.push(value),
        property::Read::Getter(callee) => {
            return push_native(
                state,
                NativeState::PropertyCall {
                    callee,
                    this,
                    assigned: None,
                },
            );
        }
    }
    Ok(())
}

pub(crate) fn write_result(
    state: &mut VmState,
    setter: Option<Value>,
    this: Value,
    assigned: Value,
) -> Result<(), Fault> {
    match setter {
        Some(callee) => push_native(
            state,
            NativeState::PropertyCall {
                callee,
                this,
                assigned: Some(assigned),
            },
        ),
        None => {
            state.push(assigned);
            Ok(())
        }
    }
}

pub(crate) fn start_instanceof(state: &mut VmState, heap: &Heap) -> Result<(), Fault> {
    let [value, target] = operands::<2>(state)?;
    let target = prototype::instance_target(heap, &value, &target)?;
    state.pop()?;
    state.pop()?;
    match target {
        Some(target) => push_native(state, NativeState::Instanceof { value, target }),
        None => {
            state.push(Value::Bool(false));
            Ok(())
        }
    }
}

impl NativeState {
    /// One step: consume `input`, return the next action. Anything allocated
    /// is stored in `self` (a root) before returning.
    fn step(&mut self, heap: &Heap, task: TaskId, input: Input) -> Result<Action, Fault> {
        match self {
            NativeState::StringRaw(raw) => {
                if let Some(build) = raw.finish()? {
                    *self = NativeState::StringBuild(build);
                    return Ok(Action::Continue);
                }
                raw.step(heap, task, input)
            }
            NativeState::JsonStringify(stringifier) => {
                use super::json::StringifyStep;
                let result = match input {
                    Input::CallResult(value) => Some(value),
                    _ => None,
                };
                Ok(match stringifier.advance(heap, task, result)? {
                    StringifyStep::Continue => Action::Continue,
                    StringifyStep::Call { callee, this, args } => {
                        Action::Call { callee, this, args }
                    }
                    StringifyStep::ToNumber(value) => {
                        Action::ToNumberLimited(value, super::json::MAX_NUMBER_CONVERSION_UNITS)
                    }
                    StringifyStep::ToString(value) => Action::ToString(value),
                    StringifyStep::Return(value) => Action::Return(value),
                })
            }
            NativeState::Join(join) => {
                if matches!(join.stage, JoinStage::Element) && join.next == join.len {
                    *self = NativeState::StringBuild(
                        crate::value::string::StringBuild::with_length(
                            std::mem::take(&mut join.parts),
                            join.units,
                        )
                        .ok_or_else(|| Fault::RangeError("string too long".into()))?,
                    );
                    return Ok(Action::Continue);
                }
                join.step(heap, task, input)
            }
            NativeState::ArrayToString { this, stage } => {
                let method = if *stage == 0 {
                    *stage = 1;
                    match property::read(
                        heap,
                        task,
                        this,
                        &property::Key::Name(JsString::from("join")),
                    )? {
                        Some(property::Read::Getter(callee)) => {
                            return Ok(Action::Call {
                                callee,
                                this: this.clone(),
                                args: vec![],
                            });
                        }
                        Some(property::Read::Value(value)) => value,
                        None => Value::Undefined,
                    }
                } else {
                    let Input::CallResult(value) = input else {
                        return Err(Fault::Internal("array toString expected call result"));
                    };
                    if *stage == 2 {
                        return Ok(Action::Return(value));
                    }
                    value
                };
                match dispatch::callee(heap, &method) {
                    Ok(_) => {
                        *stage = 2;
                        Ok(Action::Call {
                            callee: method,
                            this: this.clone(),
                            args: vec![],
                        })
                    }
                    Err(Fault::NotCallable(_)) => builtin::call(
                        heap,
                        task,
                        builtin::Builtin::ObjectPrototypeToString,
                        this,
                        &[],
                    )
                    .map(Action::Return),
                    Err(error) => Err(error),
                }
            }
            NativeState::Error(error) => error.step(heap, task, input),
            NativeState::Bind {
                target,
                args,
                length,
                stage,
            } => {
                if *stage == 0 {
                    dispatch::callee(heap, target)?;
                    let Value::Object(id) = target else {
                        unreachable!()
                    };
                    *stage = 1;
                    let key = property::Key::Name(JsString::from("length"));
                    if property::get_own_property(heap, task, *id, &key)?.is_some() {
                        match property::read(heap, task, target, &key)? {
                            Some(property::Read::Getter(callee)) => {
                                return Ok(Action::Call {
                                    callee,
                                    this: target.clone(),
                                    args: vec![],
                                });
                            }
                            Some(property::Read::Value(v)) => *length = v,
                            None => {}
                        }
                    }
                } else if *stage == 1 {
                    let Input::CallResult(v) = input else {
                        return Err(Fault::Internal("bind expected length"));
                    };
                    *length = v;
                } else {
                    let Input::CallResult(name) = input else {
                        return Err(Fault::Internal("bind expected name"));
                    };
                    return super::function::bind_with_metadata(
                        heap, task, target, args, length, &name,
                    )
                    .map(Action::Return);
                }
                *stage = 2;
                let name = match property::read(
                    heap,
                    task,
                    target,
                    &property::Key::Name(JsString::from("name")),
                )? {
                    Some(property::Read::Getter(callee)) => {
                        return Ok(Action::Call {
                            callee,
                            this: target.clone(),
                            args: vec![],
                        });
                    }
                    Some(property::Read::Value(v)) => v,
                    None => Value::Undefined,
                };
                super::function::bind_with_metadata(heap, task, target, args, length, &name)
                    .map(Action::Return)
            }
            NativeState::Apply {
                callee,
                this,
                list,
                args,
                len,
                max_args,
                called,
            } => {
                if *called {
                    let Input::CallResult(v) = input else {
                        return Err(Fault::Internal("apply expected a call result"));
                    };
                    return Ok(Action::Return(v));
                }
                dispatch::callee(heap, callee)?;
                if len.is_none() {
                    if !matches!(list, Value::Object(_)) {
                        return Err(Fault::TypeError(
                            "an argument list must be an object".into(),
                        ));
                    }
                    let value = match input {
                        Input::Start => match property::read(
                            heap,
                            task,
                            list,
                            &property::Key::Name(JsString::from("length")),
                        )? {
                            Some(property::Read::Getter(callee)) => {
                                return Ok(Action::Call {
                                    callee,
                                    this: list.clone(),
                                    args: vec![],
                                });
                            }
                            Some(property::Read::Value(v)) => v,
                            None => Value::Undefined,
                        },
                        Input::CallResult(v) => v,
                        Input::Dependency(_) => {
                            return Err(Fault::Internal("apply received dependency"));
                        }
                    };
                    if matches!(value, Value::Object(_)) {
                        return Ok(Action::ToNumber(value));
                    }
                    let length = to_length(&value)?;
                    if length > *max_args as u64 {
                        return Err(Fault::StackOverflow(format!(
                            "too many arguments: an argument list of length {length} exceeds the limit of {max_args}"
                        )));
                    }
                    *len = Some(length as usize);
                } else if let Input::CallResult(v) = input {
                    args.push(v);
                }
                while args.len() < len.unwrap() {
                    match property::read(heap, task, list, &index_key(args.len() as u64))? {
                        Some(property::Read::Getter(callee)) => {
                            return Ok(Action::Call {
                                callee,
                                this: list.clone(),
                                args: vec![],
                            });
                        }
                        Some(property::Read::Value(v)) => args.push(v),
                        None => args.push(Value::Undefined),
                    }
                }
                *called = true;
                Ok(Action::Call {
                    callee: callee.clone(),
                    this: this.clone(),
                    args: args.clone(),
                })
            }
            NativeState::Push {
                this,
                args,
                len,
                next,
                finishing,
            } => {
                if *finishing {
                    return Ok(Action::Return(Value::Number(
                        (len.unwrap() + args.len() as u64) as f64,
                    )));
                }
                if len.is_none() {
                    if !matches!(this, Value::Object(_)) {
                        return Err(match this {
                            Value::Undefined | Value::Null => Fault::TypeError(format!("cannot convert {} to object", this.type_name())),
                            _ => Fault::Unsupported("Array.prototype.push on a primitive needs a wrapper object, which is not supported".into()),
                        });
                    }
                    let value = match input {
                        Input::Start => match property::read(
                            heap,
                            task,
                            this,
                            &property::Key::Name(JsString::from("length")),
                        )? {
                            Some(property::Read::Getter(callee)) => {
                                return Ok(Action::Call {
                                    callee,
                                    this: this.clone(),
                                    args: vec![],
                                });
                            }
                            Some(property::Read::Value(v)) => v,
                            None => Value::Undefined,
                        },
                        Input::CallResult(v) => v,
                        Input::Dependency(_) => {
                            return Err(Fault::Internal("push received dependency"));
                        }
                    };
                    if matches!(value, Value::Object(_)) {
                        return Ok(Action::ToNumber(value));
                    }
                    let length = to_length(&value)?;
                    if args.len() as u64 > ((1 << 53) - 1) - length {
                        return Err(Fault::TypeError(
                            "array-like length exceeds 2^53 - 1".into(),
                        ));
                    }
                    *len = Some(length);
                }
                while *next < args.len() {
                    let key = index_key(len.unwrap() + *next as u64);
                    let item = args[*next].clone();
                    let setter = property::prepare_put(heap, task, this, &key, item.clone(), true)?;
                    *next += 1;
                    if let Some(callee) = setter {
                        return Ok(Action::Call {
                            callee,
                            this: this.clone(),
                            args: vec![item],
                        });
                    }
                }
                let length = Value::Number((len.unwrap() + args.len() as u64) as f64);
                let setter = property::prepare_put(
                    heap,
                    task,
                    this,
                    &property::Key::Name(JsString::from("length")),
                    length.clone(),
                    true,
                )?;
                *finishing = true;
                match setter {
                    Some(callee) => Ok(Action::Call {
                        callee,
                        this: this.clone(),
                        args: vec![length],
                    }),
                    None => Ok(Action::Return(length)),
                }
            }
            NativeState::Instanceof { value, target } => {
                let proto = match input {
                    Input::Start => match property::read(
                        heap,
                        task,
                        target,
                        &property::Key::Name(JsString::from("prototype")),
                    )? {
                        Some(property::Read::Getter(callee)) => {
                            return Ok(Action::Call {
                                callee,
                                this: target.clone(),
                                args: vec![],
                            });
                        }
                        Some(property::Read::Value(v)) => v,
                        None => Value::Undefined,
                    },
                    Input::CallResult(v) => v,
                    Input::Dependency(_) => {
                        return Err(Fault::Internal("instanceof received a dependency"));
                    }
                };
                prototype::instance_of_prototype(heap, task, value, &proto)
                    .map(|v| Action::Return(Value::Bool(v)))
            }
            NativeState::DefineProperty(desc) => {
                let result = match input {
                    Input::Start => None,
                    Input::CallResult(v) => Some(v),
                    Input::Dependency(_) => {
                        return Err(Fault::Internal("descriptor received a dependency"));
                    }
                };
                match desc.step(heap, task, result)? {
                    descriptor::DefineStep::Getter { callee, this } => Ok(Action::Call {
                        callee,
                        this,
                        args: vec![],
                    }),
                    descriptor::DefineStep::Done(v) => Ok(Action::Return(v)),
                }
            }
            NativeState::PropertyCall {
                callee,
                this,
                assigned,
            } => match input {
                Input::Start => Ok(Action::Call {
                    callee: callee.clone(),
                    this: this.clone(),
                    args: assigned.iter().cloned().collect(),
                }),
                Input::CallResult(v) => Ok(Action::Return(assigned.clone().unwrap_or(v))),
                Input::Dependency(_) => {
                    Err(Fault::Internal("property callback received a dependency"))
                }
            },
            NativeState::Reduce {
                f,
                vec,
                acc,
                next,
                len,
            } => {
                match input {
                    Input::Start => {}
                    Input::CallResult(v) => {
                        *acc = v;
                        *next += 1;
                    }
                    Input::Dependency(_) => {
                        return Err(Fault::Internal("reduce received a dependency"));
                    }
                }
                if *next >= *len {
                    return Ok(Action::Return(acc.clone()));
                }
                let x = coll::nth(heap, vec.clone(), *next)?;
                Ok(Action::Call {
                    callee: f.clone(),
                    this: Value::Undefined,
                    args: vec![acc.clone(), x],
                })
            }
            NativeState::NeedAll {
                defs,
                kind,
                out,
                next,
                len,
            } => {
                match input {
                    Input::Start => *out = coll::new_vector(heap, &[])?,
                    Input::Dependency(v) => {
                        *out = coll::conj(heap, out.clone(), v)?;
                        *next += 1;
                    }
                    Input::CallResult(_) => {
                        return Err(Fault::Internal("need_all received a call result"));
                    }
                }
                if *next >= *len {
                    return Ok(Action::Return(out.clone()));
                }
                let def = coll::nth(heap, defs.clone(), *next)?;
                let dependency = dependency_id(&def).ok_or_else(|| {
                    Fault::Unsupported(format!(
                        "need_all takes definition ids as non-negative integer numbers, got {} at index {next}",
                        describe(&def)
                    ))
                })?;
                Ok(Action::Need {
                    dependency,
                    kind: *kind,
                })
            }
            NativeState::Convert {
                target,
                operands,
                next,
                end,
                hint,
                method,
                method_get,
            } => {
                match input {
                    Input::Start => {}
                    Input::CallResult(v) => {
                        if *method_get {
                            *method_get = false;
                            if dispatch::callee(heap, &v).is_ok() {
                                return Ok(Action::Call {
                                    callee: v,
                                    this: operands[*next].clone(),
                                    args: vec![],
                                });
                            }
                        } else if !matches!(v, Value::Object(_)) {
                            operands[*next] = v;
                            *method = 0;
                        }
                    }
                    Input::Dependency(_) => {
                        return Err(Fault::Internal("conversion received a dependency"));
                    }
                }
                while *next < *end {
                    let receiver = &operands[*next];
                    if !matches!(receiver, Value::Object(_)) {
                        // Numeric binary operators complete ToNumeric on
                        // each operand in order. A symbol on the left must
                        // throw before any right-hand conversion callback.
                        // Plus and comparisons instead need both primitives.
                        if matches!(target, ConvertTarget::Instruction(Instr::ToNumberPair)) {
                            operands[*next] = Value::Number(dispatch::to_number(receiver)?);
                            *next += 1;
                            continue;
                        }
                        if let ConvertTarget::StringSplit {
                            limit_undefined, ..
                        } = target
                        {
                            operands[*next] = if *next == 1 {
                                Value::Number(if *limit_undefined {
                                    u32::MAX as f64
                                } else {
                                    dispatch::to_int32(dispatch::to_number(receiver)?) as u32 as f64
                                })
                            } else {
                                Value::String(dispatch::to_js_string(receiver)?)
                            };
                            *hint = if *next == 0 {
                                Hint::Number
                            } else {
                                Hint::String
                            };
                            *next += 1;
                            continue;
                        }
                        if let ConvertTarget::Math { magnitude, .. } = target {
                            let number = dispatch::to_number(receiver)?;
                            *magnitude = magnitude.max(number.abs());
                            operands[*next] = Value::Number(number);
                            *next += 1;
                            return Ok(Action::Continue);
                        }
                        if let ConvertTarget::NumberParse { .. } = target {
                            operands[*next] = if *next == 0 {
                                Value::String(dispatch::to_js_string(receiver)?)
                            } else {
                                Value::Number(dispatch::to_number(receiver)?)
                            };
                            *hint = Hint::Number;
                        } else if let ConvertTarget::StringBuiltin {
                            which, concat_len, ..
                        } = target
                        {
                            let which = *which;
                            operands[*next] =
                                builtin::string_builtin_operand(which, *next, receiver)?;
                            if which == builtin::Builtin::StringPrototypeConcat {
                                let text = dispatch::to_js_string(&operands[*next])?;
                                *concat_len = concat_len
                                    .checked_add(text.len())
                                    .filter(|n| *n <= crate::value::string::MAX_STRING_LEN)
                                    .ok_or_else(|| {
                                        Fault::RangeError("string too long".to_string())
                                    })?;
                            }
                            *hint = if builtin::string_operand_is_text(which, *next + 1) {
                                Hint::String
                            } else {
                                Hint::Number
                            };
                            if *next == 1
                                && matches!(
                                    which,
                                    builtin::Builtin::StringPrototypePadStart
                                        | builtin::Builtin::StringPrototypePadEnd
                                )
                            {
                                let length = dispatch::to_number(&operands[1])?;
                                let text = dispatch::to_js_string(&operands[0])?;
                                if length.is_nan() || length.trunc() <= text.len() as f64 {
                                    // A short target never observes the filler.
                                    *end = 2;
                                }
                            }
                        }
                        *next += 1;
                        continue;
                    }
                    while *method < 2 {
                        let names = if matches!(hint, Hint::String) {
                            ["toString", "valueOf"]
                        } else {
                            ["valueOf", "toString"]
                        };
                        let name = JsString::from(names[*method]);
                        *method += 1;
                        // Lookup the second method only after the first has
                        // returned an object: its callback may change it.
                        let callee = match property::conversion_method(heap, task, receiver, &name)?
                        {
                            property::Read::Value(v) => v,
                            property::Read::Getter(callee) => {
                                *method_get = true;
                                return Ok(Action::Call {
                                    callee,
                                    this: receiver.clone(),
                                    args: vec![],
                                });
                            }
                        };
                        match dispatch::callee(heap, &callee) {
                            Ok(_) => {
                                return Ok(Action::Call {
                                    callee,
                                    this: receiver.clone(),
                                    args: vec![],
                                });
                            }
                            Err(Fault::NotCallable(_)) => {}
                            Err(e) => return Err(e),
                        }
                    }
                    return Err(Fault::TypeError(
                        "cannot convert object to primitive value".to_string(),
                    ));
                }
                match *target {
                    ConvertTarget::SymbolRegistryKey => {
                        let key = dispatch::to_js_string(&operands[0])?;
                        Ok(Action::Return(Value::Symbol(heap.symbols.symbol_for(key))))
                    }
                    ConvertTarget::SymbolDescription => Ok(Action::Return(Value::Symbol(
                        crate::value::JsSymbol::new(Some(dispatch::to_js_string(&operands[0])?)),
                    ))),
                    ConvertTarget::StringSplit {
                        separator_undefined,
                        ..
                    } => {
                        let text = dispatch::to_js_string(&operands[0])?;
                        let remaining = dispatch::to_number(&operands[1])? as u32;
                        let separator = if separator_undefined {
                            None
                        } else {
                            Some(dispatch::to_js_string(&operands[2])?)
                        };
                        *self = NativeState::StringSplit {
                            split: Box::new(crate::value::string::StringSplit::new(
                                text, separator,
                            )),
                            out: property::new_array(heap, task)?,
                            remaining,
                        };
                        Ok(Action::Continue)
                    }
                    ConvertTarget::NumberParse { integer } => {
                        let text = dispatch::to_js_string(&operands[0])?;
                        let radix = if integer {
                            let radix = dispatch::to_int32(dispatch::to_number(&operands[1])?);
                            if radix != 0 && !(2..=36).contains(&radix) {
                                return Ok(Action::Return(Value::Number(f64::NAN)));
                            }
                            Some(radix as u32)
                        } else {
                            None
                        };
                        *self = NativeState::NumberParse(Box::new(
                            crate::value::string::NumberParse::new(text, radix),
                        ));
                        Ok(Action::Continue)
                    }
                    ConvertTarget::NumberValue | ConvertTarget::NumberValueLimited(_) => {
                        if let ConvertTarget::NumberValueLimited(limit) = target
                            && let Value::String(text) = &operands[0]
                            && text.len() > *limit
                        {
                            return Err(Fault::RangeError(format!(
                                "JSON.stringify Number conversion limit of {limit} UTF-16 units exceeded"
                            )));
                        }
                        dispatch::to_number(&operands[0]).map(|n| Action::Return(Value::Number(n)))
                    }
                    ConvertTarget::StringValue => dispatch::to_js_string(&operands[0])
                        .map(|s| Action::Return(Value::String(s))),
                    ConvertTarget::PropertyBuiltin { which } => Ok(Action::Builtin {
                        which,
                        args: operands.clone(),
                    }),
                    ConvertTarget::Math {
                        which: builtin::Builtin::MathHypot,
                        magnitude,
                    } => {
                        *self = NativeState::MathHypot {
                            operands: std::mem::take(operands),
                            next: 0,
                            magnitude,
                            sum: 0.0,
                            correction: 0.0,
                        };
                        Ok(Action::Continue)
                    }
                    ConvertTarget::Math { which, .. } => {
                        builtin::math(which, operands).map(Action::Return)
                    }
                    ConvertTarget::Instruction(instr) => Ok(Action::Resume {
                        instr,
                        operands: operands.clone(),
                    }),
                    ConvertTarget::PrimitiveConstructor { which, construct } => {
                        // String's descriptive-symbol exception applies only
                        // to an original symbol argument, never the result of
                        // an object's ToPrimitive.
                        if which == builtin::Builtin::String {
                            operands[0] = Value::String(dispatch::to_js_string(&operands[0])?);
                        }
                        builtin::primitive_constructor(heap, task, which, operands, construct)
                            .map(Action::Return)
                    }
                    ConvertTarget::NumberPredicate(which) => {
                        builtin::number_predicate(which, &operands[0]).map(Action::Return)
                    }
                    ConvertTarget::NumberToString { number } => {
                        // An object whose primitive is undefined converts to
                        // NaN; only an originally undefined radix means 10.
                        let radix = Value::Number(dispatch::to_number(&operands[0])?);
                        builtin::number_to_string(number, &radix).map(Action::Return)
                    }
                    ConvertTarget::StringBuiltin { which, access, .. } => {
                        if builtin::is_string_build(which) {
                            *self = NativeState::StringBuild(builtin::string_build(
                                which,
                                &operands[..*end],
                            )?);
                            return Ok(Action::Continue);
                        }
                        if matches!(
                            which,
                            builtin::Builtin::StringPrototypeIndexOf
                                | builtin::Builtin::StringPrototypeLastIndexOf
                        ) {
                            let text = dispatch::to_js_string(&operands[0])?;
                            let search = dispatch::to_js_string(&operands[1])?;
                            let last = which == builtin::Builtin::StringPrototypeLastIndexOf;
                            let mut position = dispatch::to_number(&operands[2])?;
                            if position.is_nan() {
                                position = if last { f64::INFINITY } else { 0.0 };
                            }
                            let position = position.trunc().clamp(0.0, text.len() as f64) as usize;
                            *self =
                                NativeState::StringSearch(crate::value::string::StringSearch::new(
                                    text, search, position, last,
                                ));
                            return Ok(Action::Continue);
                        }
                        let (this, args) = if access {
                            (&operands[0], &operands[1..])
                        } else {
                            (&Value::Undefined, &operands[..])
                        };
                        builtin::call(heap, task, which, this, args).map(Action::Return)
                    }
                }
            }
            NativeState::MathHypot {
                operands,
                next,
                magnitude,
                sum,
                correction,
            } => {
                if magnitude.is_infinite() {
                    return Ok(Action::Return(Value::Number(f64::INFINITY)));
                }
                if let Some(value) = operands.get(*next) {
                    let number = dispatch::to_number(value)?;
                    if number != 0.0 {
                        let scaled = number / *magnitude;
                        let term = scaled * scaled - *correction;
                        let total = *sum + term;
                        *correction = (total - *sum) - term;
                        *sum = total;
                    }
                    *next += 1;
                    Ok(Action::Continue)
                } else {
                    Ok(Action::Return(Value::Number(*magnitude * sum.sqrt())))
                }
            }
            NativeState::StringSplit {
                split,
                out,
                remaining,
            } => {
                use crate::value::string::SplitStep;
                if *remaining == 0 {
                    return Ok(Action::Return(out.clone()));
                }
                match split
                    .advance()
                    .ok_or(Fault::OutOfMemory("split string allocation failed"))?
                {
                    SplitStep::Continue => Ok(Action::Continue),
                    SplitStep::Done => Ok(Action::Return(out.clone())),
                    SplitStep::Part(part) => {
                        property::append(heap, task, out, Some(Value::String(part)))?;
                        *remaining -= 1;
                        Ok(Action::Continue)
                    }
                }
            }
            NativeState::StringSearch(search) => Ok(match search.advance(256) {
                Some(index) => Action::Return(Value::Number(index as f64)),
                None => Action::Continue,
            }),
            NativeState::StringBuild(build) => Ok(match build.advance(256) {
                Some(text) => Action::Return(Value::String(text)),
                None => Action::Continue,
            }),
            NativeState::Json {
                text,
                reviver,
                converted,
                parser,
            } => {
                if !*converted {
                    *converted = true;
                    return Ok(Action::ToString(text.clone()));
                }
                if parser.is_none() {
                    let Input::CallResult(Value::String(input)) = input else {
                        return Err(Fault::Internal("JSON.parse expected ToString result"));
                    };
                    *text = Value::Undefined;
                    *parser = Some(Box::new(super::json::Parser::new(input)));
                }
                match parser.as_mut().unwrap().advance(heap, task)? {
                    Some(value) => {
                        if dispatch::callee(heap, reviver).is_ok() {
                            return Err(Fault::TypeError(
                                "JSON.parse callable reviver is not supported".to_string(),
                            ));
                        }
                        Ok(Action::Return(value))
                    }
                    None => Ok(Action::Continue),
                }
            }
            NativeState::NumberParse(parse) => Ok(match parse.advance(256) {
                Some(number) => Action::Return(Value::Number(number)),
                None => Action::Continue,
            }),
        }
    }

    /// Receive a failure an exception carries through this native and
    /// answer it (module docs, "Errors"). Neither operation recovers: the
    /// fold's accumulator and `needAll`'s results so far are abandoned with
    /// the frame, and the exception goes on to the caller's handler, which
    /// is what JavaScript's `Array.prototype.reduce` does with a throwing
    /// callback. Allocates nothing and touches no heap object.
    fn fail(&mut self, failure: Failure) -> Action {
        match (self, failure) {
            (
                NativeState::Reduce { .. }
                | NativeState::Error(_)
                | NativeState::PropertyCall { .. }
                | NativeState::DefineProperty(_)
                | NativeState::Instanceof { .. }
                | NativeState::Apply { .. }
                | NativeState::Push { .. }
                | NativeState::Join(_)
                | NativeState::StringRaw(_)
                | NativeState::ArrayToString { .. }
                | NativeState::Bind { .. }
                | NativeState::NeedAll { .. }
                | NativeState::Convert { .. }
                | NativeState::MathHypot { .. }
                | NativeState::StringSearch(_)
                | NativeState::StringSplit { .. }
                | NativeState::StringBuild(_)
                | NativeState::NumberParse(_)
                | NativeState::Json { .. }
                | NativeState::JsonStringify(_),
                Failure::Callback | Failure::Dependency | Failure::Step,
            ) => Action::Unwind,
        }
    }
}

#[cfg(debug_assertions)]
thread_local! {
    /// Scoped borrows held by native code on this thread (debug builds).
    static BORROWS: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// A scoped borrow held by native code (module docs, "Safepoint contract
/// and debug borrow checks"): while one is alive on a thread, every
/// operation that may reenter, collect, grow storage or suspend panics in
/// debug builds. Bound to its thread; released on drop, also on unwind.
#[must_use = "a scoped borrow is released when dropped"]
pub(crate) struct ScopedBorrow {
    _thread_bound: PhantomData<*const ()>,
}

/// Take a scoped borrow for the current thread (counted in debug builds).
pub(crate) fn scoped_borrow() -> ScopedBorrow {
    #[cfg(debug_assertions)]
    BORROWS.with(|b| b.set(b.get() + 1));
    ScopedBorrow {
        _thread_bound: PhantomData,
    }
}

impl Drop for ScopedBorrow {
    fn drop(&mut self) {
        #[cfg(debug_assertions)]
        BORROWS.with(|b| b.set(b.get().saturating_sub(1)));
    }
}

/// Reject (debug builds: panic) `operation`, one that may reenter, collect,
/// grow storage or suspend, if the current thread holds a scoped borrow: a
/// violation of the native safepoint contract. Free in release builds.
#[inline]
#[track_caller]
pub(crate) fn assert_no_borrow(operation: &'static str) {
    #[cfg(debug_assertions)]
    {
        let held = BORROWS.with(std::cell::Cell::get);
        assert!(
            held == 0,
            "native safepoint contract violated: {operation} while holding {held} scoped \
             borrow(s) (crate::vm::native, \"Safepoint contract and debug borrow checks\")"
        );
    }
    #[cfg(not(debug_assertions))]
    let _ = operation;
}

/// Early VM tests (hand-assembled bytecode, not JavaScript source support)
/// of the leaf/resumable split; the embedding-level tests, from source, are
/// in `tests/native_fold.rs`.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::NoHost;
    use crate::ids::{TaskId, WorldId};
    use crate::value::Primitive;
    use crate::vm::dispatch::{Exit, execute};
    use crate::vm::frames::{DiagnosticContext, VmLimits};
    use crate::vm::{Assembler, Executable, Opcode};

    #[test]
    fn join_overflow_is_checked_before_the_next_getter() {
        // Model a near-limit prefix without allocating a gigabyte. The
        // separator overflows before index 1 can be read or converted.
        let heap = Heap::default();
        let task = TaskId::from_raw(156);
        let mut join = ArrayJoin {
            this: Value::Null,
            separator: Value::String(JsString::from("ab")),
            stage: JoinStage::Element,
            len: 2,
            next: 1,
            parts: vec![],
            units: crate::value::string::MAX_STRING_LEN - 1,
        };
        assert!(
            matches!(join.step(&heap, task, Input::Start), Err(Fault::RangeError(message)) if message == "string too long")
        );
        join.units = crate::value::string::MAX_STRING_LEN;
        assert!(join.append(JsString::from("")).is_ok());
        assert!(
            matches!(join.append(JsString::from("x")), Err(Fault::RangeError(message)) if message == "string too long")
        );
    }

    /// Seed an already converted near-limit prefix instead of allocating a
    /// gigabyte string. The later object's toString must not run on overflow.
    #[test]
    fn concat_overflow_precedes_later_coercion() {
        let heap = Heap::default();
        let task = TaskId::from_raw(145);
        let prototype = crate::realm::intrinsics(&heap).unwrap().object_prototype;
        let later = Value::Object(
            heap.alloc(crate::heap::HeapObject::Plain(
                crate::object::OrdinaryObject::with_prototype(task, prototype),
            ))
            .unwrap(),
        );
        let pending = |concat_len| NativeState::Convert {
            method_get: false,
            target: ConvertTarget::StringBuiltin {
                which: builtin::Builtin::StringPrototypeConcat,
                access: true,
                concat_len,
            },
            operands: vec![
                Value::String(JsString::from("")),
                Value::String(JsString::from("ab")),
                later.clone(),
            ],
            next: 1,
            end: 3,
            hint: Hint::String,
            method: 0,
        };
        let limit = crate::value::string::MAX_STRING_LEN;
        assert!(matches!(pending(limit - 1).step(&heap, task, Input::Start),
            Err(Fault::RangeError(message)) if message == "string too long"));
        // The same continuation below the limit really does request the
        // later conversion callback, so the overflow assertion checks order.
        assert!(matches!(pending(limit - 3).step(&heap, task, Input::Start),
            Ok(Action::Call { this, .. }) if this == later));
    }

    fn vector(heap: &Heap, n: usize) -> Value {
        let items: Vec<Value> = (1..=n).map(|i| Value::Number(i as f64)).collect();
        coll::new_vector(heap, &items).unwrap()
    }

    /// Run `code(vec)` with a budget of `budget` units; the result is `None`
    /// if it yielded.
    fn run(code: &Executable, heap: &Heap, vec: Value, budget: u64) -> (Option<Value>, VmState) {
        let diag = DiagnosticContext {
            task: TaskId::from_raw(1),
            world: WorldId::INITIAL,
        };
        let mut s = VmState::new(&code.0, &[vec], diag, VmLimits::DEFAULT);
        match execute(&mut s, heap, &mut NoHost, Some(budget)) {
            Exit::Return(v) => (Some(v), s),
            Exit::Yield => (None, s),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn only_reduce_and_need_all_are_resumable() {
        let leaves = [
            Instr::Count,
            Instr::Nth,
            Instr::Conj,
            Instr::Freeze,
            Instr::AtomCas,
            Instr::HostEffect(0),
            Instr::Need,
            Instr::Call(2),
        ];
        assert!(leaves.iter().all(|&i| !is_resumable(i)));
        assert!(is_resumable(Instr::Reduce) && is_resumable(Instr::NeedAll));
    }

    /// A leaf intrinsic is one instruction: one budget unit, no native frame.
    #[test]
    fn a_leaf_native_runs_inside_its_instruction() {
        let heap = Heap::default();
        let code = Assembler::new()
            .get_local(0)
            .op(Opcode::Count)
            .op(Opcode::Return)
            .finish("count", 1, 1)
            .unwrap();
        let vec = vector(&heap, 1000);
        let (done, s) = run(&code, &heap, vec.clone(), 2);
        assert_eq!(done, None);
        assert!(s.natives.is_empty());
        assert_eq!(s.operands(), [Value::Number(1000.0)], "count already ran");
        assert_eq!(
            run(&code, &heap, vec.clone(), 3).0,
            Some(Value::Number(1000.0))
        );
    }

    /// `reduce(add, 0, vec)` costs its instruction, one step per element plus
    /// the final one, and each callback's instructions; between them the
    /// native frame is the task's own state.
    #[test]
    fn a_resumable_native_is_driven_step_by_step() {
        let heap = Heap::default();
        let add = Assembler::new()
            .get_local(0)
            .get_local(1)
            .op(Opcode::Add)
            .op(Opcode::Return)
            .finish("add", 2, 2)
            .unwrap();
        let mut asm = Assembler::new();
        asm.function(&add)
            .constant(Primitive::Number(0.0))
            .get_local(0)
            .op(Opcode::Reduce)
            .op(Opcode::Return);
        let code = asm.finish("sum", 1, 1).unwrap();
        let vec = vector(&heap, 3);
        // 4 instructions to the reduce, 4 steps, 3 callbacks of 4, 1 return.
        let total = 4 + 4 + 3 * 4 + 1;
        assert_eq!(run(&code, &heap, vec.clone(), total - 1).0, None);
        assert_eq!(
            run(&code, &heap, vec.clone(), total).0,
            Some(Value::Number(6.0))
        );

        // After the instruction and the first step: inside the first
        // callback, above the native frame.
        let (_, s) = run(&code, &heap, vec.clone(), 5);
        assert_eq!((s.natives.len(), s.depth()), (1, 2));
        assert_eq!(running(&s), None, "the callback runs, not the native");
        // After the first callback returned: the native is running again,
        // with the result delivered to its operand area.
        let (_, s) = run(&code, &heap, vec.clone(), 9);
        assert_eq!((s.natives.len(), s.depth()), (1, 1));
        assert_eq!(running(&s), Some(3), "started by instruction 3");
        assert_eq!(s.operands(), [Value::Number(1.0)]);
        // The fold's frame holds the vector (and the callback).
        let mut traced = Vec::new();
        s.natives[0].trace(&mut |id| traced.push(id));
        let Value::Object(v) = vec else {
            panic!("a vector is an object")
        };
        assert!(traced.contains(&v), "{traced:?}");
    }

    /// Run `f` and return its panic message (it must panic).
    #[cfg(debug_assertions)]
    fn violation(f: impl FnOnce()) -> String {
        let payload = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
            .expect_err("the operation is rejected");
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| payload.downcast_ref::<&str>().map(|s| s.to_string()))
            .unwrap_or_default()
    }

    /// The safepoint contract is enforced in debug builds: while native code
    /// holds a scoped borrow, reentering the VM, growing VM storage, a
    /// safepoint and suspending are each rejected with a panic naming the
    /// violation; the borrow is released on unwind, and the trampoline's own
    /// borrows never leak past a step.
    #[cfg(debug_assertions)]
    #[test]
    fn scoped_borrows_reject_reentry_growth_and_suspension_in_debug_builds() {
        let heap = Heap::default();
        let code = Assembler::new()
            .get_local(0)
            .op(Opcode::Return)
            .finish("id", 1, 1)
            .unwrap();
        let vec = vector(&heap, 2);
        let diag = DiagnosticContext {
            task: TaskId::from_raw(1),
            world: WorldId::INITIAL,
        };
        let fresh = || VmState::new(&code.0, std::slice::from_ref(&vec), diag, VmLimits::DEFAULT);
        let expect = |message: String, operation: &str| {
            assert!(
                message.contains("native safepoint contract violated")
                    && message.contains(operation),
                "{message}"
            );
        };

        // Reentry: the dispatch loop, and a call frame.
        let mut s = fresh();
        expect(
            violation(|| {
                let _b = scoped_borrow();
                execute(&mut s, &heap, &mut NoHost, None);
            }),
            "reentering the dispatch loop",
        );
        let mut s = fresh();
        let callee = coll::new_vector(&heap, &[]).unwrap();
        let Value::Object(id) = callee else {
            panic!("a vector is an object")
        };
        s.push(callee);
        expect(
            violation(|| {
                let _b = scoped_borrow();
                let _ = s.enter(std::sync::Arc::clone(&code.0), id, Value::Undefined, 0);
            }),
            "entering a call frame",
        );
        // Storage growth.
        expect(
            violation(|| {
                let _b = scoped_borrow();
                s.push(Value::Null);
            }),
            "pushing a value",
        );
        // A safepoint reached directly through the heap (as a step holding
        // `&Heap` could), whether or not a collection is requested.
        expect(
            violation(|| {
                let _b = scoped_borrow();
                heap.safepoint(&Value::Undefined);
            }),
            "reaching a VM safepoint",
        );
        // Released on unwind: the loop runs normally again.
        assert_no_borrow("after the violations");
        let mut s = fresh();
        assert!(matches!(
            execute(&mut s, &heap, &mut NoHost, None),
            Exit::Return(_)
        ));

        // Suspension and reentry through the embedding API.
        let rt = crate::Runtime::new();
        let exe =
            crate::frontend::compile_function("function f() { return __rt.need(1, 1); }").unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        let mut guard = task.try_begin_run().unwrap();
        let pending = guard.need(DependencyId::new(1), ResultKind::new(1));
        assert!(
            matches!(pending, Ok(crate::Need::Waiting { .. })),
            "{pending:?}"
        );
        expect(
            violation(|| {
                let _b = scoped_borrow();
                guard.park();
            }),
            "parking the task",
        );
        expect(
            violation(|| {
                let _b = scoped_borrow();
                let _ = task.try_begin_run();
            }),
            "starting a task run",
        );

        // A fold's steps and failure deliveries take and release borrows.
        let add = Assembler::new()
            .get_local(0)
            .get_local(1)
            .op(Opcode::Add)
            .op(Opcode::Return)
            .finish("add", 2, 2)
            .unwrap();
        let mut asm = Assembler::new();
        asm.function(&add)
            .constant(Primitive::Number(0.0))
            .get_local(0)
            .op(Opcode::Reduce)
            .op(Opcode::Return);
        let sum = asm.finish("sum", 1, 1).unwrap();
        assert_eq!(
            run(&sum, &heap, vec.clone(), 100).0,
            Some(Value::Number(3.0))
        );
        assert_no_borrow("after a fold");
    }

    /// A failure is delivered to each native it passes, innermost first, and
    /// each answers `Unwind`; natives started below the handler's frame are
    /// kept. Cancellation drops the same natives without delivering anything.
    #[test]
    fn unwinding_delivers_failures_and_pops_only_the_natives_passed() {
        let native = |depth, awaiting| NativeFrame {
            depth,
            base: 0,
            pc: 0,
            awaiting,
            state: NativeState::Reduce {
                f: Value::Undefined,
                vec: Value::Undefined,
                acc: Value::Undefined,
                next: 0,
                len: 0,
            },
        };
        let stack = || {
            vec![
                native(1, Awaiting::CallResult),
                native(3, Awaiting::CallResult),
                native(4, Awaiting::Dependency),
            ]
        };
        let depths = |n: &[NativeFrame]| n.iter().map(|n| n.depth).collect::<Vec<_>>();
        for (frame, kept) in [
            (4, vec![1, 3, 4]),
            (3, vec![1, 3]),
            (1, vec![1]),
            (0, vec![]),
        ] {
            let mut natives = stack();
            unwind(&mut natives, frame).unwrap();
            assert_eq!(depths(&natives), kept, "raise to frame {frame}");
            let mut natives = stack();
            discard(&mut natives, frame);
            assert_eq!(depths(&natives), kept, "cancel to frame {frame}");
        }
        let mut reduce = stack().pop().unwrap().state;
        for failure in [Failure::Callback, Failure::Dependency, Failure::Step] {
            assert!(matches!(reduce.fail(failure), Action::Unwind));
        }
    }
}
