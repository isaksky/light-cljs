//! Task-owned VM storage: the explicit call-frame stack.
//!
//! Everything a task needs to continue executing lives here, owned by the
//! task (not by a worker thread or a Rust stack frame): a stack of call
//! frames, one value stack holding every frame's locals and operands, the
//! resource limits, and diagnostic context. A worker only borrows it for the
//! duration of one run (see [`crate::task`]).
//!
//! **Layout.** A [`Frame`] records the shared code it runs, its saved
//! instruction position, the function object it was called through, and
//! `base`, the index in [`VmState::stack`] of its local 0. Its locals occupy
//! `stack[base .. base + locals]` and its operands follow them; a caller's
//! operands end where its callee's `base` begins. The entry frame (the code
//! the task was spawned with) is stored apart from the frames it calls, so a
//! `VmState` always has a current frame and no lookup can find the stack
//! empty.
//!
//! **Calls without the Rust stack.** `Call` pops the callee and arguments,
//! pushes a new frame whose locals start at the callee's old operand slot,
//! and the interpreter loop simply continues in the new frame; `Return`
//! truncates the value stack to the frame's `base`, pops the frame, and
//! pushes the result for the caller, whose saved `pc` already names the
//! instruction after its `Call`. The interpreter never recurses, so nesting
//! depth is bounded only by [`VmLimits`], checked before a frame is pushed,
//! never by the worker's Rust stack. Like QuickJS's `JSStackFrame`
//! (`research/quickjs/quickjs.c`, revision 7d1c13c5d8f8: `cur_pc`, `cur_sp`,
//! `arg_buf`, `var_buf`), a frame saves its instruction and operand positions,
//! but as indices into task-owned storage: frames are not `alloca`'d by a
//! recursive `JS_CallInternal`, and there is no runtime-global current-frame
//! pointer.
//!
//! **No references across calls.** Frames and values are reached by index
//! each time; nothing borrows into `stack` or `frames` across an instruction,
//! so either may reallocate on a call and the task may yield or be collected
//! between any two instructions with several frames live.
//!
//! **Tracing.** A task's root source traces the whole value stack (every
//! frame's locals and operands), each frame's `this` and each frame's
//! function object, which reports its code's constants and its captured values (the closure
//! environment, including binding cells shared with other closures); the
//! entry frame's constants are the task's executable constants (`Trace for VmState`).
//!
//! **Exception handlers.** The handler stack (`crate::vm::exception`) is
//! task-owned like the frames: each entry names its frame by index and the
//! value-stack height to restore. Leaving a frame (return or unwinding)
//! drops its entries; raising pops frames and entries in one loop, never
//! recursing. Pending completions hold values (a saved return value or
//! exception), traced with the frames.
//!
//! **Native frames.** A resumable native operation (`crate::vm::native`)
//! keeps its pending state in [`VmState::natives`], beside the call frames:
//! its arguments, progress and intermediate values, traced with the frames,
//! and its operand area on the value stack just above the frame that started
//! it, where a callback it calls is entered like any other callee.
//! Raising and cancellation unwinding pop the native frames they pass with
//! the call frames (`native::unwind`, `native::discard`).
//!
//! **Dynamic context.** The task and world in [`DiagnosticContext`] belong to
//! the task, so every frame runs in the world its task was spawned in; a call
//! never changes world. A failure reports the failing frame and its chain of
//! callers ([`crate::Diagnostic::call_chain`]).

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::Arc;

use crate::collections::OwnedBuilders;
use crate::error::{CallSite, Diagnostic, VmError, VmErrorKind};
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::{DependencyKey, TaskId, WorldId};
use crate::value::{JsString, Value};
use crate::vm::dispatch::Fault;
use crate::vm::exception::{Completion, Exception, Handler, HandlerKind};
use crate::vm::executable::Code;
use crate::vm::native::{self, NativeFrame};

/// Maximum UTF-16 units in a stack snapshot, including formatting. Individual
/// metadata fields are limited to 256 units as well, so one frame cannot force
/// a large synchronous copy. Oversized function names are replaced wholesale
/// before inspecting their rope; filenames retain a bounded UTF-16 prefix.
const ERROR_STACK_UNITS: usize = 16_384;
const ERROR_STACK_METADATA_UNITS: usize = 256;

/// An owning handle whose reference count belongs to one task. Only creating
/// this handle touches the executable's shared reference count; calls and
/// dispatch-frame changes clone the outer Arc. Both layers are Send + Sync,
/// so migration and detached generator activations need no special lifetime
/// rules or unsafe references. The immutable Code itself is never copied.
#[derive(Clone, Debug)]
pub(crate) struct TaskCode(Arc<TaskCodeOwner>);

#[derive(Debug)]
struct TaskCodeOwner {
    shared: Arc<Code>,
}

impl TaskCode {
    pub(crate) fn shared(&self) -> Arc<Code> {
        Arc::clone(&self.0.shared)
    }
}

impl std::ops::Deref for TaskCode {
    type Target = Code;
    fn deref(&self) -> &Code {
        &self.0.shared
    }
}

/// Bound retained code even for an unending stream of dynamic evals. Cached
/// code is not a GC root: a handle is only obtained from a live function or
/// executable; frames retain their usual callee/constant roots. Owning the
/// Arc prevents pointer reuse from aliasing a different executable.
#[derive(Debug, Default)]
pub(crate) struct CodeCache(RefCell<HashMap<usize, TaskCode>>);

impl CodeCache {
    pub(crate) fn get(&self, code: &Arc<Code>) -> TaskCode {
        let key = Arc::as_ptr(code) as usize;
        let mut cache = self.0.borrow_mut();
        if let Some(local) = cache.get(&key) {
            return local.clone();
        }
        if cache.len() == 256 {
            cache.clear();
        }
        let local = TaskCode(Arc::new(TaskCodeOwner {
            shared: Arc::clone(code),
        }));
        cache.insert(key, local.clone());
        local
    }
}

#[cfg(test)]
thread_local! {
    pub(crate) static FAIL_STACK_RESERVATION: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Integer remainder is exact when both Numbers round-trip through i32.
/// Saturating casts alone are insufficient: the equality guards reject NaN,
/// infinities, fractions and out-of-range values. `wrapping_rem` handles
/// i32::MIN % -1 without overflow; copysign restores the dividend's -0.
/// Everything outside this narrow guard retains the general f64 remainder.
#[inline]
fn number_remainder(a: f64, b: f64) -> f64 {
    let x = a as i32;
    let y = b as i32;
    if f64::from(x) == a && f64::from(y) == b && y != 0 {
        f64::from(x.wrapping_rem(y)).copysign(a)
    } else {
        a % b
    }
}

/// A single fallibly reserved buffer. Appends and formatting cannot grow it.
struct ErrorStackSnapshot {
    units: Vec<u16>,
    limit: usize,
}

impl ErrorStackSnapshot {
    fn append(&mut self, units: impl IntoIterator<Item = u16>) {
        self.units
            .extend(units.into_iter().take(self.limit - self.units.len()));
    }

    fn text(&mut self, text: &str) {
        self.append(text.encode_utf16());
    }
}

impl std::fmt::Write for ErrorStackSnapshot {
    fn write_str(&mut self, text: &str) -> std::fmt::Result {
        self.text(text);
        Ok(())
    }
}

/// Per-task VM resource policy, fixed when the task is spawned
/// ([`Runtime::with_limits`](crate::Runtime::with_limits)).
///
/// A `Call` that would exceed either bound fails the task with
/// [`VmErrorKind::StackOverflow`] before any frame is pushed. The entry frame
/// is always allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VmLimits {
    /// Maximum number of live frames, the entry frame included (a value
    /// below 1 behaves as 1: no calls).
    pub max_call_depth: usize,
    /// Maximum number of values (locals plus operand slots reserved by each
    /// frame's validated maximum operand depth) across all frames of the task.
    pub max_stack_values: usize,
}

impl VmLimits {
    /// The defaults: 10,000 frames and 1,048,576 values (16 MiB of values).
    pub const DEFAULT: VmLimits = VmLimits {
        max_call_depth: 10_000,
        max_stack_values: 1 << 20,
    };
}

impl Default for VmLimits {
    fn default() -> Self {
        VmLimits::DEFAULT
    }
}

