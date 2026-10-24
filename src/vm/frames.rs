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
//! frame's locals and operands) and each frame's function object, which
//! reports its code's constants and its captured values (the closure
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

use std::sync::Arc;

use crate::collections::OwnedBuilders;
use crate::error::{CallSite, Diagnostic, VmError, VmErrorKind};
use crate::heap::ObjectId;
use crate::heap::gc::Trace;
use crate::ids::{DependencyKey, TaskId, WorldId};
use crate::value::Value;
use crate::vm::dispatch::Fault;
use crate::vm::exception::{Completion, Exception, Handler, HandlerKind};
use crate::vm::executable::Code;
use crate::vm::native::{self, NativeFrame};

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
    /// The shared code this frame runs (never copied per task or call).
    pub(crate) code: Arc<Code>,
    /// The function object this frame was called through, for
    /// `CurrentFunction` and for `GetCapture` (its captured values are the
    /// frame's closure environment). `None` for the entry frame until
    /// `CurrentFunction` first allocates one.
    pub(crate) callee: Option<ObjectId>,
    /// Index of the next instruction to execute in `code`.
    pub(crate) pc: usize,
    /// Index of this frame's local 0 in [`VmState::stack`].
    pub(crate) base: usize,
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

/// The complete VM state of one task.
#[derive(Debug)]
pub(crate) struct VmState {
    /// The frame the task started in.
    entry: Frame,
    /// Frames called from the entry frame, innermost last.
    calls: Vec<Frame>,
    /// Locals and operands of every frame (see the module docs).
    stack: Vec<Value>,
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
        VmState {
            entry: Frame {
                code: Arc::clone(code),
                callee: None,
                pc: 0,
                base: 0,
            },
            calls: Vec::new(),
            stack,
            limits,
            diag,
            builders: OwnedBuilders::default(),
            pending_need: None,
            handlers: Vec::new(),
            natives: Vec::new(),
            cancelling: false,
        }
    }

    /// The running (innermost) frame.
    pub(crate) fn top(&self) -> &Frame {
        self.calls.last().unwrap_or(&self.entry)
    }

    pub(crate) fn top_mut(&mut self) -> &mut Frame {
        self.calls.last_mut().unwrap_or(&mut self.entry)
    }

    /// Number of live frames, the entry frame included.
    pub(crate) fn depth(&self) -> usize {
        1 + self.calls.len()
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
    /// replace. Fails, changing nothing, if the operands are missing or the
    /// new frame would exceed the task's limits.
    pub(crate) fn enter(
        &mut self,
        code: Arc<Code>,
        callee: ObjectId,
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
            .saturating_add(code.max_stack);
        if needed > self.limits.max_stack_values {
            return Err(Fault::StackOverflow(format!(
                "a call needing {needed} stack values exceeds the limit of {}",
                self.limits.max_stack_values
            )));
        }
        // Shift the arguments down over the callee slot, then bind them.
        self.stack.remove(base);
        bind_args(&mut self.stack, base, &code, argc);
        self.stack.reserve(code.max_stack);
        self.calls.push(Frame {
            code,
            callee: Some(callee),
            pc: 0,
            base,
        });
        Ok(())
    }

    /// Leave the running frame with `result`. Returns `Some(result)` when it
    /// was the entry frame (the task completes); otherwise pops the frame,
    /// discarding its locals and operands, and pushes `result` for the caller.
    fn leave(&mut self, result: Value) -> Option<Value> {
        let frame = self.frame_index();
        while self.handlers.last().is_some_and(|h| h.frame >= frame) {
            self.handlers.pop();
        }
        match self.calls.pop() {
            None => Some(result),
            Some(frame) => {
                self.stack.truncate(frame.base);
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
            height: self.stack.len(),
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

    /// Return `value` from the running frame. If the frame has a finally
    /// handler, the innermost one becomes a pending `Return` completion and
    /// its block runs first (entries above it are discarded: catch handlers,
    /// and pending completions that this return overrides); otherwise the
    /// frame is left as by [`Self::leave`].
    pub(crate) fn return_value(&mut self, value: Value) -> Result<Returned, Fault> {
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
            let target = match h.kind {
                HandlerKind::Catch { target } | HandlerKind::Finally { target } => target,
                HandlerKind::Pending(Completion::Cancel) => return Raised::Cancelled,
                HandlerKind::Pending(_) => continue,
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
            self.calls.truncate(h.frame);
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
    pub(crate) fn unwind_cancel(&mut self) -> bool {
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
            native::discard(&mut self.natives, h.frame);
            self.calls.truncate(h.frame);
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
        false
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
            .map(|f| Arc::clone(&f.code))
            .collect()
    }
}

/// Every frame's locals, operands and function object, and the values held
/// by pending completions (saved return values and exceptions). A frame's code
/// constants are reached through its function object (whose `Trace` reports
/// them) or, for the entry frame, through the task's executable
/// (`RootCategory::ExecutableConstant`). Pending native frames report their
/// arguments and intermediate values. The builder list is deliberately not
/// traced.
impl Trace for VmState {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        self.stack.iter().for_each(|v| v.trace(visit));
        for frame in std::iter::once(&self.entry).chain(&self.calls) {
            if let Some(id) = frame.callee {
                visit(id);
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
