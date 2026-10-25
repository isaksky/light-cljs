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
//! partially unwound. Promise callback boundaries instead install an explicit
//! `NativeCatch` handler before the call: after inner JS handlers unwind, it
//! preserves that native and delivers the thrown value to its rejection
//! continuation. Cancellation skips these handlers without running native code.
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
mod dispose;
mod intl;
pub(crate) mod iterator;
pub(crate) mod regexp;

mod add_entries;
mod array_callbacks;
mod array_copy;
mod array_from;
mod array_mutate;
mod array_search;
mod array_sort;
mod collator;
mod construct;
mod date_time_format;
mod display_names;
mod duration_format;
mod eval;
mod intl_service;
mod list_format;
mod number_format;
mod plural_rules;
mod relative_time_format;
pub(crate) mod segmenter;
mod string_methods;
mod string_raw;
mod string_replace;
mod sum_precise;
mod typed_array;

use super::builtin;
use super::dispatch::{self, Callee, Fault, Flow, dependency_id, describe, result_kind};
use super::frames::VmState;
use super::instr::Instr;
use crate::collections as coll;
use crate::heap::gc::Trace;
use crate::heap::{Heap, HeapObject, ObjectId};
use crate::host::Host;
use crate::ids::{DependencyId, ResultKind, TaskId};
use crate::value::{JsString, Value};
use crate::vm::{descriptor, property, prototype};

mod copy_properties;
pub(crate) mod error;
mod object_statics;

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
pub(crate) enum Input {
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
pub(crate) enum Action {
    /// Execute a property instruction and deliver its result to this native.
    Instruction {
        instr: Instr,
        operands: Vec<Value>,
    },
    /// Enter newly compiled eval code after releasing the native borrow.
    Eval(super::executable::Executable, Vec<Value>),
    DynamicFunction(super::executable::Executable),
    SimTimer {
        callback: Value,
        delay: f64,
        clear: bool,
    },
    /// Keep this native frame runnable for another budgeted step.
    Continue,
    /// Complete a property write through the same resumable conversion and
    /// setter protocol as bytecode writes, then deliver its original RHS.
    SetProperty {
        this: Value,
        key: property::Key,
        value: Value,
        strict: bool,
    },
    /// Create an own data property through DefineElem's resumable typed-array
    /// conversion path; inherited setters do not participate.
    CreateDataProperty {
        this: Value,
        key: Value,
        value: Value,
    },
    /// Apply a converted descriptor through the existing typed-array and
    /// property-definition continuation, returning to the parent native.
    DefineProperty(Box<descriptor::DefineProperty>),

    ToNumber(Value),
    ToPrimitiveNumber(Value),
    DefaultLocale,
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
    /// Construct through the ordinary bytecode path and deliver its result
    /// back to this native, retaining the native across constructor waits.
    Construct {
        callee: Value,
        args: Vec<Value>,
    },
    InitializeBuiltin {
        which: builtin::Builtin,
        args: Vec<Value>,
        prototype: Option<ObjectId>,
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
    /// Async resource validation returns the snapshotted method and hint.
    ReturnPair(Value, Value),
    /// Await on the current async activation, retaining this native frame.
    Await(Value),
    /// The answer to a [`Failure`]: release the frame and let the exception
    /// unwind on to the handler below it. Never the result of a step.
    Unwind,
    SuspendAsync(ObjectId),
    ResumeAsync {
        id: ObjectId,
        rejected: bool,
        value: Value,
    },
    ResumeGenerator {
        id: ObjectId,
        value: Value,
        kind: super::generator::ResumeKind,
    },
    AwaitGeneratorReturn {
        value: Value,
    },
}

/// ToPrimitive's preferred primitive type. Number and default share ordinary
/// method order; Date's default conversion fails explicitly until formatting
/// and its Symbol.toPrimitive protocol are implemented.
#[derive(Clone, Copy, Debug)]
enum Hint {
    Default,
    Number,
    String,
}

/// Symbol wrappers and unary property-key conversion consult @@toPrimitive
/// before the ordinary method pair. Other general conversions remain deferred.
#[derive(Clone, Copy, Debug)]
enum SymbolMethod {
    Pending,
    Ordinary,
    Getter,
    Call,
}

fn call_symbol_method(
    heap: &Heap,
    stage: &mut SymbolMethod,
    callee: Value,
    receiver: Value,
    hint: Hint,
) -> Result<Action, Fault> {
    if matches!(callee, Value::Null | Value::Undefined) {
        *stage = SymbolMethod::Ordinary;
        return Ok(Action::Continue);
    }
    dispatch::callee(heap, &callee)?;
    *stage = SymbolMethod::Call;
    Ok(Action::Call {
        callee,
        this: receiver,
        args: vec![Value::String(JsString::from(match hint {
            Hint::Default => "default",
            Hint::Number => "number",
            Hint::String => "string",
        }))],
    })
}

/// What to finish after OrdinaryToPrimitive has converted its operands.
#[derive(Clone, Copy, Debug)]
enum ConvertTarget {
    SimTimer {
        clear: bool,
    },
    TypedArray {
        kind: crate::object::typed_array::ElementType,
        from_buffer: bool,
    },
    TypedArrayWrite,
    ArrayBuffer(builtin::Builtin),
    SymbolDescription,
    SymbolRegistryKey,
    StringSplit {
        separator_undefined: bool,
        limit_undefined: bool,
    },
    NumberValue,
    PrimitiveValue,
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
    NumberToFixed {
        number: f64,
    },
    NumberToPrecisionFormat {
        number: f64,
        which: builtin::Builtin,
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
    DateConstructor,
    DateLocalConstructor,
    DateNow {
        construct: bool,
    },
    DateLocalGet {
        field: u8,
    },
    DateSetLocalField {
        first: u8,
        time: f64,
    },
    DateUtc,
    DateSetTime,
    DateSetUtcField {
        first: u8,
        time: f64,
    },

    BigIntToString,
    DateFunction,
    DateLocalString {
        part: u8,
    },
    BigIntAsN {
        signed: bool,
    },

    DateParse,
}

/// The operation-specific state of a native frame.
#[derive(Debug)]
enum NativeState {
    BuiltinConstruct(Box<construct::State>),
    CopyProperties(copy_properties::State),
    Rest {
        values: Vec<Value>,
        next: usize,
        out: Value,
    },
    AsyncGenerator(Box<super::async_generator::Boundary>),
    Proxy(Box<super::proxy::State>),
    Dispose(dispose::State),
    Disposal(Box<dispose::Stack>),
    MathSumPrecise(Box<sum_precise::State>),
    Async(super::asynchronous::Boundary),
    Eval(Box<eval::State>),
    DynamicFunction(Box<eval::DynamicFunction>),
    ArrayFrom(Box<array_from::State>),
    DateParse {
        parser: Box<super::date::Parser>,
        construct: bool,
    },
    Generator {
        id: ObjectId,
        failed: Option<super::exception::Exception>,
    },
    BigIntFormat(Box<crate::value::bigint::BigIntToString>),
    Iterator(Box<iterator::State>),
    BigIntParse {
        parser: Box<crate::value::bigint::StringToBigInt>,
        instr: Option<Instr>,
        operands: Vec<Value>,
        index: usize,
        wrap: Option<(u64, bool)>,
    },
    Promise(super::promise::PromiseState),
    StringSplit {
        split: Box<crate::value::string::StringSplit>,
        out: Value,
        remaining: u32,
    },
    Error(Box<error::ErrorState>),
    RegExp(regexp::State),
    StringReplace(Box<string_replace::State>),
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
        formatter: Option<Box<crate::value::bigint::BigIntToString>>,
        // Immutable string/cursor storage only; operands retain all heap edges.
        number_scan: Option<Box<crate::value::string::StringToNumber>>,
        target: ConvertTarget,
        operands: Vec<Value>,
        next: usize,
        end: usize,
        hint: Hint,
        method: usize,
        method_get: bool,
        symbol_method: SymbolMethod,
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
    Intl(Box<intl::Intl>),
    NumberFormat(Box<number_format::State>),
    PluralRules(Box<plural_rules::State>),
    RelativeTimeFormat(Box<relative_time_format::State>),
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
    TypedArray(Box<typed_array::TypedArray>),
    ArraySort(Box<array_sort::State>),
    Collator(Box<collator::State>),
    DateTimeFormat(Box<date_time_format::State>),
    ListFormat(Box<list_format::State>),
    Segmenter(Box<segmenter::State>),
    DisplayNames(Box<display_names::State>),
    DurationFormat(Box<duration_format::State>),
    ArrayMutate(Box<array_mutate::State>),
    ArraySearch(Box<array_search::State>),
    ArrayCallbacks(Box<array_callbacks::State>),
    ArrayCopy(Box<array_copy::State>),
    AddEntries(Box<add_entries::State>),
    StringMethods(Box<string_methods::State>),
    ObjectStatics(Box<object_statics::State>),
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
            .ok_or(Fault::StringTooLong)?;
        if len != 0 {
            self.parts.push((text, 0, len));
        }
        Ok(())
    }