/// One activation of a function. See the module docs.
#[derive(Debug)]
pub(crate) struct Frame {
    /// Eval's var/function declarations create configurable global properties.
    pub(crate) eval: bool,
    /// The shared code this frame runs (never copied per task or call).
    pub(crate) code: TaskCode,
    /// The function object this frame was called through, for
    /// `CurrentFunction` and for `GetCapture` (its captured values are the
    /// frame's closure environment). `None` for the entry frame until
    /// `CurrentFunction` first allocates one.
    pub(crate) callee: Option<ObjectId>,
    /// The frame's `this` (clj-quickjs-116): the receiver of the
    /// `CallMethod` that entered it, `undefined` for a plain `Call`, a
    /// native's callback and the entry frame. Task-owned and traced with the
    /// frame; `GetThis` reads it.
    pub(crate) this: Value,
    /// Construct target, undefined on ordinary calls. Arrows capture it by
    /// value; direct eval transports it through its scope descriptor.
    pub(crate) new_target: Value,
    /// A construct frame (clj-quickjs-71): entered by `Construct` with the
    /// new object as `this`. When it returns a value that is not an object,
    /// the caller receives `this` instead (the return-override rule).
    pub(crate) construct: bool,
    /// Original actual arguments for NewArguments, Rest or ForwardArguments.
    /// Traced until NewArguments consumes them, or until the frame leaves.
    pub(crate) arguments: Option<Vec<Value>>,
    /// Index of the next instruction to execute in `code`.
    pub(crate) pc: usize,
    /// Index of this frame's local 0 in [`VmState::stack`].
    pub(crate) base: usize,
}

/// A movable activation. All indices are relative to its frame/stack origin;
/// it owns no worker state or Rust stack. Also suitable for async resumes.
/// The stack includes live expression operands above the locals, and handlers
/// include pending finally completions (normal, return, throw or jump). Their
/// values are traced here while detached, then by VmState after attachment.
#[derive(Debug)]
pub(crate) struct DetachedActivation {
    frame: Frame,
    stack: Vec<Value>,
    handlers: Vec<Handler>,
    natives: Vec<NativeFrame>,
}

impl Trace for DetachedActivation {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(id) = self.frame.callee {
            visit(id.into());
        }
        self.frame.this.trace(visit);
        self.frame.new_target.trace(visit);
        self.frame.code.trace(visit);
        self.frame
            .arguments
            .iter()
            .flatten()
            .for_each(|v| v.trace(visit));
        self.stack.iter().for_each(|v| v.trace(visit));
        self.handlers.iter().for_each(|h| h.trace(visit));
        self.natives.iter().for_each(|n| n.trace(visit));
    }
}

impl Frame {
    /// Index in the value stack of this frame's first operand.
    fn floor(&self) -> usize {
        self.base + usize::from(self.code.locals)
    }
}

/// Context reported with every runtime failure.
#[derive(Clone, Copy, Debug)]
pub(crate) struct DiagnosticContext {
    pub(crate) task: TaskId,
    pub(crate) world: WorldId,
}

/// A `Need` this task is suspended at: the pending result destination
/// (`docs/design.md` §§7, 10). The result goes onto the running frame's
/// operand stack, the slot the instruction's operands occupied, and the frame
/// cannot change while the task waits. Holds a key and an instruction index
/// only, so it is not a root. See `crate::task`, "Dependency waits in the VM".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PendingNeed {
    /// The key the task subscribed to (its world is the task's).
    pub(crate) key: DependencyKey,
    /// Index of the `Need` instruction in the running frame's code: where a
    /// failed or cancelled result is reported.
    pub(crate) pc: usize,
}

/// What [`VmState::return_value`] did.
pub(crate) enum Returned {
    /// The entry frame returned: the task completes with this value.
    Complete(Value),
    /// The frame was left; the caller continues with the value pushed.
    Left,
    /// A finally block of the frame runs first, with the return pending.
    Finally,
    /// The return would discard the pending cancellation of a finally block
    /// running for the task's cancellation: the cleanup ends instead.
    Cancelled,
}

/// What [`VmState::raise`] did.
pub(crate) enum Raised {
    /// A handler took the exception; execution continues at its target.
    Caught,
    /// No handler is left: the task fails with the exception's error.
    Uncaught(Exception),
    /// The handler stack contradicts the frames (hand-assembled code only).
    Broken(&'static str),
    /// The exception would discard the pending cancellation of a finally
    /// block running for the task's cancellation: the cleanup ends instead.
    Cancelled,
}

/// A script binding retained independently of activation replacement.
#[derive(Debug)]
pub(crate) struct GlobalLexical {
    pub(crate) cell: Value,
    pub(crate) mutable: bool,
}

/// The complete VM state of one task.
#[derive(Debug)]
pub(crate) struct VmState {
    pub(crate) code_cache: CodeCache,
    /// Deterministic task-local Math.random stream; never worker/realm state.
    pub(crate) random_state: u64,
    pub(crate) module_link: Option<Box<crate::module::Linker>>,
    pub(crate) sim_host: Option<Box<crate::host::sim::facilities::TaskHost>>,
    pub(crate) collection_point: bool,
    pub(crate) jobs: std::sync::Weak<crate::job::JobInbox>,
    /// The script body's completion value, retained while jobs run.
    pub(crate) script_result: Option<Value>,
    // Diagnostic identities only: deliberately not traced.
    pub(crate) suspended_async: Vec<(ObjectId, String)>,
    /// AddToKeptObjects: deduplicated strong targets for this task's current
    /// job. Yields, native callbacks, generators and dependency waits are not
    /// job boundaries. Both parked and running VM tracing includes this set.
    pub(crate) kept_objects: std::collections::HashMap<crate::heap::gc::TraceEdge, Value>,
    /// Mutable global state belongs to the task, independently of workers.
    pub(crate) global_object: Option<ObjectId>,
    /// Script lexical bindings, including TDZ cells. Eval-local lexicals
    /// are ordinary frame bindings and never enter this environment.
    pub(crate) global_lexicals: std::collections::HashMap<crate::value::JsString, GlobalLexical>,
    /// The frame the task started in.
    entry: Frame,
    /// Frames called from the entry frame, innermost last.
    calls: Vec<Frame>,
    /// Locals and operands of every frame (see the module docs).
    stack: Vec<Value>,
    /// Values retained in frames before NewArguments consumes them.
    pending_arguments: usize,
    limits: VmLimits,
    pub(crate) diag: DiagnosticContext,
    /// Transient builders this task created, released when it finishes. Not
    /// a root (see `crate::collections::builder`).
    pub(crate) builders: OwnedBuilders,
    /// Set while the task is suspended at a `Need` whose result is not
    /// injected yet.
    pub(crate) pending_need: Option<PendingNeed>,
    /// The handler stack: catch and finally handlers and pending completions
    /// of every frame, innermost last (`crate::vm::exception`).
    handlers: Vec<Handler>,
    /// Pending resumable native operations, innermost last, each above the
    /// call frame that started it (`crate::vm::native`). Traced.
    pub(crate) natives: Vec<NativeFrame>,
    /// Set once the task's consumer cancellation started running its
    /// finally blocks (`crate::task`, "Cancellation and finally blocks").
    pub(crate) cancelling: bool,
}

/// Bind `args` to a frame's locals: as in JavaScript, missing arguments are
/// `undefined` and extra ones are ignored; non-parameter locals start
/// `undefined`.
fn bind_args(stack: &mut Vec<Value>, base: usize, code: &Code, argc: usize) {
    stack.truncate(base + argc.min(usize::from(code.params)));
    stack.resize(base + usize::from(code.locals), Value::Undefined);
}

impl VmState {
    pub(crate) fn detach_activation(&mut self) -> Result<DetachedActivation, Fault> {
        let index = self.frame_index();
        let mut frame = if let Some(frame) = self.calls.pop() {
            frame
        } else {
            // Embedders may spawn a compiled generator function directly.
            // Leave a tiny return trampoline as the task's entry frame.
            let mut asm = crate::vm::Assembler::new();
            asm.get_local(0).op(crate::vm::Opcode::Return);
            let code = asm
                .finish("<activation result>", 1, 1)
                .expect("return trampoline")
                .0;
            std::mem::replace(
                &mut self.entry,
                Frame {
                    eval: false,
                    code: self.code_cache.get(&code),
                    callee: None,
                    this: Value::Undefined,
                    new_target: Value::Undefined,
                    construct: false,
                    arguments: None,
                    pc: 1,
                    base: 0,
                },
            )
        };
        let base = frame.base;
        // Save operands as well as locals: e.g. both a and b are live at
        // suspension in a + b * (yield c). No compiler spill is needed.
        let stack = self.stack.split_off(base);
        let first = self.handlers.partition_point(|h| h.frame < index);
        let mut handlers = self.handlers.split_off(first);
        let native_origin = self.natives.partition_point(|n| n.depth() < index + 1);
        for h in &mut handlers {
            // Handler and pending-completion heights use the same origin
            // as the saved values, independent of the next caller's depth.
            h.frame -= index;
            h.height -= base;
            if let HandlerKind::NativeCatch { native } = &mut h.kind {
                *native -= native_origin;
            }
        }
        let natives = native::detach(&mut self.natives, index + 1, base);
        self.pending_arguments -= frame.arguments.as_ref().map_or(0, Vec::len);
        frame.base = 0;
        if index == 0 {
            self.stack.push(Value::Undefined);
        }
        Ok(DetachedActivation {
            frame,
            stack,
            handlers,
            natives,
        })
    }

    /// Check before removing a saved activation from its heap owner.
    pub(crate) fn check_activation(&self, saved: &DetachedActivation) -> Result<(), Fault> {
        let needed = self
            .stack
            .len()
            .saturating_add(usize::from(saved.frame.code.locals))
            .saturating_add(saved.frame.code.max_stack)
            .saturating_add(saved.frame.arguments.as_ref().map_or(0, Vec::len))
            .saturating_add(self.pending_arguments);
        if self.depth() >= self.limits.max_call_depth.max(1)
            || needed > self.limits.max_stack_values
            || self.handlers.len().saturating_add(saved.handlers.len())
                > self.limits.max_stack_values
            || self.natives.len().saturating_add(saved.natives.len())
                > self.limits.max_call_depth.max(1)
        {
            return Err(Fault::StackOverflow(
                "resuming activation exceeds VM limits".into(),
            ));
        }
        Ok(())
    }

    pub(crate) fn attach_activation(&mut self, mut saved: DetachedActivation) {
        native::assert_no_borrow("attaching an activation");
        let base = self.stack.len();
        let index = self.depth();
        saved.frame.base = base;
        self.pending_arguments += saved.frame.arguments.as_ref().map_or(0, Vec::len);
        for h in &mut saved.handlers {
            h.frame += index;
            h.height += base;
            if let HandlerKind::NativeCatch { native } = &mut h.kind {
                *native += self.natives.len();
            }
        }
        native::reattach(&mut saved.natives, index + 1, base);
        self.stack.extend(saved.stack);
        self.calls.push(saved.frame);
        self.handlers.extend(saved.handlers);
        self.natives.extend(saved.natives);
    }
    /// Fresh state at instruction 0 of `code`, with `args` bound to its
    /// parameters (see `bind_args`).
    pub(crate) fn new(
        code: &Arc<Code>,
        args: &[Value],
        diag: DiagnosticContext,
        limits: VmLimits,
    ) -> VmState {
        let mut stack = Vec::with_capacity(usize::from(code.locals) + code.max_stack);
        // Only declared parameters are copied, so ignored extra arguments
        // never enlarge the task's retained storage.
        let bound = &args[..args.len().min(usize::from(code.params))];
        stack.extend_from_slice(bound);
        bind_args(&mut stack, 0, code, bound.len());
        let code_cache = CodeCache::default();
        let entry_code = code_cache.get(code);
        VmState {
            code_cache,
            random_state: 0,
            module_link: None,
            sim_host: None,
            collection_point: false,
            jobs: std::sync::Weak::new(),
            script_result: None,
            suspended_async: Vec::new(),
            kept_objects: std::collections::HashMap::new(),
            global_object: None,
            global_lexicals: std::collections::HashMap::new(),
            entry: Frame {
                eval: false,
                code: entry_code,
                callee: None,
                this: Value::Undefined,
                new_target: Value::Undefined,
                construct: false,
                arguments: code.uses_arguments.then(|| args.to_vec()),
                pc: 0,
                base: 0,
            },
            calls: Vec::new(),
            stack,
            pending_arguments: if code.uses_arguments { args.len() } else { 0 },
            limits,
            diag,
            builders: OwnedBuilders::default(),
            pending_need: None,
            handlers: Vec::new(),
            natives: Vec::new(),
            cancelling: false,
        }
    }

    /// Replace the completed activation with a fresh job entry. The small
    /// trampoline uses ordinary Call/Return, including built-ins, bound
    /// functions, native continuations, argument binding and resource checks.
    /// Task-wide globals, builders and the script result survive the switch.
    pub(crate) fn start_job(
        &mut self,
        heap: &crate::heap::Heap,
        job: crate::job::Job,
    ) -> Result<(), Box<VmError>> {
        // Only raw functions eligible for an ordinary call can start as an
        // entry frame. Proxies must go through Call's apply validation, and
        // class constructors must go through enter's rejection without new.
        if let Ok(crate::vm::dispatch::Callee::Code(id, code)) =
            crate::vm::dispatch::cached_callee(heap, &job.callable, &self.code_cache)
            && job.callable == Value::Object(id)
            && !code.constructor_kind.is_class()
        {
            let needed = usize::from(code.locals)
                .saturating_add(code.max_stack)
                .saturating_add(if code.uses_arguments {
                    job.args.len()
                } else {
                    0
                });
            if needed > self.limits.max_stack_values {
                return Err(Box::new(self.failure(
                    VmErrorKind::StackOverflow,
                    "job activation exceeds the stack limit".into(),
                    self.top().pc,
                )));
            }
            let mut next = Self::new(&code.shared(), &job.args, self.diag, self.limits);
            next.entry.callee = Some(id);
            self.replace_activation(next);
            return Ok(());
        }
        let slots = job.args.len() + 1;
        if slots > usize::from(u16::MAX) || slots.saturating_mul(2) > self.limits.max_stack_values {
            return Err(Box::new(self.failure(
                VmErrorKind::StackOverflow,
                "job arguments exceed the stack limit".into(),
                self.top().pc,
            )));
        }
        let mut asm = crate::vm::Assembler::new();
        for i in 0..slots {
            asm.get_local(i as u16);
        }
        asm.call(job.args.len() as u16)
            .op(crate::vm::Opcode::Return);
        let code = asm
            .finish("<job>", slots as u16, slots as u16)
            .expect("valid job trampoline");
        let mut args = Vec::with_capacity(slots);
        args.push(job.callable);
        args.extend(job.args);
        let next = Self::new(&code.0, &args, self.diag, self.limits);
        self.replace_activation(next);
        Ok(())
    }

    pub(crate) fn start_module(
        &mut self,
        code: &crate::Executable,
        args: &[Value],
    ) -> Result<(), Fault> {
        if usize::from(code.0.locals).saturating_add(code.0.max_stack)
            > self.limits.max_stack_values
        {
            return Err(Fault::StackOverflow(
                "module frame exceeds the stack limit".into(),
            ));
        }
        let next = Self::new(&code.0, args, self.diag, self.limits);
        self.replace_activation(next);
        Ok(())
    }

    fn replace_activation(&mut self, mut next: Self) {
        next.random_state = self.random_state;
        next.module_link = self.module_link.take();
        next.sim_host = self.sim_host.take();
        next.jobs = self.jobs.clone();
        next.global_object = self.global_object.take();
        next.global_lexicals = std::mem::take(&mut self.global_lexicals);
        next.script_result = self.script_result.take();
        next.suspended_async = std::mem::take(&mut self.suspended_async);
        next.kept_objects = std::mem::take(&mut self.kept_objects);
        next.builders = std::mem::take(&mut self.builders);
        *self = next;
    }

    /// The running (innermost) frame.
    pub(crate) fn top(&self) -> &Frame {
        self.calls.last().unwrap_or(&self.entry)
    }

    pub(crate) fn top_mut(&mut self) -> &mut Frame {
        self.calls.last_mut().unwrap_or(&mut self.entry)
    }

    pub(crate) fn clear_arguments(&mut self) {
        if let Some(args) = self.top_mut().arguments.take() {
            self.pending_arguments -= args.len();
        }
    }

    fn truncate_calls(&mut self, len: usize) {
        for frame in &self.calls[len..] {
            self.pending_arguments -= frame.arguments.as_ref().map_or(0, Vec::len);
        }
        self.calls.truncate(len);
    }

    /// Number of live frames, the entry frame included.
    pub(crate) fn depth(&self) -> usize {
        1 + self.calls.len()
    }

    /// The bounds of resolving a forwarding built-in's call in one
    /// instruction (`crate::vm::dispatch::resolve`): how many arguments
    /// fit in the value stack above `base`, the value-stack length once the
    /// forwarding call's own operands are gone (they are replaced), after
    /// the callee slot; and how many forwarding hops are allowed (the
    /// call-depth limit: each hop stands for a native frame).
    pub(crate) fn forward_limits(&self, base: usize) -> (usize, usize) {
        (
            self.limits
                .max_stack_values
                .saturating_sub(base.saturating_add(1)),
            self.limits.max_call_depth.max(1),
        )
    }

    /// Push an operand onto the running frame.
    pub(crate) fn push(&mut self, v: Value) {
        native::assert_no_borrow("pushing a value (growing the value stack)");
        self.stack.push(v);
    }