    fn element(&mut self, value: Value) -> Result<Action, Fault> {
        if matches!(value, Value::Object(_) | Value::BigInt(_)) {
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
                if matches!(self.separator, Value::Object(_) | Value::BigInt(_)) {
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

pub(crate) fn detach(
    natives: &mut Vec<NativeFrame>,
    depth: usize,
    base: usize,
) -> Vec<NativeFrame> {
    let at = natives.partition_point(|n| n.depth < depth);
    let mut saved = natives.split_off(at);
    for n in &mut saved {
        n.depth -= depth;
        n.base -= base;
    }
    saved
}

impl NativeFrame {
    pub(super) fn base(&self) -> usize {
        self.base
    }
    pub(crate) fn depth(&self) -> usize {
        self.depth
    }
}

pub(crate) fn reattach(natives: &mut [NativeFrame], depth: usize, base: usize) {
    for n in natives {
        n.depth += depth;
        n.base += base;
    }
}

pub(crate) fn generator_boundary(state: &VmState) -> Option<ObjectId> {
    state.natives.last().and_then(|n| match n.state {
        NativeState::Generator { id, .. } if n.depth + 1 == state.depth() => Some(id),
        NativeState::AsyncGenerator(ref a) if n.depth + 1 == state.depth() => a.id,
        _ => None,
    })
}

pub(crate) fn async_id(state: &VmState) -> Option<ObjectId> {
    // An await in shared disposal retains a native frame inside this
    // activation. Its boundary is the nearest native below the JS frame.
    state
        .natives
        .iter()
        .rev()
        .find(|n| n.depth < state.depth())
        .and_then(|n| match &n.state {
            NativeState::Async(a) if n.depth + 1 == state.depth() => Some(a.id),
            NativeState::AsyncGenerator(a) if n.depth + 1 == state.depth() => a.id,
            _ => None,
        })
}

pub(crate) fn async_generator_boundary(
    state: &mut VmState,
    boundary: super::async_generator::Boundary,
) -> Result<(), Fault> {
    push_native(state, NativeState::AsyncGenerator(Box::new(boundary)))?;
    let index = state.natives.len() - 1;
    // A queue may only be claimed after both boundary resources exist.
    // A handler-limit failure must leave the lazy generator resumable.
    if let Err(error) =
        state.push_handler(super::exception::HandlerKind::NativeCatch { native: index })
    {
        state.natives.pop();
        return Err(error);
    }
    Ok(())
}

pub(crate) fn generator_awaiting(state: &mut VmState) -> Result<(), Fault> {
    let index = state.natives.len() - 1;
    state.pop_native_catch(index);
    state.push_handler(super::exception::HandlerKind::NativeCatch { native: index })?;
    state.natives[index].awaiting = Awaiting::CallResult;
    Ok(())
}

pub(crate) fn suspend_generator_return(state: &mut VmState) -> Result<(), Fault> {
    let Some(frame) = state.natives.last_mut() else {
        return Err(Fault::Internal("async generator boundary missing"));
    };
    let NativeState::AsyncGenerator(a) = &mut frame.state else {
        return Err(Fault::Internal("async generator boundary missing"));
    };
    a.suspended();
    frame.awaiting = Awaiting::CallResult;
    Ok(())
}

pub(crate) fn async_boundary(
    state: &mut VmState,
    boundary: super::asynchronous::Boundary,
) -> Result<(), Fault> {
    push_native(state, NativeState::Async(boundary))?;
    let index = state.natives.len() - 1;
    if let Err(error) =
        state.push_handler(super::exception::HandlerKind::NativeCatch { native: index })
    {
        state.natives.pop();
        return Err(error);
    }
    state.natives[index].awaiting = Awaiting::CallResult;
    Ok(())
}

pub(crate) fn await_promise(
    state: &mut VmState,
    promise: super::promise::PromiseState,
) -> Result<(), Fault> {
    push_native(state, NativeState::Promise(promise))
}

impl Trace for NativeFrame {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        match &self.state {
            NativeState::BuiltinConstruct(constructor) => constructor.trace(visit),
            NativeState::CopyProperties(copy) => copy.trace(visit),
            NativeState::ObjectStatics(object) => object.trace(visit),
            NativeState::Rest { values, out, .. } => {
                for value in values {
                    value.trace(visit);
                }
                out.trace(visit);
            }
            NativeState::Async(a) => a.trace(visit),
            NativeState::AsyncGenerator(a) => a.trace(visit),
            NativeState::Eval(eval) => eval.trace(visit),
            NativeState::DynamicFunction(function) => function.trace(visit),
            NativeState::ArrayFrom(from) => from.trace(visit),
            NativeState::AddEntries(entries) => entries.trace(visit),
            NativeState::MathSumPrecise(sum) => sum.trace(visit),
            NativeState::ArraySort(sort) => sort.trace(visit),
            NativeState::ArrayMutate(array) => array.trace(visit),
            NativeState::ArraySearch(array) => array.trace(visit),
            NativeState::ArrayCallbacks(array) => array.trace(visit),
            NativeState::ArrayCopy(array) => array.trace(visit),
            NativeState::Iterator(iterator) => iterator.trace(visit),
            NativeState::Dispose(dispose) => dispose.trace(visit),
            NativeState::Disposal(disposal) => disposal.trace(visit),
            NativeState::Generator { id, failed } => {
                visit((*id).into());
                failed.iter().for_each(|e| e.value.trace(visit));
            }
            NativeState::TypedArray(array) => array.trace(visit),
            NativeState::Promise(p) => p.trace(visit),
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
            NativeState::Intl(intl) => intl.trace(visit),
            NativeState::NumberFormat(format) => format.trace(visit),
            NativeState::PluralRules(rules) => rules.trace(visit),
            NativeState::RelativeTimeFormat(format) => format.trace(visit),
            NativeState::DisplayNames(names) => names.trace(visit),
            NativeState::DurationFormat(format) => format.trace(visit),
            NativeState::Collator(collator) => collator.trace(visit),
            NativeState::DateTimeFormat(format) => format.trace(visit),
            NativeState::ListFormat(format) => format.trace(visit),
            NativeState::Segmenter(segmenter) => segmenter.trace(visit),
            NativeState::Join(join) => {
                join.this.trace(visit);
                join.separator.trace(visit);
            }
            NativeState::ArrayToString { this, .. } => this.trace(visit),
            NativeState::Error(error) => error.trace(visit),
            NativeState::RegExp(re) => re.trace(visit),
            NativeState::StringReplace(replace) => replace.trace(visit),
            NativeState::StringMethods(method) => method.trace(visit),
            NativeState::Reduce { f, vec, acc, .. } => {
                for v in [f, vec, acc] {
                    v.trace(visit);
                }
            }
            NativeState::NeedAll { defs, out, .. } => {
                defs.trace(visit);
                out.trace(visit);
            }
            NativeState::Convert { operands, .. }
            | NativeState::MathHypot { operands, .. }
            | NativeState::BigIntParse { operands, .. } => {
                for v in operands {
                    v.trace(visit);
                }
            }
            NativeState::StringSearch(_)
            | NativeState::DateParse { .. }
            | NativeState::StringBuild(_)
            | NativeState::BigIntFormat(_)
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
            NativeState::Proxy(proxy) => proxy.trace(visit),
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
        Reduce | NeedAll | GetAsyncIterator | GetIterator | UsingCheck | UsingDispose(_)
        | UsingCheckAsync | UsingDisposeAsync(_) | ForInNext | Rest(_) | CopyDataProperties => true,

        NewPrivateName(_) | GetPrivate | PutPrivate | PrivateIn | DefinePrivateField
        | DefinePrivateMethod | DefinePrivateGetter | DefinePrivateSetter => false,
        PendingException | SuppressedError | PendingCancel => false,
        IsCell | IsObject | ToObject => false,
        RegExpLiteral(_) | Const(_) | Undefined | GetLocal(_) | SetLocal(_) | Pop | Dup | Add
        | Sub | Mul | Lt | StrictEq | Not | Jump(_) | JumpIfFalse(_) | Return | NewObject
        | HostEffect(_) | NewVector(_) | NewMap(_) | TemplateObject(_) | Count | Nth | Get
        | GetOr | Assoc | Dissoc | Conj | PopLast | NewVectorBuilder | NewMapBuilder
        | BuilderConj | BuilderAssoc | Freeze | Div | Rem | Neg | ToNumber | ToNumeric | Inc
        | Dec | ToNumberPair | Gt | Le | Ge | LooseEq | ThrowError(_) | Function(..) | Call(_)
        | CurrentFunction | GetCapture(_) | NewCell | CellInit | CellGet | CellSet | AtomNew
        | AtomLoad | AtomCas | Need | Throw | PushCatch(_) | PushFinally(_) | PopCatch
        | PopFinally | EndFinally | BitAnd | BitOr | BitXor | Shl | Sar | UShr | BitNot | Pow
        | CallFinally | DropCompletion | Plus | CompareLt | CompareGt | CompareLe | CompareGe
        | Typeof | GetField(_) | PutField(_) | DefineField(_) | GetBuiltinGlobal(_)
        | CallMethod(_) | NewObjectLiteral | ToString | GetThis | NewArray | AppendElement
        | AppendHole | GetElem | PutElem | Dup2 | ToPropertyKey | Construct(_) | SetProto
        | Instanceof | Delete | In | ForInStart | NewArguments | Method(..) | DefineGetter(_)
        | DefineSetter(_) | RequireObject | PendingThrow | CallSpread | CallMethodSpread
        | ConstructSpread | CheckSpreadArguments => false,
        AsyncGeneratorStart
        | GeneratorStart
        | GeneratorYield
        | GeneratorInitialYield
        | GeneratorYieldStar
        | RequireCoercible
        | OwnKeys
        | IsEnumerable
        | DefineElem
        | AsyncStart
        | Await
        | DefineGetterElem
        | DefineSetterElem
        | DefineClassMethod
        | DefineClassGetter
        | DefineClassSetter
        | PutGlobalStrict(_)
        | PutFieldStrict(_)
        | PutElemStrict
        | DeleteStrict
        | SetComputedName(_)
        | CheckDirectEval
        | GetRawThis
        | GetNewTarget
        | CheckSloppyThis => false,
        CheckHeritage | InitHeritage | GetSuperConstructor | ConstructSuper(_)
        | ConstructSuperSpread | BindThis | ResolveThis | ForwardArguments | GetSuperBase
        | GetSuperElem | PutSuperElem | PutSuperElemStrict | ToSuperKey | Dup3 => false,
        GetGlobalObject
        | GetGlobal(_)
        | GetGlobalOrUndefined(_)
        | PutGlobal(_)
        | DeclareGlobalVar(_)
        | DeclareGlobalFunction(_)
        | DeclareGlobalLet(_)
        | DeclareGlobalConst(_)
        | InitGlobalLexical(_)
        | CheckGlobalVar(_)
        | CheckGlobalFunction(_)
        | DeleteGlobal(_)
        | DeclareGlobalConstValue(_)
        | CheckGlobalLexicalConflict(_) => false,
    }
}

/// Start observable value coercion for an integer-indexed write. Keep both
/// the original RHS (the assignment result) and the receiver traced.
pub(crate) fn typed_array_write(
    heap: &Heap,
    state: &mut VmState,
    base: &Value,
    key: &property::Key,
    value: &Value,
) -> Result<bool, Fault> {
    typed_array_write_result(heap, state, base, key, value, value)
}

/// CreateDataProperty validates the integer-indexed descriptor before coercion
/// and returns the receiver, rather than the assignment's original RHS.
pub(crate) fn typed_array_define(
    heap: &Heap,
    state: &mut VmState,
    base: &Value,
    key: &property::Key,
    value: &Value,
) -> Result<bool, Fault> {
    let Value::Object(id) = base else {
        return Ok(false);
    };
    if !matches!(value, Value::Object(_)) {
        return Ok(false);
    }
    let desc = crate::object::PropertyDescriptor {
        value: Some(value.clone()),
        writable: Some(true),
        enumerable: Some(true),
        configurable: Some(true),
        ..Default::default()
    };
    if property::typed_array_descriptor(heap, state.diag.task, *id, key, &desc)?.is_none() {
        return Ok(false);
    }
    typed_array_write_result(heap, state, base, key, value, base)
}

fn typed_array_write_result(
    heap: &Heap,
    state: &mut VmState,
    base: &Value,
    key: &property::Key,
    value: &Value,
    result: &Value,
) -> Result<bool, Fault> {
    if !matches!(value, Value::Object(_)) {
        return Ok(false);
    }
    let Value::Object(id) = base else {
        return Ok(false);
    };
    let Some(index) = crate::object::typed_array::numeric_key(key) else {
        return Ok(false);
    };
    if crate::object::typed_array::view(heap, state.diag.task, *id)?.is_none() {
        return Ok(false);
    }
    push_native(
        state,
        NativeState::Convert {
            formatter: None,
            number_scan: None,
            target: ConvertTarget::TypedArrayWrite,
            operands: vec![
                base.clone(),
                result.clone(),
                Value::Number(index),
                value.clone(),
            ],
            next: 3,
            end: 4,
            hint: Hint::Number,
            method: 0,
            method_get: false,
            symbol_method: SymbolMethod::Pending,
        },
    )?;
    Ok(true)
}

/// The top `N` operands of the running frame, oldest first, not popped.
fn operands<const N: usize>(state: &VmState) -> Result<[Value; N], Fault> {
    let ops = state.operands();
    ops.len()
        .checked_sub(N)
        .map(|at| std::array::from_fn(|i| ops[at + i].clone()))
        .ok_or(Fault::Internal("operand stack underflow"))
}

/// Start a continuation for object conversion or BigInt/String comparison.
/// Other primitive operands retain their single-step path.
/// Property operations retain their base checks before observable key
/// conversion; equality never converts two objects or a nullish pair.
pub(crate) fn start_conversion(state: &mut VmState, instr: Instr) -> Result<bool, Fault> {
    use Instr::*;
    if matches!(
        instr,
        CompareLt | CompareGt | CompareLe | CompareGe | LooseEq
    ) {
        let ops = state.operands();
        let at = ops
            .len()
            .checked_sub(2)
            .ok_or(Fault::Internal("operand stack underflow"))?;
        if let Some(parser) = bigint_comparison(instr, &ops[at..]) {
            state.pop()?;
            state.pop()?;
            push_native(state, parser)?;
            return Ok(true);
        }
    }
    let (count, first, end, hint) = match instr {
        ToNumber | ToNumeric => (1, 0, 1, Hint::Number),
        ToString | ToSuperKey => (1, 0, 1, Hint::String),
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
        GetElem | ToPropertyKey | Delete | DeleteStrict | PutElem | PutElemStrict => {
            let count = if matches!(instr, PutElem | PutElemStrict) {
                3
            } else {
                2
            };
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
        PutSuperElem | PutSuperElemStrict => {
            let base = state
                .operands()
                .iter()
                .rev()
                .nth(2)
                .ok_or(Fault::Internal("operand stack underflow"))?;
            if !matches!(base, Value::Object(_)) {
                return Ok(false);
            }
            (4, 2, 3, Hint::String)
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
    if !ops[at + first..at + end].iter().any(|v| {
        matches!(v, Value::Object(_))
            || (matches!(v, Value::BigInt(_))
                && (matches!(
                    instr,
                    ToString
                        | ToSuperKey
                        | GetElem
                        | ToPropertyKey
                        | Delete
                        | DeleteStrict
                        | PutElem
                        | PutElemStrict
                        | PutSuperElem
                        | PutSuperElemStrict
                        | In
                ) || (instr == Plus
                    && ops[at..].iter().any(|v| matches!(v, Value::String(_))))))
    }) {
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
            formatter: None,
            number_scan: None,
            target: ConvertTarget::Instruction(instr),
            operands,
            next: first,
            end,
            hint,
            method: 0,
            method_get: false,
            symbol_method: SymbolMethod::Pending,
        },
    });
    Ok(true)
}

/// Snapshot the caller's binding references after argument evaluation. The
/// native continuation owns and traces them until compiled code takes over.
#[inline(never)]
pub(crate) fn start_direct_eval(
    heap: &Heap,
    state: &mut VmState,
    site: super::executable::EvalSite,
    args: &[Value],
    popped: usize,
) -> Result<(), Fault> {
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    let Value::String(text) = value else {
        state.replace(popped, value)?;
        return Ok(());
    };
    let mut native = eval::State::new(text)?;
    let mut captures = Vec::with_capacity(site.bindings.len() + 2);
    for binding in &site.bindings {
        captures.push(eval_source(heap, state, binding.source)?);
    }
    captures.push(eval_source(heap, state, site.receiver)?);
    if let Some(source) = site.new_target {
        captures.push(eval_source(heap, state, source)?);
    }
    for source in &site.environments {
        captures.push(eval_source(heap, state, *source)?);
    }
    if let Some(source) = site.super_home {
        captures.push(eval_source(heap, state, source)?);
    }
    if let Some((source, _)) = site.super_fields {
        captures.push(eval_source(heap, state, source)?);
    }
    native.caller = Some(eval::Caller { site, captures });
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::Eval(Box::new(native)))?;
    Ok(())
}

fn eval_source(
    heap: &Heap,
    state: &mut VmState,
    source: super::executable::EvalSource,
) -> Result<Value, Fault> {
    use super::executable::EvalSource;
    match source {
        EvalSource::Local(i) => Ok(state.local(i)?.clone()),
        EvalSource::Capture(i) => state
            .top()
            .callee
            .and_then(|id| {
                heap.read(id, |object| match object {
                    HeapObject::Function(f) => f.captures.get(usize::from(i)).cloned(),
                    _ => None,
                })
                .flatten()
            })
            .ok_or(Fault::Internal("eval caller capture is missing")),
        EvalSource::This => Ok(state.top().this.clone()),
        EvalSource::NewTarget => Ok(state.top().new_target.clone()),
        EvalSource::GlobalObject => dispatch::global_object(state, heap),
        EvalSource::CurrentFunction => {
            if let Some(id) = state.top().callee {
                return Ok(Value::Object(id));
            }
            let f = dispatch::new_function(
                heap,
                state.top().code.clone(),
                Box::new([]),
                state.diag.task,
                None,
            )?;
            if let Value::Object(id) = f {
                state.top_mut().callee = Some(id);
            }
            Ok(f)
        }
    }
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
    if !construct
        && matches!(
            which,
            Builtin::ObjectAssign
                | Builtin::ObjectEntries
                | Builtin::ObjectValues
                | Builtin::ObjectFromEntries
                | Builtin::ObjectDefineProperties
        )
    {
        object_statics::start(heap, state, which, args, popped)?;
        return Ok(true);
    }
    if matches!(
        which,
        Builtin::Number | Builtin::String | Builtin::Symbol | Builtin::BigInt
    ) {
        return start_primitive_constructor(state, which, args, construct, popped);
    }
    if which == Builtin::BigIntPrototypeToString && !construct {
        return start_bigint_to_string(heap, state, this, args, popped);
    }
    if which == Builtin::JsonStringify && !construct {
        return start_json_stringify(state, args, popped);
    }
    if matches!(which, Builtin::Error | Builtin::NativeError(_))
        || (which == Builtin::ErrorPrototypeToString && !construct)
    {
        return start_error(heap, state, which, this, args, popped);
    }
    // These leaves never coerce operands; avoid reserving the slow built-in
    // conversion frame before a proxy/prototype walk on a 64 KiB worker.
    if matches!(
        which,
        Builtin::ObjectGetPrototypeOf
            | Builtin::Object
            | Builtin::BigIntPrototypeValueOf
            | Builtin::Boolean
            | Builtin::BooleanPrototypeToString
            | Builtin::BooleanPrototypeValueOf
            | Builtin::NumberPrototypeValueOf
            | Builtin::StringPrototypeValueOf
            | Builtin::StringPrototypeToString
            | Builtin::IntlNumberFormatGetFormat
            | Builtin::IntlNumberFormatResolvedOptions
            | Builtin::ObjectSetPrototypeOf
            | Builtin::ObjectCreate
            | Builtin::ArraySpecies
            | Builtin::MapSet
            | Builtin::WeakMapSet
            | Builtin::MapGet
            | Builtin::WeakMapGet
            | Builtin::SetAdd
            | Builtin::SetHas
            | Builtin::SetDelete
            | Builtin::SetClear
            | Builtin::SetSize
            | Builtin::SetValues
            | Builtin::SetEntries
            | Builtin::WeakSetAdd
            | Builtin::WeakSetHas
            | Builtin::WeakSetDelete
    ) {
        return Ok(false);
    }
    if matches!(
        which,
        Builtin::RegExp
            | Builtin::RegExpExec
            | Builtin::RegExpTest
            | Builtin::RegExpToString
            | Builtin::RegExpFlags
    ) {
        return start_regexp(heap, state, which, this, args, construct, popped);
    }
    if matches!(
        which,
        Builtin::IntlNumberFormat
            | Builtin::IntlNumberFormatBound
            | Builtin::IntlNumberFormatSupportedLocales
    ) && (!construct || which == Builtin::IntlNumberFormat)
    {
        return start_number_format(heap, state, which, this, args, popped);
    }
    if !construct
        && matches!(
            which,
            Builtin::ObjectKeys
                | Builtin::ObjectGetOwnPropertyNames
                | Builtin::ObjectGetOwnPropertyDescriptor
                | Builtin::ObjectFreeze
                | Builtin::ObjectIsFrozen
        )
        && let Some(Value::Object(target)) = args.first()
        && super::proxy::inspect(heap, state.diag.task, *target)?.is_some()
    {
        use super::proxy::Mode;
        let mode = match which {
            Builtin::ObjectKeys => Some(Mode::Keys {
                target: *target,
                enumerable: true,
                symbols: false,
            }),
            Builtin::ObjectFreeze => Some(Mode::Freeze(*target)),
            Builtin::ObjectIsFrozen => Some(Mode::Frozen(*target)),
            Builtin::ObjectGetOwnPropertyNames => Some(Mode::Keys {
                target: *target,
                enumerable: false,
                symbols: false,
            }),
            Builtin::ObjectGetOwnPropertyDescriptor
                if !matches!(args.get(1), Some(Value::Object(_) | Value::BigInt(_))) =>
            {
                Some(Mode::Descriptor {
                    target: *target,
                    key: property::to_property_key(args.get(1).unwrap_or(&Value::Undefined))?,
                })
            }
            _ => None,
        };
        if let Some(mode) = mode {
            start_proxy(state, mode, popped)?;
            return Ok(true);
        }
    }
    if which == Builtin::AsyncDisposableStackDispose && !construct {
        dispose::start_async_stack(heap, state, this.clone(), popped)?;
        return Ok(true);
    }
    if !construct {
        match which {
            Builtin::ArrayPrototypeSort | Builtin::ArrayPrototypeToSorted => {
                return start_array_sort(state, which, this, args, popped);
            }
            Builtin::ArrayPrototypeSlice
            | Builtin::ArrayPrototypeConcat
            | Builtin::ArrayPrototypeFlat
            | Builtin::ArrayPrototypeToReversed
            | Builtin::ArrayPrototypeWith
            | Builtin::ArrayOf => {
                return start_array_copy(state, which, this, args, popped);
            }
            Builtin::ArrayPrototypeForEach
            | Builtin::ArrayPrototypeMap
            | Builtin::ArrayPrototypeFilter
            | Builtin::ArrayPrototypeReduce
            | Builtin::ArrayPrototypeReduceRight
            | Builtin::ArrayPrototypeSome
            | Builtin::ArrayPrototypeEvery
            | Builtin::ArrayPrototypeFlatMap => {
                return start_array_callbacks(state, which, this, args, popped);
            }
            Builtin::ArrayPrototypeIndexOf
            | Builtin::ArrayPrototypeLastIndexOf
            | Builtin::ArrayPrototypeIncludes
            | Builtin::ArrayPrototypeFind
            | Builtin::ArrayPrototypeFindIndex
            | Builtin::ArrayPrototypeFindLast
            | Builtin::ArrayPrototypeFindLastIndex
            | Builtin::ArrayPrototypeAt => {
                return start_array_search(state, which, this, args, popped);
            }
            Builtin::ArrayPrototypePop
            | Builtin::ArrayPrototypeShift
            | Builtin::ArrayPrototypeUnshift
            | Builtin::ArrayPrototypeSplice
            | Builtin::ArrayPrototypeReverse
            | Builtin::ArrayPrototypeFill
            | Builtin::ArrayPrototypeCopyWithin => {
                return start_array_mutator(state, which, this, args, popped);
            }
            Builtin::DisposableStackUse
            | Builtin::DisposableStackDispose
            | Builtin::AsyncDisposableStackUse
            | Builtin::AsyncDisposableStackDisposeBody => {
                return start_stack_method(heap, state, which, this, args, popped);
            }
            Builtin::DisposableStackAdopt
            | Builtin::DisposableStackDefer
            | Builtin::DisposableStackMove
            | Builtin::DisposableStackDisposed
            | Builtin::AsyncDisposableStackAdopt
            | Builtin::AsyncDisposableStackDefer
            | Builtin::AsyncDisposableStackMove
            | Builtin::AsyncDisposableStackDisposed => {
                return Ok(false);
            }
            _ => {}
        }
    }
    if which == Builtin::Function {
        let native = eval::DynamicFunction::new(args);
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::DynamicFunction(Box::new(native)))?;
        return Ok(true);
    }
    if !construct
        && matches!(
            which,
            Builtin::StringFromCharCode | Builtin::StringFromCodePoint
        )
    {
        return start_string_codes(state, which, args, popped);
    }
    if which == Builtin::Eval && !construct {
        let value = args.first().cloned().unwrap_or(Value::Undefined);
        let native = if let Value::String(text) = value {
            eval::State::new(text)?
        } else {
            state.replace(popped, value)?;
            return Ok(true);
        };
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::Eval(Box::new(native)))?;
        return Ok(true);
    }
    if !construct {
        let iterator = match which {
            Builtin::ArrayValues
            | Builtin::ArrayPrototypeKeys
            | Builtin::ArrayPrototypeEntries
            | Builtin::TypedArrayValues
            | Builtin::IteratorIdentity
            | Builtin::TypedArrayBuffer
            | Builtin::TypedArrayLength
            | Builtin::TypedArrayByteLength
            | Builtin::TypedArrayByteOffset => {
                return Ok(false);
            }
            Builtin::IteratorNext(kind) => {
                Some(iterator::State::next(heap, state.diag.task, this, kind)?)
            }
            Builtin::StringIterator => Some(iterator::State::string(this.clone())?),
            _ => None,
        };
        if let Some(iterator) = iterator {
            state.truncate_operands(state.stack_len() - popped)?;
            push_native(state, NativeState::Iterator(Box::new(iterator)))?;
            return Ok(true);
        }
    }
    if (which == Builtin::IntlDurationFormat && construct)
        || (!construct
            && matches!(
                which,
                Builtin::IntlDurationFormatFormat
                    | Builtin::IntlDurationFormatToParts
                    | Builtin::IntlDurationFormatSupportedLocales
            ))
    {
        return duration_format::start(heap, state, which, this, args, popped).map(|()| true);
    }
    if matches!(
        which,
        Builtin::Promise
            | Builtin::PromiseResolve
            | Builtin::PromiseReject
            | Builtin::PromiseThen
            | Builtin::PromiseCatch
            | Builtin::PromiseResolver(_, _)
            | Builtin::PromiseReaction(_)
            | Builtin::PromiseAll
            | Builtin::PromiseRace
            | Builtin::PromiseAllSettled
            | Builtin::PromiseAny
            | Builtin::PromiseWithResolvers
            | Builtin::PromiseFinally
            | Builtin::PromiseCallback(_, _)
            | Builtin::PromiseResolveThenable(_)
    ) {
        return start_promise(heap, state, which, this, args, construct, popped);
    }
    if matches!(
        which,
        Builtin::AsyncGeneratorNext | Builtin::AsyncGeneratorReturn | Builtin::AsyncGeneratorThrow
    ) {
        let kind = match which {
            Builtin::AsyncGeneratorReturn => super::generator::ResumeKind::Return,
            Builtin::AsyncGeneratorThrow => super::generator::ResumeKind::Throw,
            _ => super::generator::ResumeKind::Next,
        };
        super::async_generator::enqueue(
            heap,
            state,
            this,
            args.first().cloned().unwrap_or(Value::Undefined),
            kind,
            popped,
        )?;
        return Ok(true);
    }
    if matches!(
        which,
        Builtin::GeneratorNext | Builtin::GeneratorReturn | Builtin::GeneratorThrow
    ) {
        let kind = match which {
            Builtin::GeneratorReturn => super::generator::ResumeKind::Return,
            Builtin::GeneratorThrow => super::generator::ResumeKind::Throw,
            _ => super::generator::ResumeKind::Next,
        };
        return start_generator_resume(heap, state, this, args, popped, kind);
    }
    if let Builtin::IntegerTypedArray(kind) = which
        && construct
    {
        return start_typed_array_constructor(heap, state, kind, args, popped);
    }
    start_builtin_slow(heap, state, which, this, args, construct, popped)
}

pub(crate) fn start_builtin_constructor(
    state: &mut VmState,
    which: builtin::Builtin,
    target: Value,
    args: Vec<Value>,
    popped: usize,
) -> Result<(), Fault> {
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(
        state,
        NativeState::BuiltinConstruct(Box::new(construct::State::new(which, target, args))),
    )
}

/// Enter the existing built-in implementation after prototype selection. In
/// particular collections must inherit overridden set/add before consuming an
/// iterable, and Promise must hold its final prototype before its executor.
#[inline(never)]
fn initialize_builtin(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    args: &[Value],
    prototype: Option<ObjectId>,
) -> Result<(), Fault> {
    // Primitive conversions can walk user properties on 64 KiB workers.
    // Keep unrelated collection/Promise construction storage off that stack.
    if matches!(
        which,
        builtin::Builtin::Number | builtin::Builtin::String | builtin::Builtin::Boolean
    ) {
        if !start_builtin(heap, state, which, &Value::Undefined, args, true, 0)? {
            let result = builtin::primitive_constructor(heap, state.diag.task, which, args, true)?;
            state.push(result);
        }
        return Ok(());
    }
    initialize_builtin_slow(heap, state, which, args, prototype)
}

#[inline(never)]
fn initialize_builtin_slow(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    args: &[Value],
    prototype: Option<ObjectId>,
) -> Result<(), Fault> {
    use builtin::Builtin;
    let task = state.diag.task;
    let result = match which {
        Builtin::RegExp => {
            let native = regexp::start(heap, task, which, &Value::Undefined, args, true)?
                .ok_or(Fault::Internal("RegExp constructor missing"))?;
            return push_native(state, NativeState::RegExp(native));
        }
        Builtin::IntlLocale
        | Builtin::IntlNumberFormat
        | Builtin::IntlCollator
        | Builtin::IntlDateTimeFormat
        | Builtin::IntlPluralRules
        | Builtin::IntlListFormat
        | Builtin::IntlSegmenter
        | Builtin::IntlRelativeTimeFormat
        | Builtin::IntlDisplayNames
        | Builtin::IntlDurationFormat => {
            return initialize_intl_constructor(heap, state, which, args);
        }
        Builtin::Date | Builtin::ArrayBuffer => {
            if !start_builtin_conversion(heap, state, which, &Value::Undefined, args, true, 0)? {
                return Err(Fault::Internal("coercing constructor missing"));
            }
            return Ok(());
        }
        Builtin::Object => builtin::new_object(heap, task)?,
        Builtin::Array => builtin::array_constructor(heap, task, args)?,
        Builtin::Map | Builtin::WeakMap | Builtin::Set | Builtin::WeakSet => {
            let result = match which {
                Builtin::Map => builtin::map_constructor(heap, task)?,
                Builtin::WeakMap => builtin::weak_map_constructor(heap, task)?,
                Builtin::Set => builtin::set_constructor(heap, task)?,
                Builtin::WeakSet => builtin::weak_set_constructor(heap, task)?,
                _ => unreachable!(),
            };
            construct::initialize_prototype(
                heap,
                task,
                &result,
                prototype.ok_or(Fault::Internal("collection prototype missing"))?,
            )?;
            let items = args.first().cloned().unwrap_or(Value::Undefined);
            return if matches!(which, Builtin::Set | Builtin::WeakSet) {
                start_add_values(state, result, items, 0)
            } else {
                start_add_entries(state, result, items, 0)
            };
        }
        Builtin::WeakRef => builtin::weak_ref_constructor(heap, state, args)?,
        Builtin::FinalizationRegistry => {
            builtin::finalization_registry_constructor(heap, state, args)?
        }
        Builtin::Promise => {
            let native = super::promise::constructor(
                heap,
                task,
                args,
                prototype.ok_or(Fault::Internal("promise prototype missing"))?,
            )?;
            return push_native(state, NativeState::Promise(native));
        }
        _ => {
            if start_builtin(heap, state, which, &Value::Undefined, args, true, 0)? {
                // Error allocates on entry, before message/cause callbacks.
                if let Some(proto) = prototype
                    && let Some(NativeFrame {
                        state: NativeState::Error(error),
                        ..
                    }) = state.natives.last()
                    && let error::ErrorState::Constructor { object, .. } = error.as_ref()
                {
                    construct::initialize_prototype(heap, task, object, proto)?;
                }
                return Ok(());
            }
            builtin::primitive_constructor(heap, task, which, args, true)?
        }
    };
    state.push(result);
    Ok(())
}

#[inline(never)]
fn initialize_intl_constructor(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    args: &[Value],
) -> Result<(), Fault> {
    use builtin::Builtin;
    let task = state.diag.task;
    let this = &Value::Undefined;
    let native = match which {
        Builtin::IntlLocale => {
            NativeState::Intl(box_constructor_state(|| intl::Intl::new(args, true))?)
        }
        Builtin::IntlNumberFormat => NativeState::NumberFormat(box_constructor_state(|| {
            number_format::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlCollator => NativeState::Collator(box_constructor_state(|| {
            collator::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlDateTimeFormat => NativeState::DateTimeFormat(box_constructor_state(|| {
            date_time_format::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlPluralRules => NativeState::PluralRules(box_constructor_state(|| {
            plural_rules::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlListFormat => NativeState::ListFormat(box_constructor_state(|| {
            list_format::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlSegmenter => NativeState::Segmenter(box_constructor_state(|| {
            segmenter::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlRelativeTimeFormat => {
            NativeState::RelativeTimeFormat(box_constructor_state(|| {
                relative_time_format::State::new(which, this, args, heap, task)
            })?)
        }
        Builtin::IntlDisplayNames => NativeState::DisplayNames(box_constructor_state(|| {
            display_names::State::new(which, this, args, heap, task)
        })?),
        Builtin::IntlDurationFormat => NativeState::DurationFormat(box_constructor_state(|| {
            duration_format::State::new(which, this, args, heap, task)
        })?),
        _ => return Err(Fault::Internal("not an Intl constructor")),
    };
    push_native(state, native)
}

// Each constructor creates its temporary state on a separate stack frame.
// Debug builds otherwise reserve all Intl state temporaries in the dispatcher.
#[inline(never)]
fn box_constructor_state<T>(make: impl FnOnce() -> Result<T, Fault>) -> Result<Box<T>, Fault> {
    make().map(Box::new)
}

#[inline(never)]
fn start_regexp(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
    popped: usize,
) -> Result<bool, Fault> {
    if let Some(native) = regexp::start(heap, state.diag.task, which, this, args, construct)? {
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::RegExp(native))?;
        return Ok(true);
    }
    Ok(false)
}

#[inline(never)]
fn start_number_format(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = number_format::State::new(which, this, args, heap, state.diag.task)?;
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::NumberFormat(Box::new(native)))?;
    Ok(true)
}

#[inline(never)]
fn start_primitive_constructor(
    state: &mut VmState,
    which: builtin::Builtin,
    args: &[Value],
    construct: bool,
    popped: usize,
) -> Result<bool, Fault> {
    use builtin::Builtin;
    if matches!(which, Builtin::Symbol | Builtin::BigInt) && construct {
        return Ok(false);
    }
    if which == Builtin::BigInt
        && !construct
        && let Some(Value::String(text)) = args.first()
    {
        let native = NativeState::BigIntParse {
            parser: Box::new(crate::value::bigint::StringToBigInt::new(text.clone())),
            instr: None,
            operands: vec![],
            index: 0,
            wrap: None,
        };
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, native)?;
        return Ok(true);
    }
    let Some(value) = args.first().filter(|v| {
        matches!(v, Value::Object(_))
            || (matches!(which, Builtin::String | Builtin::Symbol) && matches!(v, Value::BigInt(_)))
    }) else {
        return Ok(false);
    };
    // Native callbacks can start another conversion at the same JS frame
    // depth. Avoid the general dispatcher's unrelated constructor storage.
    let native = NativeState::Convert {
        formatter: None,
        number_scan: None,
        target: if which == Builtin::Symbol {
            ConvertTarget::SymbolDescription
        } else {
            ConvertTarget::PrimitiveConstructor { which, construct }
        },
        operands: vec![value.clone()],
        next: 0,
        end: 1,
        hint: if matches!(which, Builtin::String | Builtin::Symbol) {
            Hint::String
        } else {
            Hint::Number
        },
        method: 0,
        method_get: false,
        symbol_method: SymbolMethod::Pending,
    };
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, native)?;
    Ok(true)
}

#[inline(never)]
fn start_array_sort(
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = array_sort::State::new(
        this.clone(),
        args,
        which == builtin::Builtin::ArrayPrototypeToSorted,
    );
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::ArraySort(Box::new(native)))?;
    Ok(true)
}

#[inline(never)]
fn start_array_copy(
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let limit = state.forward_limits(state.stack_len() - popped).1;
    let native = array_copy::State::new(which, this.clone(), args, limit);
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::ArrayCopy(Box::new(native)))?;
    Ok(true)
}

#[inline(never)]
fn start_array_callbacks(
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = array_callbacks::State::new(which, this.clone(), args);
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::ArrayCallbacks(Box::new(native)))?;
    Ok(true)
}

#[inline(never)]
fn start_array_search(
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = array_search::State::new(which, this.clone(), args);
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::ArraySearch(Box::new(native)))?;
    Ok(true)
}

/// Avoid the general constructor dispatcher's large stack frame for mutators.
#[inline(never)]
fn start_array_mutator(
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = array_mutate::State::new(which, this.clone(), args);
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::ArrayMutate(Box::new(native)))?;
    Ok(true)
}

/// Stack methods run without reserving unrelated built-in constructor storage.
#[inline(never)]
fn start_stack_method(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = dispose::Stack::method(
        heap,
        state.diag.task,
        which,
        this,
        args,
        state.error_stack(state.top().pc.saturating_sub(1))?,
    )?;
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::Disposal(Box::new(native)))?;
    Ok(true)
}

/// JSON wrapper serialization needs no unrelated constructor temporaries.
#[inline(never)]
fn start_json_stringify(state: &mut VmState, args: &[Value], popped: usize) -> Result<bool, Fault> {
    if !matches!(args.get(1), None | Some(Value::Undefined | Value::Null)) {
        return Err(Fault::TypeError(
            "JSON.stringify replacer is not supported".into(),
        ));
    }
    let native = NativeState::JsonStringify(Box::new(super::json::Stringifier::new(
        args.first().cloned().unwrap_or(Value::Undefined),
        args.get(2).cloned().unwrap_or(Value::Undefined),
    )));
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, native)?;
    Ok(true)
}

/// Validate branding before radix conversion, keeping wrapper access off the
/// general dispatch frame so it fits the same small workers as other wrappers.
#[inline(never)]
fn start_bigint_to_string(
    heap: &Heap,
    state: &mut VmState,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let value = builtin::this_bigint_value(heap, state.diag.task, this)?;
    let radix = args
        .first()
        .filter(|v| !matches!(v, Value::Undefined))
        .cloned()
        .unwrap_or(Value::Number(10.0));
    let native = NativeState::Convert {
        formatter: None,
        number_scan: None,
        target: ConvertTarget::BigIntToString,
        operands: vec![radix, Value::BigInt(value)],
        next: 0,
        end: 1,
        hint: Hint::Number,
        method: 0,
        method_get: false,
        symbol_method: SymbolMethod::Pending,
    };
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, native)?;
    Ok(true)
}

/// Both primitive and object arguments need budgeted conversion, without
/// reserving the general built-in dispatcher's temporaries on small stacks.
#[inline(never)]
fn start_string_codes(
    state: &mut VmState,
    which: builtin::Builtin,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let native = NativeState::Convert {
        formatter: None,
        number_scan: None,
        target: ConvertTarget::StringBuiltin {
            which,
            access: false,
            concat_len: 0,
        },
        operands: args.to_vec(),
        next: 0,
        end: args.len(),
        hint: Hint::Number,
        method: 0,
        method_get: false,
        symbol_method: SymbolMethod::Pending,
    };
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, native)?;
    Ok(true)
}

/// Keep field constructors returning typed arrays off the large general
/// built-in dispatch frame so they also execute on 64 KiB worker stacks.
#[inline(never)]
fn start_typed_array_constructor(
    heap: &Heap,
    state: &mut VmState,
    kind: crate::object::typed_array::ElementType,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    let first = args.first().cloned().unwrap_or(Value::Undefined);
    let from_buffer = crate::object::typed_array::is_buffer(heap, &first);
    if matches!(first, Value::Object(_)) && !from_buffer {
        let native = typed_array::TypedArray::constructor(kind, first);
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::TypedArray(Box::new(native)))?;
        return Ok(true);
    }
    if from_buffer {
        crate::object::array_buffer::receiver(heap, state.diag.task, &first)?;
    }
    let mut operands = vec![first];
    if from_buffer {
        operands.push(args.get(1).cloned().unwrap_or(Value::Undefined));
        if let Some(length) = args.get(2).filter(|v| !matches!(v, Value::Undefined)) {
            operands.push(length.clone());
        }
    }
    let end = operands.len();
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(
        state,
        NativeState::Convert {
            formatter: None,
            number_scan: None,
            target: ConvertTarget::TypedArray { kind, from_buffer },
            operands,
            next: usize::from(from_buffer),
            end,
            hint: Hint::Number,
            method: 0,
            method_get: false,
            symbol_method: SymbolMethod::Pending,
        },
    )?;
    Ok(true)
}

#[inline(never)]
fn start_promise(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
    popped: usize,
) -> Result<bool, Fault> {
    let native = super::promise::start(heap, state, which, this, args, construct)?;
    state.truncate_operands(state.stack_len() - popped)?;
    if let Some(native) = native {
        push_native(state, NativeState::Promise(native))?;
    } else {
        state.push(Value::Undefined);
    }
    Ok(true)
}

#[inline(never)]
fn start_generator_resume(
    heap: &Heap,
    state: &mut VmState,
    this: &Value,
    args: &[Value],
    popped: usize,
    kind: super::generator::ResumeKind,
) -> Result<bool, Fault> {
    let id = super::generator::receiver(heap, state.diag.task, this)?;
    let value = args.first().cloned().unwrap_or(Value::Undefined);
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::Generator { id, failed: None })?;
    let boundary_index = state.natives.len() - 1;
    if let Err(error) = state.push_handler(super::exception::HandlerKind::NativeCatch {
        native: boundary_index,
    }) {
        state.natives.pop();
        return Err(error);
    }
    let resumed = match super::generator::resume(heap, state, id, value.clone(), kind) {
        Ok(resumed) => resumed,
        Err(error) => {
            // A rejected attachment has not entered the body. Remove
            // its boundary so the failure cannot complete the generator.
            state.pop_native_catch(boundary_index);
            state.natives.pop();
            return Err(error);
        }
    };
    if resumed {
        let boundary = state
            .natives
            .get_mut(boundary_index)
            .ok_or(Fault::Internal("missing generator boundary"))?;
        boundary.awaiting = Awaiting::CallResult;
    } else {
        state.pop_native_catch(state.natives.len() - 1);
        state.natives.pop();
        if kind == super::generator::ResumeKind::Throw {
            return Err(Fault::Thrown(value));
        }
        state.push(super::generator::result(
            heap,
            state.diag.task,
            if kind == super::generator::ResumeKind::Return {
                value
            } else {
                Value::Undefined
            },
            true,
        )?);
    }
    Ok(true)
}

// Iterator callbacks above must not reserve every unrelated constructor's
// temporary storage while executing spread on a small worker stack.
#[inline(never)]
fn start_builtin_slow(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
    popped: usize,
) -> Result<bool, Fault> {
    use builtin::Builtin;
    if string_methods::handles(which) && !construct {
        let native = string_methods::State::new(which, this, args)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::StringMethods(Box::new(native)))?;
        return Ok(true);
    }
    if which == Builtin::SimGc {
        state.truncate_operands(state.stack_len() - popped)?;
        state.push(Value::Undefined);
        state.collection_point = true;
        return Ok(true);
    }
    if matches!(
        which,
        Builtin::StringPrototypeReplace | Builtin::StringPrototypeReplaceAll
    ) && !construct
    {
        let native = string_replace::State::new(heap, state.diag.task, which, this, args)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::StringReplace(Box::new(native)))?;
        return Ok(true);
    }
    if matches!(
        which,
        Builtin::IntlCollator | Builtin::IntlCollatorBound | Builtin::IntlCollatorSupportedLocales
    ) && (!construct || which == Builtin::IntlCollator)
    {
        let native = collator::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::Collator(Box::new(native)))?;
        return Ok(true);
    }
    if matches!(
        which,
        Builtin::IntlDateTimeFormat
            | Builtin::IntlDateTimeFormatBound
            | Builtin::IntlDateTimeFormatSupportedLocales
    ) && (!construct || which == Builtin::IntlDateTimeFormat)
    {
        let native = date_time_format::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::DateTimeFormat(Box::new(native)))?;
        return Ok(true);
    }
    if (which == Builtin::IntlDisplayNames && construct)
        || (!construct
            && matches!(
                which,
                Builtin::IntlDisplayNamesOf | Builtin::IntlDisplayNamesSupportedLocales
            ))
    {
        let native = display_names::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::DisplayNames(Box::new(native)))?;
        return Ok(true);
    }
    if (which == Builtin::IntlRelativeTimeFormat && construct)
        || (!construct
            && matches!(
                which,
                Builtin::IntlRelativeTimeFormatFormat
                    | Builtin::IntlRelativeTimeFormatToParts
                    | Builtin::IntlRelativeTimeFormatSupportedLocales
            ))
    {
        let native = relative_time_format::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::RelativeTimeFormat(Box::new(native)))?;
        return Ok(true);
    }
    if (which == Builtin::IntlPluralRules && construct)
        || (matches!(
            which,
            Builtin::IntlPluralRulesSelect | Builtin::IntlPluralRulesSupportedLocales
        ) && !construct)
    {
        let native = plural_rules::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::PluralRules(Box::new(native)))?;
        return Ok(true);
    }
    if (which == Builtin::IntlSegmenter && construct)
        || (matches!(
            which,
            Builtin::IntlSegmenterSegment
                | Builtin::IntlSegmenterSupportedLocales
                | Builtin::IntlSegmentsContaining
                | Builtin::IntlSegmentIteratorNext
        ) && !construct)
    {
        let native = segmenter::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::Segmenter(Box::new(native)))?;
        return Ok(true);
    }
    if (which == Builtin::IntlListFormat && construct)
        || (matches!(
            which,
            Builtin::IntlListFormatFormat
                | Builtin::IntlListFormatFormatToParts
                | Builtin::IntlListFormatSupportedLocales
        ) && !construct)
    {
        let native = list_format::State::new(which, this, args, heap, state.diag.task)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::ListFormat(Box::new(native)))?;
        return Ok(true);
    }
    if (which == Builtin::IntlLocale && construct)
        || (which == Builtin::IntlGetCanonicalLocales && !construct)
    {
        let native = intl::Intl::new(args, construct)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::Intl(Box::new(native)))?;
        return Ok(true);
    }
    if !construct {
        let arg = |i| args.get(i).cloned().unwrap_or(Value::Undefined);
        let native = match which {
            Builtin::SetForEach => Some(NativeState::Iterator(Box::new(
                iterator::State::set_for_each(heap, state.diag.task, this.clone(), arg(0), arg(1))?,
            ))),
            Builtin::MapForEach => Some(NativeState::Iterator(Box::new(
                iterator::State::map_for_each(heap, state.diag.task, this.clone(), arg(0), arg(1))?,
            ))),

            Builtin::MathSumPrecise => Some(NativeState::MathSumPrecise(Box::new(
                sum_precise::State::new(arg(0)),
            ))),
            Builtin::ArrayFrom => Some(NativeState::ArrayFrom(Box::new(array_from::State::new(
                this.clone(),
                args,
            )))),
            Builtin::TypedArraySet | Builtin::TypedArrayFill => {
                Some(NativeState::TypedArray(Box::new(
                    typed_array::TypedArray::method(heap, state.diag.task, which, this, args)?,
                )))
            }
            Builtin::TypedArrayJoin => {
                let view = typed_array::receiver(heap, state.diag.task, this)?;
                if view.is_detached(heap)? {
                    return Err(Fault::TypeError("ArrayBuffer is detached".into()));
                }
                Some(NativeState::Join(Box::new(ArrayJoin {
                    this: this.clone(),
                    separator: arg(0),
                    stage: JoinStage::Separator,
                    len: view.length as u64,
                    next: 0,
                    parts: vec![],
                    units: 0,
                })))
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
        && !matches!(args.get(1), Some(Value::Object(_) | Value::BigInt(_)))
    {
        let desc = descriptor::DefineProperty::new(args)?;
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::DefineProperty(desc))?;
        return Ok(true);
    }
    start_builtin_conversion(heap, state, which, this, args, construct, popped)
}

#[inline(never)]
fn start_builtin_conversion(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    construct: bool,
    popped: usize,
) -> Result<bool, Fault> {
    use builtin::Builtin;
    let (target, operands, hint) = match which {
        Builtin::ArrayBuffer if construct => (
            ConvertTarget::ArrayBuffer(which),
            vec![
                args.first().cloned().unwrap_or(Value::Undefined),
                args.get(1).cloned().unwrap_or(Value::Undefined),
            ],
            Hint::Number,
        ),
        Builtin::ArrayBufferTransfer | Builtin::ArrayBufferSlice if !construct => {
            crate::object::array_buffer::receiver(heap, state.diag.task, this)?;
            // Slice rejects an already detached receiver before conversions.
            if which == Builtin::ArrayBufferSlice
                && matches!(
                    crate::object::array_buffer::inspect(heap, state.diag.task, this, true)?,
                    Value::Bool(true)
                )
            {
                return Err(Fault::TypeError("ArrayBuffer is detached".into()));
            }
            let mut operands = vec![this.clone()];
            if which == Builtin::ArrayBufferSlice {
                operands.push(args.first().cloned().unwrap_or(Value::Undefined));
                if let Some(end) = args.get(1).filter(|v| !matches!(v, Value::Undefined)) {
                    operands.push(end.clone());
                }
            } else if let Some(length) = args.first().filter(|v| !matches!(v, Value::Undefined)) {
                operands.push(length.clone());
            }
            (ConvertTarget::ArrayBuffer(which), operands, Hint::Number)
        }
        Builtin::Date => {
            if !construct {
                (ConvertTarget::DateFunction, vec![], Hint::Number)
            } else if args.is_empty() {
                (
                    ConvertTarget::DateNow { construct: true },
                    vec![],
                    Hint::Number,
                )
            } else if args.len() > 1 {
                let mut fields = vec![Value::Number(0.0); 7];
                fields[2] = Value::Number(1.0);
                for (field, arg) in fields.iter_mut().zip(args) {
                    *field = arg.clone();
                }
                (ConvertTarget::DateLocalConstructor, fields, Hint::Number)
            } else {
                let value = match super::date::time_value(heap, state.diag.task, &args[0])? {
                    Some(time) => Value::Number(time),
                    None => args[0].clone(),
                };
                (ConvertTarget::DateConstructor, vec![value], Hint::Default)
            }
        }
        Builtin::DateNow if !construct => (
            ConvertTarget::DateNow { construct: false },
            vec![],
            Hint::Number,
        ),
        Builtin::DateParse if !construct => (
            ConvertTarget::DateParse,
            vec![args.first().cloned().unwrap_or(Value::Undefined)],
            Hint::String,
        ),
        Builtin::DateToLocalString(part) if !construct => {
            super::date::this_time_value(heap, state.diag.task, this)?;
            (
                ConvertTarget::DateLocalString { part },
                vec![this.clone()],
                Hint::Number,
            )
        }
        Builtin::DateGetLocalField(_) | Builtin::DateTimezoneOffset if !construct => {
            let field = match which {
                Builtin::DateGetLocalField(f) => f,
                _ => 8,
            };
            super::date::this_time_value(heap, state.diag.task, this)?;
            (
                ConvertTarget::DateLocalGet { field },
                vec![this.clone()],
                Hint::Number,
            )
        }
        Builtin::DateUtc if !construct => {
            let mut fields = vec![Value::Number(0.0); 7];
            fields[0] = Value::Undefined;
            fields[2] = Value::Number(1.0);
            for (field, arg) in fields.iter_mut().zip(args) {
                *field = arg.clone();
            }
            (ConvertTarget::DateUtc, fields, Hint::Number)
        }
        Builtin::DateSetUtcField(first) | Builtin::DateSetLocalField(first) if !construct => {
            let time = super::date::this_time_value(heap, state.diag.task, this)?;
            let count = super::date::UTC_SETTERS[first as usize].1 as usize;
            let mut operands = vec![this.clone()];
            operands.extend(args.iter().take(count).cloned());
            if args.is_empty() {
                operands.push(Value::Undefined);
            }
            (
                if matches!(which, Builtin::DateSetLocalField(_)) {
                    ConvertTarget::DateSetLocalField { first, time }
                } else {
                    ConvertTarget::DateSetUtcField { first, time }
                },
                operands,
                Hint::Number,
            )
        }
        Builtin::DateSetTime if !construct => {
            super::date::this_time_value(heap, state.diag.task, this)?;
            (
                ConvertTarget::DateSetTime,
                vec![
                    this.clone(),
                    args.first().cloned().unwrap_or(Value::Undefined),
                ],
                Hint::Number,
            )
        }
        Builtin::BigIntAsIntN | Builtin::BigIntAsUintN if !construct => (
            ConvertTarget::BigIntAsN {
                signed: which == Builtin::BigIntAsIntN,
            },
            vec![
                args.first().cloned().unwrap_or(Value::Undefined),
                args.get(1).cloned().unwrap_or(Value::Undefined),
            ],
            Hint::Number,
        ),
        Builtin::NumberPrototypeToFixed if !construct => {
            let number = builtin::this_number_value(heap, state.diag.task, this, "toFixed")?;
            let Some(digits @ Value::Object(_)) = args.first() else {
                return Ok(false);
            };
            (
                ConvertTarget::NumberToFixed { number },
                vec![digits.clone()],
                Hint::Number,
            )
        }
        Builtin::NumberPrototypeToExponential | Builtin::NumberPrototypeToPrecision
            if !construct =>
        {
            let number = builtin::this_number_value(heap, state.diag.task, this, which.name())?;
            let Some(digits @ Value::Object(_)) = args.first() else {
                return Ok(false);
            };
            (
                ConvertTarget::NumberToPrecisionFormat { number, which },
                vec![digits.clone()],
                Hint::Number,
            )
        }
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
            let Some(value @ (Value::Object(_) | Value::BigInt(_))) = args.first() else {
                return Ok(false);
            };
            (
                ConvertTarget::SymbolRegistryKey,
                vec![value.clone()],
                Hint::String,
            )
        }
        Builtin::ObjectDefineProperty | Builtin::ObjectGetOwnPropertyDescriptor
            if !construct && matches!(args.get(1), Some(Value::Object(_) | Value::BigInt(_))) =>
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
            if !matches!(this, Value::Object(_) | Value::BigInt(_))
                && !matches!(pos, Value::Object(_))
            {
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
            if crate::regexp::object::data(heap, state.diag.task, &separator)?.is_some() {
                return Err(Fault::TypeError(
                    "RegExp separators in String.prototype.split are not supported".into(),
                ));
            }
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
        Builtin::SimSetTimeout | Builtin::SimClearTimeout => {
            let clear = which == Builtin::SimClearTimeout;
            let callback = if clear {
                Value::Undefined
            } else {
                let callback = args.first().cloned().unwrap_or(Value::Undefined);
                crate::host::sim::facilities::validate_callback(heap, &callback)?;
                callback
            };
            let delay = args
                .get(usize::from(!clear))
                .cloned()
                .unwrap_or(Value::Number(0.0));
            (
                ConvertTarget::SimTimer { clear },
                vec![callback, delay],
                Hint::Number,
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
    let (first, end) = if matches!(
        target,
        ConvertTarget::TypedArray {
            from_buffer: true,
            ..
        }
    ) || matches!(target, ConvertTarget::ArrayBuffer(which) if which != Builtin::ArrayBuffer)
    {
        (1, operands.len())
    } else if matches!(target, ConvertTarget::ArrayBuffer(Builtin::ArrayBuffer)) {
        (0, 1)
    } else if matches!(
        target,
        ConvertTarget::DateLocalGet { .. } | ConvertTarget::DateLocalString { .. }
    ) {
        (operands.len(), operands.len())
    } else if matches!(
        target,
        ConvertTarget::DateSetUtcField { .. } | ConvertTarget::DateSetLocalField { .. }
    ) {
        (1, operands.len())
    } else if matches!(
        target,
        ConvertTarget::PropertyBuiltin { .. }
            | ConvertTarget::DateSetTime
            | ConvertTarget::SimTimer { .. }
    ) {
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
            formatter: None,
            number_scan: None,
            target,
            operands,
            next: first,
            end,
            hint,
            method: 0,
            method_get: false,
            symbol_method: SymbolMethod::Pending,
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
        Instr::Rest(index) => (
            0,
            NativeState::Rest {
                values: state
                    .top()
                    .arguments
                    .as_ref()
                    .ok_or(Fault::Internal("rest arguments unavailable"))?
                    .iter()
                    .skip(usize::from(index))
                    .cloned()
                    .collect(),
                next: 0,
                out: Value::Undefined,
            },
        ),
        Instr::CopyDataProperties => {
            let [target, source] = operands::<2>(state)?;
            (
                2,
                NativeState::CopyProperties(copy_properties::State::new(
                    target.clone(),
                    source.clone(),
                )),
            )
        }
        Instr::ForInNext => (
            1,
            NativeState::Proxy(Box::new(super::proxy::State::new(
                super::proxy::Mode::ForIn(operands::<1>(state)?[0].clone()),
            ))),
        ),
        Instr::UsingDispose(count) | Instr::UsingDisposeAsync(count) => {
            use crate::object::disposable_stack::{Hint, Resource};
            let count = usize::from(count);
            let width = if matches!(instr, Instr::UsingDisposeAsync(_)) {
                3
            } else {
                2
            };
            let popped = width * count + 3;
            let values = dispatch::top_operands(state, popped)?;
            let resources = values[..width * count]
                .chunks_exact(width)
                .map(|pair| Resource {
                    value: pair[0].clone(),
                    method: pair[1].clone(),
                    hint: match pair.get(2) {
                        Some(Value::Number(1.0)) => Hint::Async,
                        Some(Value::Number(2.0)) => Hint::AsyncFallback,
                        _ => Hint::Sync,
                    },
                })
                .collect();
            let error = values[width * count]
                .truthy()
                .then(|| values[width * count + 1].clone());
            let cancelling = values[width * count + 2].truthy();
            let run = dispose::Run::new(
                resources,
                error,
                cancelling,
                state.error_stack(state.top().pc.saturating_sub(1))?,
            );
            (
                popped,
                NativeState::Disposal(Box::new(dispose::Stack::Run(run))),
            )
        }
        Instr::UsingCheck => (
            1,
            NativeState::Dispose(dispose::State::new(operands::<1>(state)?[0].clone())),
        ),
        Instr::UsingCheckAsync => (
            1,
            NativeState::Dispose(dispose::State::asynchronous(
                operands::<1>(state)?[0].clone(),
            )),
        ),
        Instr::GetAsyncIterator => (
            1,
            NativeState::Iterator(Box::new(iterator::State::get_async_iterator(
                operands::<1>(state)?[0].clone(),
            ))),
        ),
        Instr::GetIterator => (
            1,
            NativeState::Iterator(Box::new(iterator::State::get_iterator(
                operands::<1>(state)?[0].clone(),
            ))),
        ),
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
            | Action::SuspendAsync(_)
            | Action::ResumeAsync { .. }
            | Action::ResumeGenerator { .. }
            | Action::AwaitGeneratorReturn { .. }
            | Action::Eval(..)
            | Action::DynamicFunction(..)
            | Action::SimTimer { .. }
            | Action::SetProperty { .. }
            | Action::CreateDataProperty { .. }
            | Action::DefineProperty(_)
            | Action::Instruction { .. }
            | Action::Call { .. }
            | Action::Construct { .. }
            | Action::InitializeBuiltin { .. }
            | Action::ToNumber(_)
            | Action::ToPrimitiveNumber(_)
            | Action::DefaultLocale
            | Action::ToNumberLimited(_, _)
            | Action::ToString(_)
            | Action::Builtin { .. }
            | Action::Need { .. }
            | Action::Return(_)
            | Action::ReturnPair(..)
            | Action::Await(_)
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
pub(crate) fn discard(natives: &mut Vec<NativeFrame>, frame: usize, heap: &Heap) {
    let kept = natives.partition_point(|n| n.depth <= frame);
    for native in &natives[kept..] {
        if let NativeState::Generator { id, .. } = native.state {
            super::generator::complete(heap, id);
        }
        if let NativeState::AsyncGenerator(a) = &native.state
            && let Some(id) = a.id
        {
            super::generator::complete(heap, id);
        }
    }
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
    if !state.natives.is_empty() {
        state.pop_native_catch(state.natives.len() - 1);
    }
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
    let native_index = state.natives.len() - 1;
    // These steps begin without a delivered operand, so the handler's
    // height is exactly the native base. Other boundaries catch only calls.
    if match &state.natives[native_index].state {
        NativeState::AsyncGenerator(a) => a.catches_step(),
        NativeState::ArrayFrom(from) => from.catches_step(),
        NativeState::AddEntries(entries) => entries.catches_step(),
        NativeState::ListFormat(format) => format.catches_step(),
        NativeState::MathSumPrecise(sum) => sum.catches_step(),
        NativeState::Promise(promise) => promise.catches(),
        _ => false,
    } {
        state.push_handler(super::exception::HandlerKind::NativeCatch {
            native: native_index,
        })?;
    }
    let frame = state
        .natives
        .last_mut()
        .ok_or(Fault::Internal("no running native frame"))?;
    frame.awaiting = Awaiting::Nothing;
    let action = {
        let _borrow = scoped_borrow();
        // Eval's recursive compiler uses a separate bounded stack. Avoid
        // entering the large general native dispatcher on small VM workers.
        if let NativeState::Eval(eval) = &mut frame.state {
            eval.step()?
        } else if let NativeState::DynamicFunction(function) = &mut frame.state {
            function.step(input)?
        } else if let NativeState::Instanceof { value, target } = &frame.state {
            step_instanceof(heap, state.diag.task, value, target, input)?
        } else {
            frame
                .state
                .step(heap, state.diag.task, &state.jobs, input, host)?
        }
    };
    state.pop_native_catch(native_index);
    state.truncate_operands(base)?;
    // Constructor entry can start another conversion native. Do not retain
    // apply_action's unrelated call/async temporaries on small worker stacks.
    if let Action::InitializeBuiltin {
        which,
        args,
        prototype,
    } = action
    {
        set_awaiting(state, Awaiting::CallResult)?;
        initialize_builtin(heap, state, which, &args, prototype)?;
        return Ok(Flow::Next);
    }
    apply_action(state, heap, host, action)
}

// Conversions can traverse properties and numeric string parsing. Keep the
// action dispatcher's call/activation temporaries off that Rust stack path.
#[inline(never)]
fn apply_action(
    state: &mut VmState,
    heap: &Heap,
    host: &mut dyn Host,
    action: Action,
) -> Result<Flow, Fault> {
    match action {
        Action::SuspendAsync(source) => {
            state.natives.pop();
            if let Some(NativeFrame {
                depth,
                state: NativeState::AsyncGenerator(a),
                ..
            }) = state.natives.last()
                && *depth == state.depth()
                && let Some(id) = a.id
            {
                return super::async_generator::suspend(heap, state, source, id);
            }
            super::asynchronous::suspend(heap, state, source)
        }
        Action::ResumeGenerator { id, value, kind } => {
            generator_awaiting(state)?;
            if !super::generator::resume(heap, state, id, value, kind)? {
                return Err(Fault::Internal("async generator unexpectedly completed"));
            }
            Ok(Flow::FrameChanged)
        }
        Action::AwaitGeneratorReturn { value } => {
            state.push_handler(super::exception::HandlerKind::NativeCatch {
                native: state.natives.len() - 1,
            })?;
            await_promise(
                state,
                super::promise::await_state(heap, state.diag.task, value)?,
            )?;
            Ok(Flow::FrameChanged)
        }
        Action::ResumeAsync {
            id,
            rejected,
            value,
        } => {
            state.natives.pop();
            super::asynchronous::resume(heap, state, id, rejected, value)
        }
        Action::DynamicFunction(executable) => {
            state.natives.pop();
            let function =
                dispatch::new_function(heap, executable.0, Box::new([]), state.diag.task, None)?;
            state.push(function);
            Ok(Flow::Next)
        }
        Action::Eval(executable, captures) => {
            state.natives.pop();
            let code = executable.0;
            let function = dispatch::new_function(
                heap,
                code.clone(),
                captures.into_boxed_slice(),
                state.diag.task,
                None,
            )?;
            let Value::Object(id) = function else {
                return Err(Fault::Internal("eval function is not an object"));
            };
            state.push(function);
            state.enter(code, id, Value::Undefined, 0)?;
            state.top_mut().eval = true;
            Ok(Flow::FrameChanged)
        }
        Action::SimTimer {
            callback,
            delay,
            clear,
        } => {
            let host = state.sim_host.as_mut().ok_or(Fault::TypeError(
                "simulated host bindings are not installed".into(),
            ))?;
            let value = host.timer(callback, delay, clear)?;
            state.natives.pop();
            state.push(value);
            Ok(Flow::Next)
        }
        Action::DefaultLocale => {
            let locale = crate::intl::default_locale(Some(host));
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(Value::String(JsString::from(locale.as_str())));
            Ok(Flow::Next)
        }
        Action::SetProperty {
            this,
            key,
            value,
            strict,
        } => {
            set_awaiting(state, Awaiting::CallResult)?;
            if proxy_write(heap, state, &this, &key, &value, strict)? {
                return Ok(Flow::Next);
            }
            if !typed_array_write(heap, state, &this, &key, &value)? {
                let setter = property::prepare_put(
                    heap,
                    state.diag.task,
                    &this,
                    &key,
                    value.clone(),
                    strict,
                )?;
                write_result(state, setter, this, value)?;
            }
            Ok(Flow::Next)
        }
        Action::Instruction { instr, operands } => {
            if state
                .natives
                .last()
                .is_some_and(|frame| frame.state.catches())
            {
                state.push_handler(super::exception::HandlerKind::NativeCatch {
                    native: state.natives.len() - 1,
                })?;
            }
            set_awaiting(state, Awaiting::CallResult)?;
            for value in operands {
                state.push(value);
            }
            Ok(Flow::Resume(instr))
        }
        Action::CreateDataProperty { this, key, value } => {
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(this);
            state.push(key);
            state.push(value);
            Ok(Flow::Resume(Instr::DefineElem))
        }
        Action::DefineProperty(descriptor) => {
            set_awaiting(state, Awaiting::CallResult)?;
            push_native(state, NativeState::DefineProperty(*descriptor))?;
            Ok(Flow::Next)
        }
        Action::ToNumber(_)
        | Action::ToNumberLimited(_, _)
        | Action::ToString(_)
        | Action::ToPrimitiveNumber(_) => {
            let (value, target, hint) = match action {
                Action::ToPrimitiveNumber(value) => {
                    (value, ConvertTarget::PrimitiveValue, Hint::Number)
                }
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
                    formatter: None,
                    number_scan: None,
                    target,
                    operands: vec![value],
                    next: 0,
                    end: 1,
                    hint,
                    method: 0,
                    method_get: false,
                    symbol_method: SymbolMethod::Pending,
                },
            )?;
            Ok(Flow::Next)
        }
        Action::Builtin { which, args } => {
            state.natives.pop();
            if !start_builtin(heap, state, which, &Value::Undefined, &args, false, 0)? {
                let result = builtin::call_in_state(heap, state, which, &Value::Undefined, &args)?;
                state.push(result);
            }
            Ok(Flow::Next)
        }
        Action::Call { callee, this, args } => {
            if state
                .natives
                .last()
                .is_some_and(|frame| frame.state.catches())
            {
                state.push_handler(super::exception::HandlerKind::NativeCatch {
                    native: state.natives.len() - 1,
                })?;
            }
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
                        return Ok(Flow::FrameChanged);
                    }
                    // Runs to completion (`crate::vm::builtin`); no safepoint
                    // falls between here and the push that roots its result.
                    // The next step receives it like a callback's return.
                    let result =
                        builtin::call_in_state(heap, state, which, &call.this, &call.args)?;
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
        Action::Construct { callee, args } => {
            let argc = u16::try_from(args.len())
                .map_err(|_| Fault::StackOverflow("native constructor arguments".into()))?;
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(callee);
            args.into_iter().for_each(|value| state.push(value));
            Ok(Flow::Resume(Instr::Construct(argc)))
        }
        Action::InitializeBuiltin { .. } => {
            unreachable!("constructor entered before action dispatch")
        }
        Action::Continue => Ok(Flow::Next),
        Action::Need { dependency, kind } => {
            set_awaiting(state, Awaiting::Dependency)?;
            Ok(Flow::Need(dependency, kind))
        }
        Action::Await(value) => {
            state.push_handler(super::exception::HandlerKind::NativeCatch {
                native: state.natives.len() - 1,
            })?;
            set_awaiting(state, Awaiting::CallResult)?;
            state.push(value);
            // Execute the ordinary await only after dropping this large
            // dispatch frame, as for resumable conversions. Keep the disposal
            // frame and its catch handler inside the activation being saved.
            Ok(Flow::Resume(Instr::Await))
        }
        Action::ReturnPair(first, second) => {
            state.natives.pop();
            state.push(first);
            state.push(second);
            Ok(Flow::Next)
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
            Ok(Flow::Resume(instr))
        }
        Action::Unwind => Err(Fault::Internal("a native step unwound without a failure")),
    }
}

/// Native boundaries receive exceptions after JavaScript's own catch/finally
/// handlers have run. Generators and Array.from retain the full diagnostic
/// for rethrowing or IteratorClose; promises consume the value. Cancellation
/// never calls this path.
pub(crate) fn catch(
    natives: &mut Vec<NativeFrame>,
    index: usize,
    exception: super::exception::Exception,
) -> Result<(), &'static str> {
    natives.truncate(index + 1);
    let frame = natives.get_mut(index).ok_or("native catch frame missing")?;
    match &mut frame.state {
        NativeState::Async(a) => a.failed = Some(exception.value),
        NativeState::AsyncGenerator(a) => a.failed = Some(exception.value),
        NativeState::Disposal(disposal) if disposal.catches() => disposal.failed(exception),
        NativeState::Promise(promise) if promise.catches() => promise.failed(exception.value),
        NativeState::ArrayFrom(from) if from.catches() => from.failed(exception),
        NativeState::AddEntries(entries) if entries.catches() => entries.failed(exception),
        NativeState::ListFormat(format) if format.catches() => format.failed(exception),
        NativeState::MathSumPrecise(sum) if sum.catches() => sum.failed(exception),
        NativeState::Generator { failed, .. } => *failed = Some(exception),
        _ => return Err("invalid native catch"),
    }
    frame.awaiting = Awaiting::Nothing;
    Ok(())
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

/// Operands whose primitive completion is ToString rather than ToNumber.
fn string_operand(target: ConvertTarget, index: usize) -> bool {
    use ConvertTarget::*;
    match target {
        StringValue | SymbolDescription | SymbolRegistryKey | DateParse => true,
        PrimitiveConstructor { which, .. } => {
            matches!(which, builtin::Builtin::String | builtin::Builtin::Symbol)
        }
        Instruction(Instr::ToString | Instr::ToSuperKey) => true,
        Instruction(Instr::PutSuperElem | Instr::PutSuperElemStrict) => index == 2,
        Instruction(
            Instr::GetElem
            | Instr::ToPropertyKey
            | Instr::Delete
            | Instr::DeleteStrict
            | Instr::PutElem
            | Instr::PutElemStrict,
        ) => index == 1,
        Instruction(Instr::In) | NumberParse { .. } => index == 0,
        PropertyBuiltin { .. } => index == 1,
        StringSplit { .. } => index != 1,
        StringBuiltin { which, .. } => builtin::string_operand_is_text(which, index),
        _ => false,
    }
}

// Keep the scanner's large construction temporaries off the conversion
// trampoline's Rust frame, which also serves calls on 64 KiB worker stacks.
#[inline(never)]
fn string_code_number_scan(
    operand: &mut Value,
) -> Result<Box<crate::value::string::StringToNumber>, Fault> {
    // Transfer ownership so flattening releases a unique rope in slices,
    // rather than retaining it until the entire scan has finished.
    let Value::String(text) = std::mem::replace(operand, Value::Undefined) else {
        unreachable!("string operand checked by caller")
    };
    Ok(Box::new(
        crate::value::string::StringToNumber::new(text)
            .ok_or(Fault::OutOfMemory("string code numeric input"))?,
    ))
}

/// Parse a string only after both operands have passed through ToPrimitive.
/// Invalid strings become NaN, which is unequal/unordered for these operations.
fn bigint_comparison(instr: Instr, operands: &[Value]) -> Option<NativeState> {
    if !matches!(
        instr,
        Instr::CompareLt | Instr::CompareGt | Instr::CompareLe | Instr::CompareGe | Instr::LooseEq
    ) {
        return None;
    }
    let index = match operands {
        [Value::String(_), Value::BigInt(_)] => 0,
        [Value::BigInt(_), Value::String(_)] => 1,
        _ => return None,
    };
    let Value::String(text) = &operands[index] else {
        unreachable!()
    };
    Some(NativeState::BigIntParse {
        parser: Box::new(crate::value::bigint::StringToBigInt::new(text.clone())),
        instr: Some(instr),
        operands: operands.to_vec(),
        index,
        wrap: None,
    })
}

fn step_instanceof(
    heap: &Heap,
    task: TaskId,
    value: &Value,
    target: &Value,
    input: Input,
) -> Result<Action, Fault> {
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
        Input::Dependency(_) => return Err(Fault::Internal("instanceof received a dependency")),
    };
    prototype::instance_of_prototype(heap, task, value, &proto)
        .map(|yes| Action::Return(Value::Bool(yes)))
}

impl NativeState {
    fn catches(&self) -> bool {
        match self {
            Self::Async(_) => true,
            Self::AsyncGenerator(a) => a.catches(),
            Self::Disposal(disposal) => disposal.catches(),
            Self::Promise(promise) => promise.catches(),
            Self::ArrayFrom(from) => from.catches(),
            Self::AddEntries(entries) => entries.catches(),
            Self::ListFormat(format) => format.catches(),
            Self::MathSumPrecise(sum) => sum.catches(),
            _ => false,
        }
    }
    /// One step: consume `input`, return the next action. Anything allocated
    /// is stored in `self` (a root) before returning.
    #[inline(never)]
    fn step(
        &mut self,
        heap: &Heap,
        task: TaskId,
        jobs: &std::sync::Weak<crate::job::JobInbox>,
        input: Input,
        host: &mut dyn Host,
    ) -> Result<Action, Fault> {
        match self {
            NativeState::BuiltinConstruct(constructor) => constructor.step(heap, task, input),
            NativeState::RegExp(regexp) => regexp.step(heap, task, input),
            NativeState::Intl(intl) => intl.step(heap, task, input),
            NativeState::NumberFormat(format) => format.step(heap, task, input),
            NativeState::PluralRules(rules) => rules.step(heap, task, input),
            NativeState::RelativeTimeFormat(format) => format.step(heap, task, input),
            NativeState::DisplayNames(names) => names.step(heap, task, input),
            NativeState::Collator(collator) => collator.step(heap, task, input),
            NativeState::DateTimeFormat(format) => format.step(heap, task, input, host),
            NativeState::ListFormat(format) => format.step(heap, task, input),
            NativeState::Segmenter(segmenter) => segmenter.step(heap, task, input),
            NativeState::Convert { .. } => self.step_conversion(heap, task, input, host),
            // Iterator protocol reads can descend through descriptor and
            // conversion helpers. Keep the unrelated native arms' stack
            // storage out of spread execution on 64 KiB workers.
            NativeState::Iterator(iterator) => iterator.step(heap, task, input),
            NativeState::AddEntries(entries) => entries.step(heap, task, input),
            NativeState::Dispose(dispose) => dispose.step(heap, task, input),
            NativeState::Disposal(disposal) => disposal.step(heap, task, input),
            NativeState::Promise(promise) => promise.step(heap, task, jobs, input),
            NativeState::AsyncGenerator(generator) => generator.step(heap, task, input),
            NativeState::DurationFormat(format) => format.step(heap, task, input),
            NativeState::ArrayMutate(array) => array.step(heap, task, input),
            NativeState::ArraySearch(array) => array.step(heap, task, input),
            NativeState::ArrayCallbacks(array) => array.step(heap, task, input),
            NativeState::ArrayCopy(array) => array.step(heap, task, input),
            NativeState::ArraySort(array) => array.step(heap, task, input),

            NativeState::CopyProperties(copy) => copy.step(heap, task, input),
            NativeState::ObjectStatics(object) => object.step(heap, task, input),
            NativeState::Rest { .. } => self.step_rest(heap, task),
            _ => self.step_other(heap, task, input, host),
        }
    }

    #[inline(never)]
    fn step_rest(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let NativeState::Rest { values, next, out } = self else {
            return Err(Fault::Internal("expected rest state"));
        };
        if matches!(out, Value::Undefined) {
            *out = property::new_array(heap, task)?;
            return Ok(Action::Continue);
        }
        if let Some(value) = values.get_mut(*next) {
            property::append(
                heap,
                task,
                out,
                Some(std::mem::replace(value, Value::Undefined)),
            )?;
            *next += 1;
            Ok(Action::Continue)
        } else {
            Ok(Action::Return(out.clone()))
        }
    }

    fn step_other(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
        host: &mut dyn Host,
    ) -> Result<Action, Fault> {
        match self {
            NativeState::BuiltinConstruct(_) => unreachable!("constructor dispatched separately"),
            NativeState::Async(a) => a.step(heap, input),
            NativeState::CopyProperties(_)
            | NativeState::Rest { .. }
            | NativeState::ObjectStatics(_) => {
                Err(Fault::Internal("copy native bypassed dispatch"))
            }
            NativeState::AsyncGenerator(_) => {
                unreachable!("async generator dispatched before other native arms")
            }
            NativeState::Eval(eval) => eval.step(),
            NativeState::DynamicFunction(function) => function.step(input),
            NativeState::Generator { id, failed } => {
                if let Some(error) = failed.take() {
                    super::generator::complete(heap, *id);
                    return Err(Fault::Rethrow(error));
                }
                let Input::CallResult(value) = input else {
                    return Err(Fault::Internal("generator expected result"));
                };
                let (yielded, delegated) = heap
                    .read(*id, |o| match o {
                        crate::heap::HeapObject::Generator(g) => match g.state {
                            super::generator::State::SuspendedYield(_) => (true, false),
                            super::generator::State::SuspendedYieldStar(_) => (true, true),
                            _ => (false, false),
                        },
                        _ => (false, false),
                    })
                    .unwrap_or((false, false));
                if delegated {
                    return Ok(Action::Return(value));
                }
                if !yielded {
                    super::generator::complete(heap, *id);
                }
                Ok(Action::Return(super::generator::result(
                    heap, task, value, !yielded,
                )?))
            }
            NativeState::BigIntFormat(formatter) => Ok(match formatter.advance() {
                Some(text) => Action::Return(Value::String(text)),
                None => Action::Continue,
            }),
            NativeState::BigIntParse {
                parser,
                instr,
                operands,
                index,
                wrap,
            } => {
                use crate::value::bigint::BigIntError;
                let Some(result) = parser.advance() else {
                    return Ok(Action::Continue);
                };
                let value = match result {
                    Ok(n) => Value::BigInt(n),
                    Err(BigIntError::InvalidDigits) if instr.is_some() => Value::Number(f64::NAN),
                    Err(BigIntError::InvalidDigits) => {
                        return Err(Fault::SyntaxError("invalid BigInt string".into()));
                    }
                    Err(error) => return Err(Fault::RangeError(error.to_string())),
                };
                if let Some(instr) = instr {
                    operands[*index] = value;
                    Ok(Action::Resume {
                        instr: *instr,
                        operands: std::mem::take(operands),
                    })
                } else {
                    let value = if let Some((bits, signed)) = wrap {
                        builtin::bigint_as_n(*bits, &value, *signed)?
                    } else {
                        value
                    };
                    Ok(Action::Return(value))
                }
            }
            NativeState::TypedArray(array) => array.step(heap, task, input),
            NativeState::Iterator(_) | NativeState::Dispose(_) | NativeState::Disposal(_) => {
                unreachable!("iterator dispatched before other native arms")
            }
            NativeState::ArrayFrom(from) => from.step(heap, task, input),
            NativeState::AddEntries(_) => {
                unreachable!("entry consumer dispatched before other native arms")
            }
            NativeState::MathSumPrecise(sum) => sum.step(heap, task, input),
            NativeState::ArraySort(_) => {
                unreachable!("array sort dispatched before other native arms")
            }
            NativeState::ArraySearch(_) => {
                unreachable!("array search dispatched before other native arms")
            }
            NativeState::ArrayCallbacks(_) => {
                unreachable!("array callbacks dispatched before other native arms")
            }
            NativeState::ArrayCopy(_) => {
                unreachable!("array copy dispatched before other native arms")
            }
            NativeState::ArrayMutate(_) => {
                unreachable!("array mutator dispatched before other native arms")
            }
            NativeState::Promise(_) => unreachable!("promise dispatched before other native arms"),
            NativeState::Intl(_)
            | NativeState::NumberFormat(_)
            | NativeState::PluralRules(_)
            | NativeState::RelativeTimeFormat(_)
            | NativeState::DisplayNames(_)
            | NativeState::DurationFormat(_)
            | NativeState::Collator(_)
            | NativeState::DateTimeFormat(_)
            | NativeState::ListFormat(_)
            | NativeState::Segmenter(_) => unreachable!("Intl dispatched before other native arms"),
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
                    StringifyStep::Native(action) => action,
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
                        .ok_or(Fault::OutOfMemory("join output buffer"))?,
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
            NativeState::RegExp(_) => unreachable!("RegExp dispatched before other native arms"),
            NativeState::StringReplace(replace) => replace.step(heap, task, input),
            NativeState::StringMethods(method) => method.step(heap, task, input),
            NativeState::Bind {
                target,
                args,
                length,
                stage,
            } => {
                if *stage == 0 || *stage == 3 {
                    dispatch::callee(heap, target)?;
                    let Value::Object(id) = target else {
                        unreachable!()
                    };
                    let key = property::Key::Name(JsString::from("length"));
                    if *stage == 0 && super::proxy::inspect(heap, task, *id)?.is_some() {
                        *stage = 3;
                        let object = crate::realm::intrinsics(heap)
                            .ok_or(Fault::OutOfMemory("realm allocation"))?
                            .global(crate::realm::Global::Object);
                        let callee = property::get(
                            heap,
                            task,
                            &object,
                            &property::Key::Name(JsString::from("getOwnPropertyDescriptor")),
                        )?;
                        return Ok(Action::Call {
                            callee,
                            this: object,
                            args: vec![target.clone(), Value::String(JsString::from("length"))],
                        });
                    }
                    let has_length = if *stage == 3 {
                        match &input {
                            Input::CallResult(v) => *v != Value::Undefined,
                            _ => return Err(Fault::Internal("bind expected descriptor")),
                        }
                    } else {
                        property::get_own_property(heap, task, *id, &key)?.is_some()
                    };
                    *stage = 1;
                    if has_length {
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
                if *next < args.len() {
                    let key = index_key(len.unwrap() + *next as u64);
                    let item = args[*next].clone();
                    *next += 1;
                    return Ok(Action::SetProperty {
                        this: this.clone(),
                        key,
                        value: item,
                        strict: true,
                    });
                }
                let length = Value::Number((len.unwrap() + args.len() as u64) as f64);
                *finishing = true;
                Ok(Action::SetProperty {
                    this: this.clone(),
                    key: property::Key::Name(JsString::from("length")),
                    value: length,
                    strict: true,
                })
            }
            NativeState::Instanceof { value, target } => {
                step_instanceof(heap, task, value, target, input)
            }
            NativeState::Proxy(proxy) => proxy.step(heap, task, input),
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
                    descriptor::DefineStep::TypedArrayValue {
                        target,
                        result,
                        index,
                        value,
                    } => {
                        *self = NativeState::Convert {
                            formatter: None,
                            number_scan: None,
                            target: ConvertTarget::TypedArrayWrite,
                            operands: vec![target, result, Value::Number(index), value],
                            next: 3,
                            end: 4,
                            hint: Hint::Number,
                            method: 0,
                            method_get: false,
                            symbol_method: SymbolMethod::Pending,
                        };
                        Ok(Action::Continue)
                    }
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
            NativeState::Convert { .. } => self.step_conversion(heap, task, input, host),
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
                    *parser = Some(Box::new(super::json::Parser::new(input)?));
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
            NativeState::DateParse { parser, construct } => {
                let Some(parsed) = parser.advance(256) else {
                    return Ok(Action::Continue);
                };
                let time = parsed.map_or(f64::NAN, |fields| {
                    let zone = if fields.is_local {
                        super::date::zone(heap, host)
                    } else {
                        crate::host::TimeZone::UTC
                    };
                    fields.time(zone)
                });
                if *construct {
                    super::date::construct(heap, task, time).map(Action::Return)
                } else {
                    Ok(Action::Return(Value::Number(time)))
                }
            }
        }
    }

    /// Receive a failure an exception carries through this native and
    /// answer it (module docs, "Errors"). Neither operation recovers: the
    /// fold's accumulator and `needAll`'s results so far are abandoned with
    /// the frame, and the exception goes on to the caller's handler, which
    /// is what JavaScript's `Array.prototype.reduce` does with a throwing
    /// callback. Allocates nothing and touches no heap object.
    // Keep conversion callbacks off the large general-native dispatch frame.
    // This path must execute on 64 KiB worker stacks in debug builds.
    #[inline(never)]
    fn step_conversion(
        &mut self,
        heap: &Heap,
        task: TaskId,
        input: Input,
        host: &mut dyn Host,
    ) -> Result<Action, Fault> {
        match self {
            NativeState::Convert {
                formatter,
                number_scan,
                target,
                operands,
                next,
                end,
                hint,
                method,
                method_get,
                symbol_method,
            } => {
                if let Some(scan) = number_scan {
                    let Some(number) = scan.advance(256) else {
                        return Ok(Action::Continue);
                    };
                    operands[*next] = Value::Number(number);
                    *number_scan = None;
                }
                if let Some(pending) = formatter {
                    let Some(text) = pending.advance() else {
                        return Ok(Action::Continue);
                    };
                    operands[*next] = Value::String(text);
                    *formatter = None;
                }
                match input {
                    Input::Start => {}
                    Input::CallResult(v) => {
                        if matches!(symbol_method, SymbolMethod::Getter) {
                            return call_symbol_method(
                                heap,
                                symbol_method,
                                v,
                                operands[*next].clone(),
                                *hint,
                            );
                        }
                        if matches!(symbol_method, SymbolMethod::Call) {
                            if matches!(v, Value::Object(_)) {
                                return Err(Fault::TypeError(
                                    "cannot convert object to primitive value".into(),
                                ));
                            }
                            operands[*next] = v;
                            *symbol_method = SymbolMethod::Pending;
                            *method = 0;
                        } else if *method_get {
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
                            *symbol_method = SymbolMethod::Pending;
                        }
                    }
                    Input::Dependency(_) => {
                        return Err(Fault::Internal("conversion received a dependency"));
                    }
                }
                while *next < *end {
                    let receiver = &operands[*next];
                    if let Value::BigInt(value) = receiver
                        && string_operand(*target, *next)
                    {
                        *formatter = Some(Box::new(crate::value::bigint::BigIntToString::new(
                            value, 10,
                        )));
                        return Ok(Action::Continue);
                    }
                    if !matches!(receiver, Value::Object(_)) {
                        if matches!(target, ConvertTarget::BigIntAsN { .. }) && *next == 0 {
                            // ToIndex must finish (and reject) before any
                            // observable ToBigInt conversion of the value.
                            operands[0] = Value::Number(dispatch::to_index(receiver)? as f64);
                            *next += 1;
                            continue;
                        }
                        if let ConvertTarget::TypedArray { kind, from_buffer } = target {
                            let number =
                                Value::Number(crate::object::typed_array::to_number(receiver)?);
                            if *from_buffer && *next == 1 {
                                crate::object::typed_array::validate_offset(*kind, &number)?;
                            } else {
                                crate::object::typed_array::to_index(&number)?;
                            }
                            operands[*next] = number;
                            *next += 1;
                            continue;
                        }
                        if let ConvertTarget::ArrayBuffer(which) = target {
                            let number = Value::Number(dispatch::to_number(receiver)?);
                            // Validate ToIndex now, before any later conversion.
                            if *which != builtin::Builtin::ArrayBufferSlice {
                                crate::object::array_buffer::to_index(&number)?;
                            }
                            operands[*next] = number;

                            *next += 1;
                            continue;
                        }
                        if matches!(
                            target,
                            ConvertTarget::DateUtc
                                | ConvertTarget::DateLocalConstructor
                                | ConvertTarget::DateSetLocalField { .. }
                                | ConvertTarget::DateSetTime
                                | ConvertTarget::DateSetUtcField { .. }
                        ) {
                            // Finish each ToNumber before observing the next argument.
                            // Even NaN must not skip later coercion side effects.
                            operands[*next] =
                                Value::Number(super::date::number_argument(receiver)?);
                            *next += 1;
                            continue;
                        }
                        // Numeric binary operators complete ToNumeric on
                        // each operand in order. A symbol on the left must
                        // throw before any right-hand conversion callback.
                        // Plus and comparisons instead need both primitives.
                        if matches!(target, ConvertTarget::Instruction(Instr::ToNumberPair)) {
                            if !matches!(receiver, Value::BigInt(_)) {
                                operands[*next] = Value::Number(dispatch::to_number(receiver)?);
                            }
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
                            let codes = matches!(
                                which,
                                builtin::Builtin::StringFromCharCode
                                    | builtin::Builtin::StringFromCodePoint
                            );
                            if codes && matches!(receiver, Value::String(_)) {
                                // Reuse the strict scanner's budgeted rope
                                // flattening, trimming, validation and parsing.
                                *number_scan = Some(string_code_number_scan(&mut operands[*next])?);
                                return Ok(Action::Continue);
                            }
                            operands[*next] =
                                builtin::string_builtin_operand(which, *next, receiver)?;
                            if codes {
                                // Validate this point before later callbacks,
                                // and bound even primitive-only argument loops.
                                *next += 1;
                                return Ok(Action::Continue);
                            }
                            if which == builtin::Builtin::StringPrototypeConcat {
                                let text = dispatch::to_js_string(&operands[*next])?;
                                *concat_len = concat_len
                                    .checked_add(text.len())
                                    .filter(|n| *n <= crate::value::string::MAX_STRING_LEN)
                                    .ok_or(Fault::StringTooLong)?;
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
                    if matches!(symbol_method, SymbolMethod::Pending) {
                        *symbol_method = SymbolMethod::Ordinary;
                        if matches!(target, ConvertTarget::Instruction(Instr::ToSuperKey))
                            || matches!(
                                builtin::wrapper_data(heap, task, receiver)?,
                                Some(crate::object::PrimitiveData::Symbol(_))
                            )
                        {
                            let key = property::Key::Symbol(crate::value::symbol::well_known(
                                "toPrimitive",
                            ));
                            match property::read(heap, task, receiver, &key)? {
                                Some(property::Read::Getter(callee)) => {
                                    *symbol_method = SymbolMethod::Getter;
                                    return Ok(Action::Call {
                                        callee,
                                        this: receiver.clone(),
                                        args: vec![],
                                    });
                                }
                                Some(property::Read::Value(callee)) => {
                                    return call_symbol_method(
                                        heap,
                                        symbol_method,
                                        callee,
                                        receiver.clone(),
                                        *hint,
                                    );
                                }
                                None => {}
                            }
                        }
                    }
                    while *method < 2 {
                        if matches!(hint, Hint::Default)
                            && super::date::time_value(heap, task, receiver)?.is_some()
                        {
                            return Err(Fault::Unsupported(
                                "Date default conversion requires formatting/Symbol.toPrimitive support".into(),
                            ));
                        }
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
                    ConvertTarget::DateFunction => {
                        let time = super::date::now(host)?;
                        let zone = super::date::zone(heap, host);
                        Ok(Action::Return(super::date::format_string(
                            time,
                            Some(zone),
                            3,
                        )))
                    }
                    ConvertTarget::DateLocalString { part } => {
                        let time = super::date::this_time_value(heap, task, &operands[0])?;
                        let zone = super::date::zone(heap, host);
                        Ok(Action::Return(super::date::format_string(
                            time,
                            Some(zone),
                            part,
                        )))
                    }
                    ConvertTarget::DateNow { construct } => {
                        let time = super::date::now(host)?;
                        if construct {
                            super::date::construct(heap, task, time).map(Action::Return)
                        } else {
                            Ok(Action::Return(Value::Number(time)))
                        }
                    }
                    ConvertTarget::DateLocalGet { field } => {
                        let zone = super::date::zone(heap, host);
                        super::date::get_local_field(heap, task, &operands[0], field, zone)
                            .map(Action::Return)
                    }
                    ConvertTarget::DateLocalConstructor => {
                        let zone = super::date::zone(heap, host);
                        let time = zone.utc_time_millis(super::date::fields_time(operands)?);
                        super::date::construct(heap, task, time).map(Action::Return)
                    }
                    ConvertTarget::DateSetLocalField { first, time } => {
                        let zone = super::date::zone(heap, host);
                        super::date::set_local_field(
                            heap,
                            task,
                            &operands[0],
                            first,
                            time,
                            &operands[1..],
                            zone,
                        )
                        .map(Action::Return)
                    }
                    ConvertTarget::SimTimer { clear } => Ok(Action::SimTimer {
                        callback: operands[0].clone(),
                        delay: dispatch::to_number(&operands[1])?,
                        clear,
                    }),
                    ConvertTarget::DateConstructor => {
                        if let Value::String(text) = &operands[0] {
                            *self = NativeState::DateParse {
                                parser: Box::new(super::date::Parser::new(text.clone())?),
                                construct: true,
                            };
                            Ok(Action::Continue)
                        } else {
                            super::date::construct(heap, task, dispatch::to_number(&operands[0])?)
                                .map(Action::Return)
                        }
                    }
                    ConvertTarget::DateParse => {
                        let text = dispatch::to_js_string(&operands[0])?;
                        *self = NativeState::DateParse {
                            parser: Box::new(super::date::Parser::new(text)?),
                            construct: false,
                        };
                        Ok(Action::Continue)
                    }
                    ConvertTarget::DateUtc => super::date::utc(operands).map(Action::Return),
                    ConvertTarget::DateSetUtcField { first, time } => super::date::set_utc_field(
                        heap,
                        task,
                        &operands[0],
                        first,
                        time,
                        &operands[1..],
                    )
                    .map(Action::Return),
                    ConvertTarget::DateSetTime => super::date::set_time(
                        heap,
                        task,
                        &operands[0],
                        dispatch::to_number(&operands[1])?,
                    )
                    .map(Action::Return),
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
                                "Number conversion limit of {limit} UTF-16 units exceeded"
                            )));
                        }
                        dispatch::to_number(&operands[0]).map(|n| Action::Return(Value::Number(n)))
                    }
                    ConvertTarget::PrimitiveValue => Ok(Action::Return(operands[0].clone())),
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
                    ConvertTarget::Instruction(instr) => {
                        // `+` first obtains both primitives with hint default.
                        // Only a string primitive then selects ToString.
                        if instr == Instr::Plus
                            && operands.iter().any(|v| matches!(v, Value::String(_)))
                            && let Some(index) =
                                operands.iter().position(|v| matches!(v, Value::BigInt(_)))
                            && let Value::BigInt(value) = &operands[index]
                        {
                            *next = index;
                            *formatter = Some(Box::new(crate::value::bigint::BigIntToString::new(
                                value, 10,
                            )));
                            return Ok(Action::Continue);
                        }
                        if let Some(parser) = bigint_comparison(instr, operands) {
                            *self = parser;
                            return Ok(Action::Continue);
                        }
                        Ok(Action::Resume {
                            instr,
                            operands: operands.clone(),
                        })
                    }
                    ConvertTarget::TypedArray { kind, from_buffer } => {
                        crate::object::typed_array::constructor(
                            heap,
                            task,
                            kind,
                            operands,
                            from_buffer,
                        )
                        .map(Action::Return)
                    }
                    ConvertTarget::TypedArrayWrite => {
                        let Value::Object(id) = operands[0] else {
                            return Err(Fault::Internal("typed array write receiver"));
                        };
                        let Value::Number(index) = operands[2] else {
                            return Err(Fault::Internal("typed array write key"));
                        };
                        let view = crate::object::typed_array::view(heap, task, id)?
                            .ok_or(Fault::Internal("typed array write kind"))?;
                        view.set(heap, index, &operands[3])?;
                        Ok(Action::Return(operands[1].clone()))
                    }
                    ConvertTarget::ArrayBuffer(which) => {
                        use crate::object::array_buffer as buffer;
                        let undefined = Value::Undefined;
                        match which {
                            builtin::Builtin::ArrayBuffer => {
                                if matches!(operands.get(1), Some(Value::Object(_))) {
                                    return Err(Fault::Unsupported("ArrayBuffer options and resizable buffers are not supported".into()));
                                }
                                buffer::constructor(heap, task, &operands[0])
                            }
                            builtin::Builtin::ArrayBufferTransfer => buffer::transfer(
                                heap,
                                task,
                                &operands[0],
                                operands.get(1).unwrap_or(&undefined),
                            ),
                            builtin::Builtin::ArrayBufferSlice => buffer::slice(
                                heap,
                                task,
                                &operands[0],
                                &operands[1],
                                operands.get(2).unwrap_or(&undefined),
                            ),
                            _ => Err(Fault::Internal("unexpected ArrayBuffer conversion")),
                        }
                        .map(Action::Return)
                    }
                    ConvertTarget::PrimitiveConstructor { which, construct } => {
                        if which == builtin::Builtin::BigInt
                            && let Value::String(text) = &operands[0]
                        {
                            *self = NativeState::BigIntParse {
                                parser: Box::new(crate::value::bigint::StringToBigInt::new(
                                    text.clone(),
                                )),
                                instr: None,
                                operands: vec![],
                                index: 0,
                                wrap: None,
                            };
                            return Ok(Action::Continue);
                        }
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
                    ConvertTarget::NumberToFixed { number } => {
                        builtin::number_to_fixed(number, &operands[0]).map(Action::Return)
                    }
                    ConvertTarget::NumberToPrecisionFormat { number, which } => {
                        // Undefined returned by an object is converted to NaN;
                        // it does not mean the optional count was omitted.
                        builtin::number_to_precision_format(which, number, &operands[0], false)
                            .map(Action::Return)
                    }
                    ConvertTarget::BigIntAsN { signed } => {
                        let bits = dispatch::to_index(&operands[0])?;
                        if let Value::String(text) = &operands[1] {
                            *self = NativeState::BigIntParse {
                                parser: Box::new(crate::value::bigint::StringToBigInt::new(
                                    text.clone(),
                                )),
                                instr: None,
                                operands: vec![],
                                index: 0,
                                wrap: Some((bits, signed)),
                            };
                            Ok(Action::Continue)
                        } else {
                            builtin::bigint_as_n(bits, &operands[1], signed).map(Action::Return)
                        }
                    }
                    ConvertTarget::BigIntToString => {
                        // Only an originally undefined radix defaulted to 10.
                        let radix =
                            builtin::to_radix(&Value::Number(dispatch::to_number(&operands[0])?))?;
                        let Value::BigInt(value) = &operands[1] else {
                            return Err(Fault::Internal("BigInt formatting receiver"));
                        };
                        *self = NativeState::BigIntFormat(Box::new(
                            crate::value::bigint::BigIntToString::new(value, radix),
                        ));
                        Ok(Action::Continue)
                    }
                    ConvertTarget::StringBuiltin { .. } => self.finish_string_builtin(heap, task),
                }
            }
            _ => Err(Fault::Internal("expected conversion state")),
        }
    }

    // Keep string completion temporaries off every other conversion's Rust
    // frame, including class-field conversions on 64 KiB worker stacks.
    #[inline(never)]
    fn finish_string_builtin(&mut self, heap: &Heap, task: TaskId) -> Result<Action, Fault> {
        let NativeState::Convert {
            target: ConvertTarget::StringBuiltin { which, access, .. },
            operands,
            end,
            ..
        } = self
        else {
            return Err(Fault::Internal("expected string conversion state"));
        };
        let (which, access) = (*which, *access);
        if matches!(
            which,
            builtin::Builtin::StringFromCharCode | builtin::Builtin::StringFromCodePoint
        ) {
            return builtin::string_from_codes(which, operands).map(Action::Return);
        }
        if builtin::is_string_build(which) {
            *self = NativeState::StringBuild(builtin::string_build(which, &operands[..*end])?);
            return Ok(Action::Continue);
        }
        if matches!(
            which,
            builtin::Builtin::StringPrototypeIndexOf | builtin::Builtin::StringPrototypeLastIndexOf
        ) {
            let text = dispatch::to_js_string(&operands[0])?;
            let search = dispatch::to_js_string(&operands[1])?;
            let last = which == builtin::Builtin::StringPrototypeLastIndexOf;
            let mut position = dispatch::to_number(&operands[2])?;
            if position.is_nan() {
                position = if last { f64::INFINITY } else { 0.0 };
            }
            let position = position.trunc().clamp(0.0, text.len() as f64) as usize;
            *self = NativeState::StringSearch(crate::value::string::StringSearch::new(
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

    fn fail(&mut self, failure: Failure) -> Action {
        match (self, failure) {
            (NativeState::BigIntParse { .. } | NativeState::BigIntFormat(_), _) => Action::Unwind,
            (
                NativeState::Eval(_)
                | NativeState::BuiltinConstruct(_)
                | NativeState::CopyProperties(_)
                | NativeState::ObjectStatics(_)
                | NativeState::Rest { .. }
                | NativeState::DynamicFunction(_)
                | NativeState::Reduce { .. }
                | NativeState::ArrayFrom(_)
                | NativeState::AddEntries(_)
                | NativeState::MathSumPrecise(_)
                | NativeState::ArraySort(_)
                | NativeState::ArrayMutate(_)
                | NativeState::ArraySearch(_)
                | NativeState::ArrayCallbacks(_)
                | NativeState::ArrayCopy(_)
                | NativeState::Iterator(_)
                | NativeState::Dispose(_)
                | NativeState::Disposal(_)
                | NativeState::Generator { .. }
                | NativeState::Async(_)
                | NativeState::AsyncGenerator(_)
                | NativeState::TypedArray(_)
                | NativeState::Promise(_)
                | NativeState::Error(_)
                | NativeState::RegExp(_)
                | NativeState::StringReplace(_)
                | NativeState::StringMethods(_)
                | NativeState::PropertyCall { .. }
                | NativeState::DefineProperty(_)
                | NativeState::Proxy(_)
                | NativeState::Instanceof { .. }
                | NativeState::Apply { .. }
                | NativeState::Push { .. }
                | NativeState::Join(_)
                | NativeState::StringRaw(_)
                | NativeState::Intl(_)
                | NativeState::PluralRules(_)
                | NativeState::RelativeTimeFormat(_)
                | NativeState::DisplayNames(_)
                | NativeState::DurationFormat(_)
                | NativeState::Collator(_)
                | NativeState::DateTimeFormat(_)
                | NativeState::ListFormat(_)
                | NativeState::Segmenter(_)
                | NativeState::NumberFormat(_)
                | NativeState::ArrayToString { .. }
                | NativeState::Bind { .. }
                | NativeState::NeedAll { .. }
                | NativeState::Convert { .. }
                | NativeState::MathHypot { .. }
                | NativeState::StringSearch(_)
                | NativeState::StringSplit { .. }
                | NativeState::StringBuild(_)
                | NativeState::NumberParse(_)
                | NativeState::DateParse { .. }
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

/// Fill an allocated task-owned target through its observable `set` method.
pub(crate) fn start_add_entries(
    state: &mut VmState,
    target: Value,
    items: Value,
    popped: usize,
) -> Result<(), Fault> {
    let native = add_entries::State::new(target, items);
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::AddEntries(Box::new(native)))
}

pub(crate) fn start_disposable_stack_constructor(
    state: &mut VmState,
    target: Value,
    popped: usize,
    kind: crate::object::disposable_stack::Kind,
) -> Result<(), Fault> {
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(
        state,
        NativeState::Disposal(Box::new(dispose::Stack::Construct {
            target,
            reading: false,
            kind,
        })),
    )
}

pub(crate) fn start_proxy(
    state: &mut VmState,
    mode: super::proxy::Mode,
    popped: usize,
) -> Result<(), Fault> {
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(
        state,
        NativeState::Proxy(Box::new(super::proxy::State::new(mode))),
    )
}

/// OrdinarySet keeps the original proxy receiver while forwarding its target
/// lookup, so the receiver's supported descriptor trap remains observable.
pub(crate) fn proxy_write(
    heap: &Heap,
    state: &mut VmState,
    base: &Value,
    key: &property::Key,
    value: &Value,
    strict: bool,
) -> Result<bool, Fault> {
    let Value::Object(target) = base else {
        return Ok(false);
    };
    if super::proxy::inspect(heap, state.diag.task, *target)?.is_none() {
        return Ok(false);
    }
    start_proxy(
        state,
        super::proxy::Mode::Set {
            target: *target,
            key: key.clone(),
            value: value.clone(),
            strict,
        },
        0,
    )?;
    Ok(true)
}

// Promise ownership failures construct errors on migrated 64 KiB workers.
#[inline(never)]
fn start_error(
    heap: &Heap,
    state: &mut VmState,
    which: builtin::Builtin,
    this: &Value,
    args: &[Value],
    popped: usize,
) -> Result<bool, Fault> {
    use builtin::Builtin;
    let error = match which {
        Builtin::Error => Some(error::ErrorState::constructor(
            heap,
            state.diag.task,
            args,
            None,
            state.error_stack(state.top().pc.saturating_sub(1))?,
        )?),
        Builtin::NativeError(kind) => Some(error::ErrorState::constructor(
            heap,
            state.diag.task,
            args,
            Some(kind),
            state.error_stack(state.top().pc.saturating_sub(1))?,
        )?),
        Builtin::ErrorPrototypeToString => Some(error::ErrorState::to_string(this)?),
        _ => None,
    };
    if let Some(native) = error {
        state.truncate_operands(state.stack_len() - popped)?;
        push_native(state, NativeState::Error(Box::new(native)))?;
        return Ok(true);
    }
    Ok(false)
}

pub(crate) fn start_add_values(
    state: &mut VmState,
    target: Value,
    items: Value,
    popped: usize,
) -> Result<(), Fault> {
    let mut native = add_entries::State::new(target, items);
    native.consume_values();
    state.truncate_operands(state.stack_len() - popped)?;
    push_native(state, NativeState::AddEntries(Box::new(native)))
}

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
    fn string_code_unit_coercion_releases_rope_nodes_incrementally() {
        use crate::value::string::cursor_release_counts;
        use crate::vm::builtin::Builtin;

        for which in [Builtin::StringFromCharCode, Builtin::StringFromCodePoint] {
            for prepend in [false, true] {
                let heap = Heap::default();
                let task = TaskId::from_raw(265);
                let mut text = JsString::from("                                ");
                for _ in 0..4096 {
                    let unit = JsString::from(" ");
                    text = if prepend {
                        unit.concat(&text)
                    } else {
                        text.concat(&unit)
                    }
                    .unwrap();
                }
                // The continuation owns the only reference to the input.
                // Public source tests cannot inspect these release counters.
                let mut native = NativeState::Convert {
                    formatter: None,
                    number_scan: None,
                    target: ConvertTarget::StringBuiltin {
                        which,
                        access: false,
                        concat_len: 0,
                    },
                    operands: vec![Value::String(text)],
                    next: 0,
                    end: 1,
                    hint: Hint::Number,
                    method: 0,
                    method_get: false,
                    symbol_method: SymbolMethod::Pending,
                };
                let initial = cursor_release_counts();
                let mut returned = None;
                for _ in 0..1000 {
                    let before = cursor_release_counts();
                    let action = native.step_conversion(&heap, task, Input::Start, &mut NoHost);
                    let after = cursor_release_counts();
                    assert!(after.0 - before.0 <= 256, "cursor disposal exceeded budget");
                    assert!(after.1 - before.1 <= 260, "rope disposal exceeded budget");
                    match action {
                        Ok(Action::Continue) => {}
                        Ok(Action::Return(Value::String(text))) => {
                            assert_eq!(text.code_units(), &[0]);
                            returned = Some(text);
                            break;
                        }
                        _ => panic!("unexpected constructor action"),
                    }
                }
                assert!(returned.is_some(), "constructor exceeded step bound");
                assert!(cursor_release_counts().1 - initial.1 >= 8192);
                let before = cursor_release_counts();
                drop(native);
                let after = cursor_release_counts();
                assert!(
                    after.0 - before.0 <= 2,
                    "native cursor disposal exceeded budget"
                );
                assert!(
                    after.1 - before.1 <= 4,
                    "native rope disposal exceeded budget"
                );
            }
        }
    }

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
        assert!(matches!(
            join.step(&heap, task, Input::Start),
            Err(Fault::StringTooLong)
        ));
        join.units = crate::value::string::MAX_STRING_LEN;
        assert!(join.append(JsString::from("")).is_ok());
        assert!(matches!(
            join.append(JsString::from("x")),
            Err(Fault::StringTooLong)
        ));
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
            formatter: None,
            number_scan: None,
            method_get: false,
            symbol_method: SymbolMethod::Pending,
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
        assert!(matches!(
            pending(limit - 1).step(
                &heap,
                task,
                &std::sync::Weak::new(),
                Input::Start,
                &mut NoHost
            ),
            Err(Fault::StringTooLong)
        ));
        // The same continuation below the limit really does request the
        // later conversion callback, so the overflow assertion checks order.
        assert!(
            matches!(pending(limit - 3).step(&heap, task, &std::sync::Weak::new(), Input::Start, &mut NoHost),
            Ok(Action::Call { this, .. }) if this == later)
        );
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
        assert!(traced.contains(&v.into()), "{traced:?}");
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
            discard(&mut natives, frame, &Heap::default());
            assert_eq!(depths(&natives), kept, "cancel to frame {frame}");
        }
        let mut reduce = stack().pop().unwrap().state;
        for failure in [Failure::Callback, Failure::Dependency, Failure::Step] {
            assert!(matches!(reduce.fail(failure), Action::Unwind));
        }
    }
}