    /// Number of values on the value stack (every frame's locals and
    /// operands).
    pub(crate) fn stack_len(&self) -> usize {
        self.stack.len()
    }

    /// The value at value-stack index `at`, which must be an operand of the
    /// running frame.
    pub(crate) fn stack_value(&self, at: usize) -> Result<Value, Fault> {
        self.stack
            .get(at)
            .cloned()
            .filter(|_| at >= self.top().floor())
            .ok_or(Fault::Internal("operand index out of range"))
    }

    /// Drop the running frame's operands from value-stack index `len` up.
    pub(crate) fn truncate_operands(&mut self, len: usize) -> Result<(), Fault> {
        if len < self.top().floor() {
            return Err(Fault::Internal("operand stack underflow"));
        }
        self.stack.truncate(len);
        Ok(())
    }

    /// Pop an operand of the running frame; never reaches into its locals or
    /// a caller's storage.
    pub(crate) fn pop(&mut self) -> Result<Value, Fault> {
        if self.stack.len() <= self.top().floor() {
            return Err(Fault::Internal("operand stack underflow"));
        }
        self.stack
            .pop()
            .ok_or(Fault::Internal("operand stack underflow"))
    }

    /// The running frame's operands, oldest first.
    pub(crate) fn operands(&self) -> &[Value] {
        self.stack.get(self.top().floor()..).unwrap_or(&[])
    }

    #[inline]
    fn fusion_number(&self, code: &Code, source: super::dispatch::NumberSource) -> Option<f64> {
        use super::dispatch::NumberSource;
        let value = match source {
            NumberSource::Local(i) if i < code.locals => {
                self.stack.get(self.top().base + usize::from(i))?
            }
            NumberSource::Constant(i) => code.constants.get(usize::from(i))?,
            _ => return None,
        };
        match value {
            Value::Number(number) => Some(*number),
            _ => None,
        }
    }

    /// Execute only a completely guarded expression. Nothing can allocate,
    /// call, poll, throw or yield between these numeric reads and the commit;
    /// heap references remain in traced task storage throughout. Misses leave
    /// pc, locals and operands untouched for the original instruction stream.
    #[inline]
    pub(crate) fn fused_primitive_step(
        &mut self,
        code: &Code,
        fusion: &super::dispatch::PrimitiveFusion,
    ) -> bool {
        use super::dispatch::FusionOutput;
        use super::instr::Instr;
        #[inline]
        fn arithmetic(op: Instr, a: f64, b: f64) -> f64 {
            match op {
                Instr::Add | Instr::Plus => a + b,
                Instr::Sub => a - b,
                Instr::Mul => a * b,
                Instr::Div => a / b,
                Instr::Rem => number_remainder(a, b),
                _ => unreachable!("validated fused arithmetic"),
            }
        }
        let Some(mut a) = self.fusion_number(code, fusion.left) else {
            return false;
        };
        let Some(mut b) = self.fusion_number(code, fusion.right) else {
            return false;
        };
        let mut op = fusion.op;
        if let Some((source, outer)) = fusion.outer {
            let Some(first) = self.fusion_number(code, source) else {
                return false;
            };
            b = arithmetic(op, a, b);
            a = first;
            op = outer;
        }
        if let FusionOutput::Store(i, _) = fusion.output
            && (i >= code.locals || self.top().base + usize::from(i) >= self.top().floor())
        {
            return false;
        }
        let value = match op {
            Instr::Lt | Instr::CompareLt => Value::Bool(a < b),
            Instr::Gt | Instr::CompareGt => Value::Bool(a > b),
            Instr::Le | Instr::CompareLe => Value::Bool(a <= b),
            Instr::Ge | Instr::CompareGe => Value::Bool(a >= b),
            _ => Value::Number(arithmetic(op, a, b)),
        };
        let next = self.top().pc + fusion.len as usize;
        match fusion.output {
            FusionOutput::Push => self.push(value),
            FusionOutput::Store(i, keep) => {
                let at = self.top().base + usize::from(i);
                if keep {
                    self.push(value.clone());
                }
                self.stack[at] = value;
            }
            FusionOutput::Branch(target) if !value.truthy() => {
                self.top_mut().pc = target as usize;
                return true;
            }
            FusionOutput::Branch(_) => {}
        }
        self.top_mut().pc = next;
        true
    }

    /// Primitive dispatch without a `Result<_, Fault>` temporary. A miss
    /// changes nothing, so the ordinary instruction path can diagnose it or
    /// perform JavaScript conversion. All indexing remains checked.
    #[inline]
    pub(crate) fn primitive_step(&mut self, code: &Code, instr: super::instr::Instr) -> bool {
        use super::instr::Instr;
        let floor = self.top().floor();
        let len = self.stack.len();
        let depth = len.saturating_sub(floor);
        match instr {
            Instr::Const(i) => {
                let Some(value) = code.constants.get(usize::from(i)) else {
                    return false;
                };
                self.push(value.clone());
            }
            Instr::Undefined => self.push(Value::Undefined),
            Instr::GetLocal(i) => {
                if i >= code.locals {
                    return false;
                }
                let Some(value) = self.stack.get(self.top().base + usize::from(i)) else {
                    return false;
                };
                self.push(value.clone());
            }
            Instr::SetLocal(i) => {
                let at = self.top().base + usize::from(i);
                if i >= code.locals || depth == 0 || at >= floor {
                    return false;
                }
                let value = self.stack.pop().expect("checked operand");
                self.stack[at] = value;
            }
            Instr::Pop if depth > 0 => {
                self.stack.pop();
            }
            Instr::Dup if depth > 0 => self.push(self.stack[len - 1].clone()),
            Instr::Not if depth > 0 => {
                self.stack[len - 1] = Value::Bool(!self.stack[len - 1].truthy())
            }
            Instr::StrictEq if depth >= 2 => {
                let equal = self.stack[len - 2].strict_equals(&self.stack[len - 1]);
                self.stack.pop();
                self.stack[len - 2] = Value::Bool(equal);
            }
            Instr::Jump(target) => {
                self.top_mut().pc = target as usize;
                return true;
            }
            Instr::JumpIfFalse(target) if depth > 0 => {
                let taken = !self.stack.pop().expect("checked operand").truthy();
                self.top_mut().pc = if taken {
                    target as usize
                } else {
                    self.top().pc + 1
                };
                return true;
            }
            Instr::ToNumber | Instr::ToNumeric | Instr::Neg | Instr::Inc | Instr::Dec
                if depth > 0 =>
            {
                let Value::Number(value) = self.stack[len - 1] else {
                    return false;
                };
                self.stack[len - 1] = Value::Number(match instr {
                    Instr::Neg => -value,
                    Instr::Inc => value + 1.0,
                    Instr::Dec => value - 1.0,
                    _ => value,
                });
            }
            Instr::ToNumberPair
            | Instr::Add
            | Instr::Plus
            | Instr::Sub
            | Instr::Mul
            | Instr::Div
            | Instr::Rem
            | Instr::Lt
            | Instr::Gt
            | Instr::Le
            | Instr::Ge
            | Instr::CompareLt
            | Instr::CompareGt
            | Instr::CompareLe
            | Instr::CompareGe
                if depth >= 2 =>
            {
                let (Value::Number(a), Value::Number(b)) =
                    (&self.stack[len - 2], &self.stack[len - 1])
                else {
                    return false;
                };
                let value = match instr {
                    Instr::ToNumberPair => {
                        self.top_mut().pc += 1;
                        return true;
                    }
                    Instr::Add | Instr::Plus => Value::Number(a + b),
                    Instr::Sub => Value::Number(a - b),
                    Instr::Mul => Value::Number(a * b),
                    Instr::Div => Value::Number(a / b),
                    Instr::Rem => Value::Number(number_remainder(*a, *b)),
                    Instr::Lt | Instr::CompareLt => Value::Bool(a < b),
                    Instr::Gt | Instr::CompareGt => Value::Bool(a > b),
                    Instr::Le | Instr::CompareLe => Value::Bool(a <= b),
                    _ => Value::Bool(a >= b),
                };
                self.stack.pop();
                self.stack[len - 2] = value;
            }
            _ => return false,
        }
        self.top_mut().pc += 1;
        true
    }

    /// Remove the running frame's operand that has `above` operands on top
    /// of it (0: the top), shifting those down.
    pub(crate) fn remove_operand(&mut self, above: usize) -> Result<Value, Fault> {
        let at = self
            .stack
            .len()
            .checked_sub(above + 1)
            .filter(|&at| at >= self.top().floor())
            .ok_or(Fault::Internal("operand stack underflow"))?;
        native::assert_no_borrow("removing an operand (moving the value stack)");
        Ok(self.stack.remove(at))
    }

    /// Replace the top `popped` operands of the running frame with `result`.
    pub(crate) fn replace(&mut self, popped: usize, result: Value) -> Result<(), Fault> {
        let keep = self
            .stack
            .len()
            .checked_sub(popped)
            .filter(|&keep| keep >= self.top().floor())
            .ok_or(Fault::Internal("operand stack underflow"))?;
        self.stack.truncate(keep);
        self.stack.push(result);
        Ok(())
    }

    /// Local `index` of the running frame.
    pub(crate) fn local(&mut self, index: u16) -> Result<&mut Value, Fault> {
        let top = self.top();
        if index >= top.code.locals {
            return Err(Fault::Internal("local index out of range"));
        }
        let at = top.base + usize::from(index);
        self.stack
            .get_mut(at)
            .ok_or(Fault::Internal("local index out of range"))
    }

    /// Enter `code`, called through the function object `callee`: its
    /// arguments are the top `argc` operands of the running frame, and the
    /// operand below them is the callee value, which the new frame's locals
    /// replace, and `this` is its `this` (clj-quickjs-116). Fails, changing
    /// nothing, if the operands are missing or the new frame would exceed
    /// the task's limits.
    pub(crate) fn enter(
        &mut self,
        code: TaskCode,
        callee: ObjectId,
        this: Value,
        argc: usize,
    ) -> Result<(), Fault> {
        if code.constructor_kind.is_class() {
            return Err(Fault::TypeError(
                "class constructor cannot be invoked without new".into(),
            ));
        }
        self.enter_constructor(code, callee, this, argc)
    }

    /// Enter after the construct path has checked `[[Construct]]`, or after
    /// `enter` has checked ordinary-call eligibility.
    pub(crate) fn enter_constructor(
        &mut self,
        code: TaskCode,
        callee: ObjectId,
        this: Value,
        argc: usize,
    ) -> Result<(), Fault> {
        native::assert_no_borrow("entering a call frame (reentering JavaScript)");
        let base = self
            .stack
            .len()
            .checked_sub(argc + 1)
            .filter(|&b| b >= self.top().floor())
            .ok_or(Fault::Internal("operand stack underflow"))?;
        if self.depth() >= self.limits.max_call_depth.max(1) {
            return Err(Fault::StackOverflow(format!(
                "maximum call depth of {} frames exceeded",
                self.limits.max_call_depth.max(1)
            )));
        }
        let needed = base
            .saturating_add(usize::from(code.locals))
            .saturating_add(code.max_stack)
            .saturating_add(if code.uses_arguments { argc } else { 0 })
            .saturating_add(self.pending_arguments);
        if needed > self.limits.max_stack_values {
            return Err(Fault::StackOverflow(format!(
                "a call needing {needed} stack values exceeds the limit of {}",
                self.limits.max_stack_values
            )));
        }
        // Shift the arguments down over the callee slot, then bind them.
        self.stack.remove(base);
        let arguments = code
            .uses_arguments
            .then(|| self.stack[base..base + argc].to_vec());
        if arguments.is_some() {
            self.pending_arguments += argc;
        }
        bind_args(&mut self.stack, base, &code, argc);
        self.stack.reserve(code.max_stack);
        self.calls.push(Frame {
            eval: false,
            code,
            callee: Some(callee),
            this,
            new_target: Value::Undefined,
            construct: false,
            arguments,
            pc: 0,
            base,
        });
        Ok(())
    }

    /// Leave the running frame with `result`. Returns `Some(result)` when it
    /// was the entry frame (the task completes); otherwise pops the frame,
    /// discarding its locals and operands, and pushes `result` for the caller
    /// (a construct frame's `this` instead, unless `result` is an object).
    fn leave(&mut self, result: Value) -> Option<Value> {
        let frame = self.frame_index();
        while self.handlers.last().is_some_and(|h| h.frame >= frame) {
            self.handlers.pop();
        }
        match self.calls.pop() {
            None => Some(result),
            Some(frame) => {
                self.pending_arguments -= frame.arguments.as_ref().map_or(0, Vec::len);
                self.stack.truncate(frame.base);
                let result = match result {
                    Value::Object(_) => result,
                    _ if frame.construct => frame.this,
                    _ => result,
                };
                self.stack.push(result);
                None
            }
        }
    }

    /// Index of the running frame (0: the entry frame).
    fn frame_index(&self) -> usize {
        self.calls.len()
    }

    /// Push a catch or finally handler of the running frame, at its current
    /// operand height. The handler stack is bounded like the value stack.
    pub(crate) fn push_handler(&mut self, kind: HandlerKind) -> Result<(), Fault> {
        let limit = self.limits.max_stack_values;
        if self.handlers.len() >= limit {
            return Err(Fault::StackOverflow(format!(
                "more than {limit} handlers or pending completions are live"
            )));
        }
        self.handlers.push(Handler {
            frame: self.frame_index(),
            // A native may catch while consuming a delivered result. Its
            // exception continuation resumes with an empty operand area.
            height: match &kind {
                HandlerKind::NativeCatch { native } => self.natives[*native].base(),
                _ => self.stack.len(),
            },
            kind,
        });
        Ok(())
    }

    /// The innermost handler-stack entry, if it belongs to the running frame.
    fn own_handler(&mut self) -> Option<&mut Handler> {
        let frame = self.frame_index();
        self.handlers.last_mut().filter(|h| h.frame == frame)
    }

    /// `PopCatch`: pop the running frame's innermost entry, a catch handler.
    pub(crate) fn pop_catch(&mut self) -> Result<(), Fault> {
        match self.own_handler() {
            Some(Handler {
                kind: HandlerKind::Catch { .. },
                ..
            }) => {
                self.handlers.pop();
                Ok(())
            }
            _ => Err(Fault::Internal(
                "pop_catch without a catch handler of its frame",
            )),
        }
    }

    /// `PopFinally`: the running frame's innermost entry, a finally handler,
    /// becomes the pending normal completion of its finally block.
    pub(crate) fn pop_finally(&mut self) -> Result<(), Fault> {
        match self.own_handler() {
            Some(h) if matches!(h.kind, HandlerKind::Finally { .. }) => {
                h.kind = HandlerKind::Pending(Completion::Normal);
                Ok(())
            }
            _ => Err(Fault::Internal(
                "pop_finally without a finally handler of its frame",
            )),
        }
    }

    /// `EndFinally`: pop the running frame's innermost entry, a pending
    /// completion, for the caller to resume.
    pub(crate) fn end_finally(&mut self) -> Result<Completion, Fault> {
        if !matches!(
            self.own_handler(),
            Some(Handler {
                kind: HandlerKind::Pending(_),
                ..
            })
        ) {
            return Err(Fault::Internal(
                "end_finally without a pending completion of its frame",
            ));
        }
        match self.handlers.pop() {
            Some(Handler {
                kind: HandlerKind::Pending(Completion::Jump(resume)),
                height,
                ..
            }) => {
                // The resume point was validated at the depth of its
                // `CallFinally`, which ran at this height.
                if self.stack.len() != height {
                    return Err(Fault::Internal(
                        "end_finally of a jump with the operand stack off its handler's height",
                    ));
                }
                self.top_mut().pc = resume as usize;
                Ok(Completion::Jump(resume))
            }
            Some(Handler {
                kind: HandlerKind::Pending(completion),
                ..
            }) => Ok(completion),
            _ => Err(Fault::Internal("handler stack changed")),
        }
    }

    /// `CallFinally`: the running frame's innermost entry, a finally handler,
    /// becomes a pending jump completion resuming at the running frame's
    /// next instruction (its saved `pc`, already past the `CallFinally`),
    /// and execution continues at the handler's finally block. The operand
    /// stack must be at the handler's height, so the resume point is entered
    /// at the depth the validator checked.
    pub(crate) fn call_finally(&mut self) -> Result<(), Fault> {
        let len = self.stack.len();
        let resume = self.top().pc;
        let resume = u32::try_from(resume)
            .map_err(|_| Fault::Internal("call_finally resume point out of range"))?;
        let target = match self.own_handler() {
            Some(h) if h.height != len => {
                return Err(Fault::Internal(
                    "call_finally with the operand stack off its finally handler's height",
                ));
            }
            Some(h) => match h.kind {
                HandlerKind::Finally { target } => {
                    h.kind = HandlerKind::Pending(Completion::Jump(resume));
                    target
                }
                _ => return Err(Fault::Internal("call_finally without a finally handler")),
            },
            None => return Err(Fault::Internal("call_finally without a finally handler")),
        };
        self.top_mut().pc = target as usize;
        Ok(())
    }

    /// `DropCompletion`: pop the running frame's innermost entry, a pending
    /// completion, without resuming it (a `break` or `continue` leaves its
    /// finally block). Returns false, leaving it in place, when it is a
    /// pending cancellation: nothing may override that, so the cleanup ends
    /// (`crate::task`, "Cancellation and finally blocks").
    pub(crate) fn drop_completion(&mut self) -> Result<bool, Fault> {
        match self.own_handler() {
            Some(Handler {
                kind: HandlerKind::Pending(Completion::Cancel),
                ..
            }) => Ok(false),
            Some(Handler {
                kind: HandlerKind::Pending(_),
                ..
            }) => {
                self.handlers.pop();
                Ok(true)
            }
            _ => Err(Fault::Internal(
                "drop_completion without a pending completion of its frame",
            )),
        }
    }

    /// True when some catch or finally handler would see an exception raised
    /// now.
    pub(crate) fn has_handlers(&self) -> bool {
        self.handlers
            .iter()
            .any(|h| !matches!(h.kind, HandlerKind::Pending(_)))
    }

    /// IteratorClose preserves a pending throw over errors in `return`.
    pub(crate) fn pending_throw(&self) -> bool {
        self.handlers.last().is_some_and(|h| {
            h.frame == self.frame_index()
                && matches!(h.kind, HandlerKind::Pending(Completion::Throw(_)))
        })
    }

    pub(crate) fn pending_exception(&self) -> Value {
        match self.handlers.last() {
            Some(h) if h.frame == self.frame_index() => match &h.kind {
                HandlerKind::Pending(Completion::Throw(exception)) => exception.value.clone(),
                _ => Value::Undefined,
            },
            _ => Value::Undefined,
        }
    }

    pub(crate) fn pending_cancel(&self) -> bool {
        self.handlers.last().is_some_and(|h| {
            h.frame == self.frame_index()
                && matches!(h.kind, HandlerKind::Pending(Completion::Cancel))
        })
    }

    /// A suspension must not detach this frame's pending cancellation and
    /// allow its caller to continue ordinary execution during cleanup.
    pub(crate) fn activation_cancelling(&self) -> bool {
        self.handlers.iter().any(|h| {
            h.frame == self.frame_index()
                && matches!(h.kind, HandlerKind::Pending(Completion::Cancel))
        })
    }

    /// Return `value` from the running frame. If the frame has a finally
    /// handler, the innermost one becomes a pending `Return` completion and
    /// its block runs first (entries above it are discarded: catch handlers,
    /// and pending completions that this return overrides); otherwise the
    /// frame is left as by [`Self::leave`].
    pub(crate) fn return_value(
        &mut self,
        heap: &crate::heap::Heap,
        mut value: Value,
    ) -> Result<Returned, Fault> {
        let frame = self.frame_index();
        while let Some(h) = self.handlers.last().filter(|h| h.frame == frame) {
            if let HandlerKind::Pending(Completion::Cancel) = h.kind {
                return Ok(Returned::Cancelled);
            }
            if let HandlerKind::Finally { target } = h.kind {
                let height = h.height;
                if height < self.top().floor() || height > self.stack.len() {
                    return Err(Fault::Internal(
                        "operand stack below a finally handler's height",
                    ));
                }
                self.stack.truncate(height);
                if let Some(h) = self.handlers.last_mut() {
                    h.kind = HandlerKind::Pending(Completion::Return(value));
                }
                self.top_mut().pc = target as usize;
                return Ok(Returned::Finally);
            }
            self.handlers.pop();
        }
        if self.top().construct
            && self.top().code.constructor_kind.is_derived()
            && !matches!(value, Value::Object(_))
        {
            if !matches!(value, Value::Undefined) {
                return Err(Fault::TypeError(
                    "derived constructor must return an object or undefined".into(),
                ));
            }
            value = super::dispatch::resolve_this(heap, self.diag.task, &self.top().this)?;
        }
        Ok(match self.leave(value) {
            Some(result) => Returned::Complete(result),
            None => Returned::Left,
        })
    }

    /// Raise `exception`: unwind to the innermost catch or finally handler
    /// (popping the frames above its frame, every pending native frame
    /// started by its frame or above, which receives the failure first, and
    /// every pending completion passed on the way) and continue there, or
    /// report that nothing catches it (`crate::vm::native`, "Errors"). Never
    /// recurses, whatever the number of frames.
    pub(crate) fn raise(&mut self, exception: Exception) -> Raised {
        while let Some(h) = self.handlers.pop() {
            if let HandlerKind::NativeCatch { native } = h.kind {
                self.truncate_calls(h.frame);
                self.stack.truncate(h.height);
                if let Err(message) = native::catch(&mut self.natives, native, exception) {
                    return Raised::Broken(message);
                }
                return Raised::Caught;
            }
            let target = match h.kind {
                HandlerKind::Catch { target } | HandlerKind::Finally { target } => target,
                HandlerKind::Pending(Completion::Cancel) => return Raised::Cancelled,
                HandlerKind::Pending(_) => continue,
                HandlerKind::NativeCatch { .. } => unreachable!(),
            };
            // The handler's frame must still be live, and its operands must
            // not have dropped below the handler's height since it was
            // pushed (only hand-assembled code could do either).
            let above = match self.calls.get(h.frame) {
                Some(callee) => callee.base,
                None => self.stack.len(),
            };
            if h.frame > self.calls.len() || above < h.height {
                return Raised::Broken("operand stack below a handler's height");
            }
            // The natives this exception passes receive the failure and are
            // released; their operand areas lie above the handler's height
            // (the handler was pushed before they started).
            if let Err(message) = native::unwind(&mut self.natives, h.frame) {
                return Raised::Broken(message);
            }
            self.truncate_calls(h.frame);
            if h.height < self.top().floor() {
                return Raised::Broken("handler height below its frame's operands");
            }
            self.stack.truncate(h.height);
            self.top_mut().pc = target as usize;
            match h.kind {
                HandlerKind::Catch { .. } => self.stack.push(exception.value),
                _ => self.handlers.push(Handler {
                    kind: HandlerKind::Pending(Completion::Throw(exception)),
                    ..h
                }),
            }
            return Raised::Caught;
        }
        Raised::Uncaught(exception)
    }

    pub(crate) fn pop_native_catch(&mut self, native: usize) {
        if self.handlers.last().is_some_and(|h| {
            matches!(h.kind,
            HandlerKind::NativeCatch { native: index } if index == native)
        }) {
            self.handlers.pop();
        }
    }

    /// True when some finally handler would run for a cancellation observed
    /// now (some may still be refused by [`Self::unwind_cancel`]).
    pub(crate) fn has_finally(&self) -> bool {
        self.handlers
            .iter()
            .any(|h| matches!(h.kind, HandlerKind::Finally { .. }))
    }

    /// Unwind the task's consumer cancellation to the innermost finally
    /// handler (`crate::task`, "Cancellation and finally blocks"): like
    /// [`Self::raise`], but catch handlers are skipped, and the finally
    /// handler becomes a pending [`Completion::Cancel`]. Pending completions
    /// passed on the way are discarded (the cancellation overrides them), and
    /// so are pending native frames, without running native code
    /// (`crate::vm::native`, "Errors"). Returns true if a finally block runs
    /// next; false when none is left or the next one contradicts the
    /// frames: the cleanup is over.
    pub(crate) fn unwind_cancel(&mut self, heap: &crate::heap::Heap) -> bool {
        while let Some(h) = self.handlers.pop() {
            let HandlerKind::Finally { target } = h.kind else {
                continue;
            };
            let above = match self.calls.get(h.frame) {
                Some(callee) => callee.base,
                None => self.stack.len(),
            };
            if h.frame > self.calls.len() || above < h.height {
                return false;
            }
            native::discard(&mut self.natives, h.frame, heap);
            self.truncate_calls(h.frame);
            if h.height < self.top().floor() {
                return false;
            }
            self.stack.truncate(h.height);
            self.top_mut().pc = target as usize;
            self.handlers.push(Handler {
                kind: HandlerKind::Pending(Completion::Cancel),
                ..h
            });
            return true;
        }
        native::discard(&mut self.natives, 0, heap);
        false
    }

    /// Snapshot at most 64 JS frames and 16,384 UTF-16 units, innermost first.
    /// Fields are capped before copying; the final line may be truncated.
    /// Native callbacks use
    /// their initiating JS instruction's saved position, just like diagnostics.
    /// This reads immutable code metadata only: no getters, heap edges, or
    /// worker/Rust-stack state. The resulting string survives frame teardown.
    pub(crate) fn error_stack(&self, pc: usize) -> Result<JsString, Fault> {
        self.error_stack_with_input(pc, None)
    }

    /// An input pseudo-frame shares the snapshot's total bound with JS callers.
    pub(crate) fn error_stack_with_input(
        &self,
        pc: usize,
        input: Option<(&'static str, usize, usize)>,
    ) -> Result<JsString, Fault> {
        self.error_stack_with_limit::<ERROR_STACK_UNITS>(pc, input)
    }

    fn error_stack_with_limit<const LIMIT: usize>(
        &self,
        pc: usize,
        input: Option<(&'static str, usize, usize)>,
    ) -> Result<JsString, Fault> {
        use std::fmt::Write;

        let limit = LIMIT.min(crate::value::string::MAX_STRING_LEN);
        let mut units = Vec::new();
        // Same checked reservation/owned-buffer path as engine messages in
        // raise_failure. The hook forces a capacity error, never a large alloc.
        #[cfg(test)]
        let reservation = if FAIL_STACK_RESERVATION.replace(false) {
            usize::MAX
        } else {
            limit
        };
        #[cfg(not(test))]
        let reservation = limit;
        units
            .try_reserve_exact(reservation)
            .map_err(|_| Fault::OutOfMemory("error stack allocation failed"))?;
        let mut stack = ErrorStackSnapshot { units, limit };
        if let Some((name, line, column)) = input {
            // Names are internal static labels, never user source or getters.
            let _ = writeln!(stack, "    at {name}:{line}:{column}");
        }
        for (index, frame) in std::iter::once(&self.entry)
            .chain(&self.calls)
            .rev()
            .take(64)
            .enumerate()
        {
            if stack.units.len() == limit {
                break;
            }
            let at = if index == 0 {
                pc
            } else {
                frame.pc.saturating_sub(1)
            };
            let code = &frame.code;
            stack.text("    at ");
            if code.function_name.is_empty() {
                stack.text("<anonymous>");
            } else if code.function_name.len() > ERROR_STACK_METADATA_UNITS
                || code.function_name.len() > limit - stack.units.len()
            {
                // Do not flatten or even traverse oversized/deep rope names.
                stack.text("<truncated>");
            } else {
                // Inferred names may contain arbitrary UTF-16, including
                // lone surrogates. Preserve them without a Rust str round trip.
                code.function_name
                    .try_copy_code_units_to(&mut stack.units)
                    .ok_or(Fault::OutOfMemory("error stack allocation failed"))?;
            }
            let file = code.debug.as_ref().map_or(&*code.name, |d| &*d.filename);
            stack.text(" (");
            let mut file = file.encode_utf16();
            stack.append(file.by_ref().take(ERROR_STACK_METADATA_UNITS));
            if file.next().is_some() {
                stack.text("…");
            }
            if let Some(source) = code.location(at) {
                // Formatting writes into the reserved buffer without a String.
                let _ = write!(stack, ":{}:{}", source.line, source.column);
            }
            stack.text(")\n");
        }
        JsString::from_owned_code_units(stack.units).ok_or(Fault::StringTooLong)
    }

    /// A failure at instruction `pc` of the running frame, with the call
    /// chain of its callers.
    pub(crate) fn failure(&self, kind: VmErrorKind, message: String, pc: usize) -> VmError {
        let code = &self.top().code;
        // Callers, innermost first; each one's saved `pc` is just past its
        // `Call`.
        let callers = std::iter::once(&self.entry)
            .chain(&self.calls)
            .rev()
            .skip(1);
        let call_chain = callers
            .map(|frame| {
                let at = frame.pc.saturating_sub(1);
                CallSite {
                    executable: frame.code.name.clone(),
                    instruction: at,
                    byte_offset: frame.code.offsets.get(at).copied(),
                    source: frame.code.location(at),
                }
            })
            .collect();
        VmError {
            kind,
            message,
            diagnostic: Diagnostic {
                executable: code.name.clone(),
                task: self.diag.task,
                world: self.diag.world,
                instruction: pc,
                byte_offset: code.offsets.get(pc).copied(),
                opcode: code.instrs.get(pc).map(|i| i.opcode()),
                source: code.location(pc),
                call_chain,
            },
        }
    }

    /// The running frame's saved instruction position, operands and locals
    /// (tests).
    #[cfg(test)]
    pub(crate) fn snapshot(&self) -> (usize, Vec<Value>, Vec<Value>) {
        let top = self.top();
        let locals = self.stack[top.base..top.floor()].to_vec();
        (top.pc, self.operands().to_vec(), locals)
    }

    /// The code of every live frame, entry first (tests).
    #[cfg(test)]
    pub(crate) fn frame_code(&self) -> Vec<Arc<Code>> {
        std::iter::once(&self.entry)
            .chain(&self.calls)
            .map(|f| f.code.shared())
            .collect()
    }
}

/// Every frame's locals, operands, function object and `this`, and the values held
/// by pending completions (saved return values and exceptions). A frame's code
/// constants are reached through its function object (whose `Trace` reports
/// them) or, for the entry frame, through the task's executable
/// (`RootCategory::ExecutableConstant`). Pending native frames report their
/// arguments and intermediate values. The builder list is deliberately not
/// traced.
impl Trace for VmState {
    fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
        if let Some(module) = &self.module_link {
            module.trace(visit);
        }
        if let Some(host) = &self.sim_host {
            host.trace(visit);
        }
        self.kept_objects.values().for_each(|v| v.trace(visit));
        if let Some(value) = &self.script_result {
            value.trace(visit);
        }
        if let Some(id) = self.global_object {
            visit(id.into());
        }
        self.global_lexicals
            .values()
            .for_each(|b| b.cell.trace(visit));
        self.stack.iter().for_each(|v| v.trace(visit));
        for frame in std::iter::once(&self.entry).chain(&self.calls) {
            if let Some(id) = frame.callee {
                visit(id.into());
            }
            frame.this.trace(visit);
            frame.new_target.trace(visit);
            if let Some(args) = &frame.arguments {
                args.iter().for_each(|v| v.trace(visit));
            }
        }
        self.handlers.iter().for_each(|h| h.trace(visit));
        self.natives.iter().for_each(|n| n.trace(visit));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vm::Assembler;
    use crate::vm::instr::Opcode;

    #[test]
    fn guarded_remainder_matches_f64_bits() {
        fn check(a: f64, b: f64) {
            let actual = number_remainder(a, b);
            let expected = a % b;
            if expected.is_nan() {
                assert!(actual.is_nan(), "{a:?} % {b:?}");
            } else {
                assert_eq!(actual.to_bits(), expected.to_bits(), "{a:?} % {b:?}");
            }
        }
        let edges = [
            -0.0,
            0.0,
            -1.0,
            1.0,
            -3.0,
            3.0,
            -7.0,
            7.0,
            -1.5,
            1.5,
            f64::NAN,
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::MAX,
            -f64::MAX,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::from(i32::MIN) - 1.0,
            f64::from(i32::MIN),
            f64::from(i32::MIN) + 0.5,
            f64::from(i32::MAX) - 0.5,
            f64::from(i32::MAX),
            f64::from(i32::MAX) + 1.0,
            9007199254740991.0,
            9007199254740992.0,
            9007199254740994.0,
            -9007199254740992.0,
            i64::MAX as f64,
            i64::MIN as f64,
        ];
        for a in edges {
            for b in edges {
                check(a, b);
            }
        }
        // Deterministic broad coverage of both guarded integer pairs and
        // arbitrary IEEE bit patterns; no random seed or host JS dependency.
        let mut bits = 0x9e3779b97f4a7c15_u64;
        for _ in 0..20_000 {
            bits ^= bits << 13;
            bits ^= bits >> 7;
            bits ^= bits << 17;
            let next = bits.rotate_left(29);
            check(f64::from(bits as i32), f64::from(next as i32));
            check(f64::from_bits(bits), f64::from_bits(next));
        }
    }

    #[test]
    fn call_handles_keep_shared_counts_constant_and_tasks_independent() {
        let exe = Assembler::new()
            .op(Opcode::Undefined)
            .op(Opcode::Return)
            .finish("shared", 0, 0)
            .unwrap();
        let a = CodeCache::default();
        let b = CodeCache::default();
        let ca = a.get(&exe.0);
        let cb = b.get(&exe.0);
        assert!(!Arc::ptr_eq(&ca.0, &cb.0));
        assert!(Arc::ptr_eq(&ca.0, &a.get(&exe.0).0));
        let count = Arc::strong_count(&exe.0);
        let calls: Vec<_> = (0..1_000).map(|_| a.get(&exe.0)).collect();
        assert_eq!(Arc::strong_count(&exe.0), count);
        drop(calls);
        assert_eq!(Arc::strong_count(&exe.0), count);
    }

    #[test]
    fn detached_code_owns_its_lifetime_after_cache_eviction_and_migration() {
        let exe = Assembler::new()
            .op(Opcode::Undefined)
            .op(Opcode::Return)
            .finish("detached", 0, 0)
            .unwrap();
        let weak = Arc::downgrade(&exe.0);
        let mut state = VmState::new(
            &exe.0,
            &[],
            DiagnosticContext {
                task: TaskId::from_raw(1),
                world: WorldId::INITIAL,
            },
            VmLimits::DEFAULT,
        );
        let saved = state.detach_activation().ok().unwrap();
        // More distinct code than the cache can retain, including the
        // detached frame's former entry. Eviction must not invalidate it.
        for _ in 0..256 {
            let other = Assembler::new()
                .op(Opcode::Undefined)
                .op(Opcode::Return)
                .finish("other", 0, 0)
                .unwrap();
            state.code_cache.get(&other.0);
        }
        assert!(
            !state
                .code_cache
                .0
                .borrow()
                .contains_key(&(Arc::as_ptr(&exe.0) as usize))
        );
        drop(state);
        drop(exe);
        assert!(weak.upgrade().is_some());
        std::thread::spawn(move || {
            assert_eq!(&*saved.frame.code.name, "detached");
            assert_eq!(saved.frame.code.instrs.len(), 2);
            drop(saved);
        })
        .join()
        .unwrap();
        assert!(weak.upgrade().is_none());
    }

    fn metadata_state(name: JsString, filename: &str, frames: usize) -> VmState {
        // Early VM fixture: loaded metadata and repeated explicit frames.
        let exe = Assembler::new()
            .op(Opcode::Undefined)
            .op(Opcode::Return)
            .finish(filename, 0, 0)
            .unwrap();
        let mut spec = exe.to_spec();
        spec.function_name = Some(name);
        let exe = crate::vm::Executable::load(spec).unwrap();
        let diag = DiagnosticContext {
            task: TaskId::from_raw(1),
            world: WorldId::INITIAL,
        };
        let mut vm = VmState::new(&exe.0, &[], diag, VmLimits::DEFAULT);
        for _ in 1..frames {
            vm.calls
                .push(VmState::new(&exe.0, &[], diag, VmLimits::DEFAULT).entry);
        }
        vm
    }

    #[test]
    fn error_stack_caps_metadata_before_copying() {
        // Early VM fixture: loaded rope metadata and repeated explicit frames.
        // A reduced snapshot limit reproduces overflow without large buffers.
        let mut name = JsString::from("rope-name");
        for _ in 0..10 {
            name = name.concat(&name).unwrap();
        }
        let vm = metadata_state(name, "filename", 4);
        let before = JsString::copied_code_units();
        let stack = vm.error_stack_with_limit::<64>(0, None).ok().unwrap();
        assert!(stack.len() <= 64);
        assert!(JsString::copied_code_units() - before <= 64);
        assert!(
            stack
                .to_string_lossy()
                .starts_with("    at <truncated> (filename)\n")
        );
    }

    #[test]
    fn error_stack_preserves_bounded_rope_names_and_caps_filenames() {
        let expected_name: Vec<u16> = (0..256)
            .map(|i| match i {
                0 => 0xd800,
                129 => 0xdc00,
                _ => u16::from(b'a') + i / 32,
            })
            .collect();
        // Distinct children and a skewed, unflattened rope detect reversed
        // traversal or duplicated children, while retaining lone surrogates.
        let mut name = JsString::from_code_units(expected_name[..32].to_vec()).unwrap();
        for chunk in expected_name[32..].chunks(32) {
            name = name
                .concat(&JsString::from_code_units(chunk.to_vec()).unwrap())
                .unwrap();
        }
        let filename = "😀".repeat(300);
        let vm = metadata_state(name, &filename, 100);
        let before = JsString::copied_code_units();
        let stack = vm.error_stack(0).ok().unwrap();
        assert_eq!(stack.len(), ERROR_STACK_UNITS);
        assert!(JsString::copied_code_units() - before <= ERROR_STACK_UNITS as u64);
        let units = stack.code_units();
        assert_eq!(&units[7..263], expected_name);
        let file_start = 265;
        assert_eq!(
            &units[file_start..file_start + 256],
            "😀".repeat(128).encode_utf16().collect::<Vec<_>>()
        );
        assert_eq!(units[file_start + 256], '…' as u16);
        assert!(
            vm.error_stack_with_limit::<0>(0, None)
                .ok()
                .unwrap()
                .is_empty()
        );

        let vm = metadata_state(JsString::from("name"), "file", 1);
        let expected = "    at name (file)\n";
        assert_eq!(
            vm.error_stack_with_limit::<19>(0, None).ok().unwrap(),
            expected
        );
        assert_eq!(
            vm.error_stack_with_limit::<20>(0, None).ok().unwrap(),
            expected
        );
        assert_eq!(
            vm.error_stack_with_limit::<18>(0, None).ok().unwrap(),
            "    at name (file)"
        );
    }

    #[test]
    fn input_position_shares_the_stack_snapshot_bound() {
        let vm = metadata_state(JsString::from("name"), "file", 100);
        for limit in [0, 8, 32, ERROR_STACK_UNITS] {
            // Exercise an input prefix longer than the entire snapshot too.
            let input = Some(("<eval>", usize::MAX, usize::MAX));
            let stack = match limit {
                0 => vm.error_stack_with_limit::<0>(0, input),
                8 => vm.error_stack_with_limit::<8>(0, input),
                32 => vm.error_stack_with_limit::<32>(0, input),
                _ => vm.error_stack_with_input(0, input),
            }
            .ok()
            .unwrap();
            assert!(stack.len() <= limit);
            let prefix = format!("    at <eval>:{}:{}\n", usize::MAX, usize::MAX);
            let count = stack.len().min(prefix.len());
            assert_eq!(&stack.to_string_lossy()[..count], &prefix[..count]);
        }
        let stack = vm
            .error_stack_with_limit::<64>(0, Some(("<Function>", 2, 6)))
            .ok()
            .unwrap();
        assert_eq!(stack.len(), 64);
        assert!(
            stack
                .to_string_lossy()
                .starts_with("    at <Function>:2:6\n    at name (file)\n")
        );
    }

    #[test]
    fn error_stack_allocation_failure_is_uncatchable() {
        use crate::heap::Heap;
        use crate::host::NoHost;
        use crate::vm::dispatch::{Exit, execute};

        // Synthetic fixtures, not light-cljs output. Cover constructors,
        // engine-error wrapping, suppression and disposal snapshot callers.
        for operation in [
            "Error('x');",
            "new TypeError('x');",
            "missing;",
            "SuppressedError(1, 2);",
            "using x = {[Symbol.dispose]() { throw 1; }}; throw 2;",
            "let s = new DisposableStack(); s.dispose();",
        ] {
            let source = format!("try {{ {operation} }} catch (e) {{}} 42;");
            let exe = crate::frontend::compile_script("stack-allocation.js", &source).unwrap();
            let heap = Heap::default();
            let diag = DiagnosticContext {
                task: TaskId::from_raw(1),
                world: WorldId::INITIAL,
            };
            let mut vm = VmState::new(&exe.0, &[], diag, VmLimits::DEFAULT);
            FAIL_STACK_RESERVATION.set(true);
            let Exit::Fail(error) = execute(&mut vm, &heap, &mut NoHost, None) else {
                panic!("stack reservation must fail without entering catch: {operation}");
            };
            assert_eq!(error.kind, VmErrorKind::OutOfMemory, "{operation}");
            assert_eq!(
                error.message, "error stack allocation failed",
                "{operation}"
            );
            assert!(!FAIL_STACK_RESERVATION.get(), "hook consumed once");
            let mut vm = VmState::new(&exe.0, &[], diag, VmLimits::DEFAULT);
            assert!(matches!(
                execute(&mut vm, &heap, &mut NoHost, None),
                Exit::Return(Value::Number(42.0))
            ));
        }
    }

    #[test]
    fn ignored_entry_arguments_do_not_grow_saved_storage() {
        // function f(a) { let b; return b; }, given a million arguments.
        let f = Assembler::new()
            .get_local(1)
            .op(Opcode::Return)
            .finish("f", 1, 2)
            .unwrap();
        let args = vec![Value::Number(7.0); 1_000_000];
        let diag = DiagnosticContext {
            task: TaskId::from_raw(1),
            world: WorldId::INITIAL,
        };
        let vm = VmState::new(&f.0, &args, diag, VmLimits::DEFAULT);
        assert_eq!(vm.stack, [Value::Number(7.0), Value::Undefined]);
        assert!(vm.stack.capacity() < 16, "{}", vm.stack.capacity());
    }
}
