//! Tasks, the execution guard, and run outcomes.
//!
//! A [`Task`] owns its VM state (`VmState`: instruction position, operands,
//! locals, diagnostic context). It is a cheap-to-clone, `Send + Sync` handle so
//! an embedder's scheduler can keep it in queues and hand it to whichever of
//! its own threads is free; the runtime owns no threads (`docs/design.md` §3).
//!
//! **Execution guard.** Running requires a [`RunGuard`], obtained with
//! [`Task::try_begin_run`]. Acquisition never blocks: if another thread (or
//! the same thread) already holds the guard, it fails with
//! [`RunError::AlreadyRunning`]. So a task can never execute on two workers at
//! once, and a worker only borrows task state for the duration of one run.
//! The guard is a per-task `Mutex` acquired with `try_lock`; it is never
//! waited on and never held across a suspension (runs return before any
//! wait), so it is an ownership token rather than a lock that serializes work.
//!
//! **Budgeted runs.** [`Task::run_with`] takes a [`Budget`] and a
//! [`Host`]. When the budget runs out the run returns
//! [`RunOutcome::Yielded`], releases the guard, and leaves the task `Ready`
//! with its VM state saved; running it again (on any thread) continues at the
//! saved instruction. No Rust stack frame survives between runs.

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use crate::error::{VmError, VmErrorKind};
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId, Rooted};
use crate::host::{Host, NoHost};
use crate::ids::{TaskId, WaitKey, WorldId};
#[cfg(test)]
use crate::value::Value;
use crate::vm::Executable;
use crate::vm::dispatch::{Exit, execute};
use crate::vm::frames::VmState;

/// Result of one run of a task.
#[derive(Debug)]
pub enum RunOutcome {
    /// The task returned a value, delivered as a persistent root so it stays
    /// valid after the task is dropped.
    Complete(Rooted),
    /// The task failed at run time.
    Failed(VmError),
    /// Reserved: the task is suspended waiting on a dependency. Not produced
    /// by this slice (dependency cells are pellet clj-quickjs-6 and later).
    Waiting(WaitKey),
    /// The task used up its execution [`Budget`] before finishing. Its
    /// instruction position, operands and locals are saved in the task; the
    /// next run (on any thread) continues at the saved instruction.
    Yielded,
}

/// How many instructions one run may execute before it returns
/// [`RunOutcome::Yielded`].
///
/// The budget is checked between instructions only, so every instruction
/// (including a leaf host effect) executes completely in exactly one run. A
/// budget of zero yields immediately without executing anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Budget(Option<u64>);

impl Budget {
    /// Run until the task completes or fails.
    pub const UNLIMITED: Budget = Budget(None);

    /// Execute at most `n` instructions in this run.
    pub const fn instructions(n: u64) -> Budget {
        Budget(Some(n))
    }
}

/// Why a run could not start.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunError {
    /// The task is currently being run (its execution guard is held).
    AlreadyRunning,
    /// The task already completed or failed; its VM state is gone.
    Finished,
    /// A previous run panicked while holding the guard, so the task state may
    /// be inconsistent. The VM does not panic by design; this is defensive.
    Poisoned,
    /// The calling thread is inside a leaf host effect, which may not reenter
    /// the VM (see [`crate::host`]).
    InsideHostEffect,
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RunError::AlreadyRunning => "task is already running",
            RunError::Finished => "task has finished",
            RunError::Poisoned => "task state is poisoned by an earlier panic",
            RunError::InsideHostEffect => "a host effect may not reenter the VM",
        })
    }
}

impl std::error::Error for RunError {}

/// Observable task status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskStatus {
    /// Can be run.
    Ready,
    /// Its execution guard is held.
    Running,
    /// Completed or failed.
    Finished,
}

/// Task-owned state behind the execution guard.
#[derive(Debug)]
enum TaskBody {
    Ready(VmState),
    Finished,
}

impl Trace for TaskBody {
    fn trace(&self, visit: &mut dyn FnMut(ObjectId)) {
        match self {
            TaskBody::Ready(vm) => vm.trace(visit),
            TaskBody::Finished => {}
        }
    }
}

#[derive(Debug)]
struct TaskInner {
    id: TaskId,
    world: WorldId,
    executable: Executable,
    heap: Arc<Heap>,
    body: Mutex<TaskBody>,
}

/// A resumable unit of execution. See the module docs.
#[derive(Clone, Debug)]
pub struct Task(Arc<TaskInner>);

/// Exclusive right to run a task, held for one run. Not `Send`: the run
/// happens on the thread that acquired it.
pub struct RunGuard<'t> {
    task: &'t TaskInner,
    body: MutexGuard<'t, TaskBody>,
}

impl Task {
    pub(crate) fn new(
        id: TaskId,
        world: WorldId,
        executable: Executable,
        heap: Arc<Heap>,
        vm: VmState,
    ) -> Task {
        Task(Arc::new(TaskInner {
            id,
            world,
            executable,
            heap,
            body: Mutex::new(TaskBody::Ready(vm)),
        }))
    }

    pub fn id(&self) -> TaskId {
        self.0.id
    }

    /// The world/version this task executes in.
    pub fn world(&self) -> WorldId {
        self.0.world
    }

    /// The shared code this task runs.
    pub fn executable(&self) -> &Executable {
        &self.0.executable
    }

    /// Current status. A snapshot: it may change as soon as it is returned.
    pub fn status(&self) -> TaskStatus {
        match self.0.body.try_lock() {
            Ok(body) => match *body {
                TaskBody::Ready(_) => TaskStatus::Ready,
                TaskBody::Finished => TaskStatus::Finished,
            },
            Err(TryLockError::WouldBlock) => TaskStatus::Running,
            Err(TryLockError::Poisoned(_)) => TaskStatus::Finished,
        }
    }

    /// Acquire the execution guard without blocking.
    pub fn try_begin_run(&self) -> Result<RunGuard<'_>, RunError> {
        if crate::host::inside_effect() {
            return Err(RunError::InsideHostEffect);
        }
        let body = match self.0.body.try_lock() {
            Ok(body) => body,
            Err(TryLockError::WouldBlock) => return Err(RunError::AlreadyRunning),
            Err(TryLockError::Poisoned(_)) => return Err(RunError::Poisoned),
        };
        match *body {
            TaskBody::Ready(_) => Ok(RunGuard {
                task: &self.0,
                body,
            }),
            TaskBody::Finished => Err(RunError::Finished),
        }
    }

    /// Acquire the guard and run on the calling thread with an unlimited
    /// budget and no host (host effects fail the task).
    pub fn run(&self) -> Result<RunOutcome, RunError> {
        Ok(self.try_begin_run()?.run())
    }

    /// Acquire the guard and run on the calling thread for at most `budget`
    /// instructions, sending leaf host effects to `host`.
    pub fn run_with(&self, budget: Budget, host: &mut dyn Host) -> Result<RunOutcome, RunError> {
        Ok(self.try_begin_run()?.run_with(budget, host))
    }

    /// The saved instruction position (index of the next instruction) and the
    /// saved operands and locals, or `None` while running or finished.
    #[cfg(test)]
    pub(crate) fn saved_state(&self) -> Option<(usize, Vec<Value>, Vec<Value>)> {
        match &*self.0.body.try_lock().ok()? {
            TaskBody::Ready(vm) => Some((
                vm.frame.pc,
                vm.frame.operands.clone(),
                vm.frame.locals.to_vec(),
            )),
            TaskBody::Finished => None,
        }
    }

    /// Visit heap references in the task's VM state; `false` if the state is
    /// not accessible (the task is running). For the future collector, which
    /// reaches tasks at safepoints.
    #[cfg(test)]
    pub(crate) fn trace_frames(&self, visit: &mut dyn FnMut(ObjectId)) -> bool {
        match self.0.body.try_lock() {
            Ok(body) => {
                body.trace(visit);
                true
            }
            Err(_) => false,
        }
    }
}

impl RunGuard<'_> {
    /// The task being run.
    pub fn task_id(&self) -> TaskId {
        self.task.id
    }

    /// Execute until the task returns or fails, then release the guard.
    /// Unlimited budget, no host: see [`Task::run`].
    pub fn run(self) -> RunOutcome {
        self.run_with(Budget::UNLIMITED, &mut NoHost)
    }

    /// Execute until the task returns or fails or `budget` is used up, then
    /// release the guard. See [`Task::run_with`].
    pub fn run_with(mut self, budget: Budget, host: &mut dyn Host) -> RunOutcome {
        let TaskBody::Ready(vm) = &mut *self.body else {
            // `try_begin_run` only hands out guards for ready tasks, and only
            // `run_with` (which consumes the guard) changes the body.
            return RunOutcome::Failed(guard_failure(
                self.task,
                VmErrorKind::Internal,
                "run guard held for a finished task",
                0,
            ));
        };
        if crate::host::inside_effect() {
            // Reached only with a guard acquired before the effect started
            // (`try_begin_run` refuses inside an effect). Fail this task rather
            // than reenter the VM from a leaf effect.
            let error = guard_failure(
                self.task,
                VmErrorKind::HostEffect,
                "a host effect may not reenter the VM",
                vm.frame.pc,
            );
            *self.body = TaskBody::Finished;
            return RunOutcome::Failed(error);
        }
        let outcome = match execute(vm, &self.task.executable.0, &self.task.heap, host, budget.0) {
            Exit::Return(value) => RunOutcome::Complete(Rooted::new(&self.task.heap, value)),
            Exit::Fail(error) => RunOutcome::Failed(error),
            // The VM state stays in the task; dropping the guard releases it.
            Exit::Yield => return RunOutcome::Yielded,
        };
        // Completion or failure releases the task's frames; a returned value
        // stays alive through its persistent root, not through the task.
        *self.body = TaskBody::Finished;
        outcome
    }
}

#[cold]
fn guard_failure(task: &TaskInner, kind: VmErrorKind, message: &str, pc: usize) -> VmError {
    let code = &task.executable.0;
    VmError {
        kind,
        message: message.to_string(),
        diagnostic: crate::error::Diagnostic {
            executable: code.name.clone(),
            task: task.id,
            world: task.world,
            instruction: pc,
            byte_offset: code.offsets.get(pc).copied(),
            opcode: code.instrs.get(pc).map(|i| i.opcode()),
        },
    }
}

/// Early VM tests: hand-assembled bytecode, not JavaScript source support.
#[cfg(test)]
mod tests {
    use super::*;

    use crate::value::{Primitive, ValueView};
    use crate::vm::executable::Code;
    use crate::vm::instr::Instr;
    use crate::vm::{Assembler, Opcode};
    use crate::{Runtime, ValidationErrorKind};

    fn num(n: f64) -> Primitive {
        Primitive::Number(n)
    }

    fn complete(outcome: RunOutcome) -> ValueView {
        match outcome {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    fn failed(outcome: RunOutcome) -> VmError {
        match outcome {
            RunOutcome::Failed(e) => e,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    /// `function f(n) { let acc = 1; while (0 < n) { acc = acc * n; n = n - 1; } return acc; }`
    fn factorial() -> Executable {
        let mut asm = Assembler::new();
        let (top, done) = (asm.label(), asm.label());
        asm.constant(num(1.0)).set_local(1);
        asm.bind(top);
        asm.constant(num(0.0))
            .get_local(0)
            .op(Opcode::Lt)
            .jump_if_false(done);
        asm.get_local(1).get_local(0).op(Opcode::Mul).set_local(1);
        asm.get_local(0)
            .constant(num(1.0))
            .op(Opcode::Sub)
            .set_local(0);
        asm.jump(top);
        asm.bind(done);
        asm.get_local(1).op(Opcode::Return);
        asm.finish("factorial", 1, 2).unwrap()
    }

    #[test]
    fn completes_with_constants_locals_branches_and_return() {
        let rt = Runtime::new();
        let five = rt.root(num(5.0));
        let task = rt.spawn(&factorial(), &[&five]).unwrap();
        assert_eq!(task.status(), TaskStatus::Ready);
        assert_eq!(complete(task.run().unwrap()), ValueView::Number(120.0));
        assert_eq!(task.status(), TaskStatus::Finished);
        assert_eq!(task.run().unwrap_err(), RunError::Finished);
    }

    #[test]
    fn primitive_operations_follow_documented_semantics() {
        use Opcode::*;
        let cases: Vec<(Vec<Primitive>, Vec<Opcode>, ValueView)> = vec![
            (vec![num(7.0), num(2.0)], vec![Sub], ValueView::Number(5.0)),
            (vec![num(1.0), num(2.0)], vec![Lt], ValueView::Bool(true)),
            (vec![num(2.0), num(1.0)], vec![Lt], ValueView::Bool(false)),
            (
                vec![num(0.0), num(-0.0)],
                vec![StrictEq],
                ValueView::Bool(true),
            ),
            (
                vec![num(f64::NAN)],
                vec![Dup, StrictEq],
                ValueView::Bool(false),
            ),
            (
                vec![Primitive::Null, Primitive::Undefined],
                vec![StrictEq],
                ValueView::Bool(false),
            ),
            (vec![num(0.0)], vec![Not], ValueView::Bool(true)),
            (
                vec![Primitive::Null, Primitive::Bool(true)],
                vec![Pop, Not],
                ValueView::Bool(true),
            ),
        ];
        let rt = Runtime::new();
        for (consts, ops, expected) in cases {
            let mut asm = Assembler::new();
            for c in &consts {
                asm.constant(*c);
            }
            for op in &ops {
                asm.op(*op);
            }
            asm.op(Return);
            let exe = asm.finish("case", 0, 0).unwrap();
            let got = complete(rt.spawn(&exe, &[]).unwrap().run().unwrap());
            assert_eq!(got, expected, "{consts:?} {ops:?}");
        }
    }

    #[test]
    fn new_object_identity_and_strict_equality() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.op(Opcode::NewObject).set_local(0);
        // local1 = (local0 === local0)  -> true
        asm.get_local(0)
            .get_local(0)
            .op(Opcode::StrictEq)
            .set_local(1);
        // (local0 === {}) -> false; return that === !local1
        asm.get_local(0).op(Opcode::NewObject).op(Opcode::StrictEq);
        asm.get_local(1)
            .op(Opcode::Not)
            .op(Opcode::StrictEq)
            .op(Opcode::Return);
        let exe = asm.finish("identity", 0, 2).unwrap();
        let outcome = rt.spawn(&exe, &[]).unwrap().run().unwrap();
        assert_eq!(complete(outcome), ValueView::Bool(true));
        assert_eq!(rt.heap_stats().objects, 2);
    }

    #[test]
    fn runtime_failure_reports_diagnostic_context() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.constant(num(1.0)).op(Opcode::NewObject).op(Opcode::Add);
        asm.op(Opcode::Return);
        let exe = asm.finish("bad-add", 0, 0).unwrap();
        let task = rt.spawn(&exe, &[]).unwrap();
        let err = failed(task.run().unwrap());
        assert_eq!(err.kind(), VmErrorKind::UnsupportedOperand);
        assert!(
            err.message().contains("number and object"),
            "{}",
            err.message()
        );
        let d = err.diagnostic();
        assert_eq!(&*d.executable, "bad-add");
        assert_eq!(d.instruction, 2);
        assert_eq!(d.byte_offset, Some(4));
        assert_eq!(d.opcode, Some(Opcode::Add));
        assert_eq!((d.task, d.world), (task.id(), task.world()));
        assert_eq!(task.status(), TaskStatus::Finished);
        assert_eq!(task.run().unwrap_err(), RunError::Finished);
    }

    #[test]
    fn tasks_have_independent_state_over_shared_code() {
        let rt = Runtime::new();
        let exe = factorial();
        let (three, four) = (rt.root(num(3.0)), rt.root(num(4.0)));
        let a = rt.spawn(&exe, &[&three]).unwrap();
        let b = rt.spawn(&exe, &[&four]).unwrap();
        let c = rt.spawn(&exe, &[&three]).unwrap();
        assert!(Executable::same_code(a.executable(), b.executable()));
        assert_ne!(a.id(), b.id());
        // Run in a different order from spawn; each sees only its own locals.
        assert_eq!(complete(b.run().unwrap()), ValueView::Number(24.0));
        assert_eq!(c.status(), TaskStatus::Ready);
        assert_eq!(complete(a.run().unwrap()), ValueView::Number(6.0));
        assert_eq!(complete(c.run().unwrap()), ValueView::Number(6.0));
    }

    #[test]
    fn missing_arguments_are_undefined_and_extra_ones_ignored() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.get_local(1).op(Opcode::Return);
        let exe = asm.finish("second-param", 2, 2).unwrap();
        let one = rt.root(num(1.0));
        let t = rt.spawn(&exe, &[&one]).unwrap();
        assert_eq!(complete(t.run().unwrap()), ValueView::Undefined);
        let t = rt.spawn(&exe, &[&one, &one, &one]).unwrap();
        assert_eq!(complete(t.run().unwrap()), ValueView::Number(1.0));
    }

    #[test]
    fn overlapping_run_is_rejected_by_the_execution_guard() {
        let rt = Runtime::new();
        let task = rt.spawn(&factorial(), &[]).unwrap();
        let alias = task.clone();
        let guard = task.try_begin_run().unwrap();
        assert_eq!(alias.status(), TaskStatus::Running);
        assert_eq!(alias.try_begin_run().err(), Some(RunError::AlreadyRunning));
        assert_eq!(alias.run().unwrap_err(), RunError::AlreadyRunning);
        assert_eq!(guard.task_id(), task.id());
        // factorial(undefined): `0 < undefined` is an unsupported operand.
        assert_eq!(failed(guard.run()).kind(), VmErrorKind::UnsupportedOperand);
        assert_eq!(alias.try_begin_run().err(), Some(RunError::Finished));
    }

    #[test]
    fn dropping_unused_guard_keeps_task_runnable() {
        let rt = Runtime::new();
        let four = rt.root(num(4.0));
        let task = rt.spawn(&factorial(), &[&four]).unwrap();
        let guard = task.try_begin_run().unwrap();
        assert_eq!(task.try_begin_run().err(), Some(RunError::AlreadyRunning));
        drop(guard);
        assert_eq!(task.status(), TaskStatus::Ready);
        let guard = task.try_begin_run().unwrap();
        assert_eq!(complete(guard.run()), ValueView::Number(24.0));
    }

    #[test]
    fn task_frames_are_traceable_until_the_task_finishes() {
        let rt = Runtime::new();
        let obj = rt.scope(|s| {
            let o = s.new_object();
            s.persist(o)
        });
        let mut asm = Assembler::new();
        asm.get_local(0).op(Opcode::Return);
        let task = rt.spawn(&asm.finish("id", 1, 1).unwrap(), &[&obj]).unwrap();
        let mut seen = Vec::new();
        assert!(task.trace_frames(&mut |o| seen.push(o)));
        assert_eq!(seen.len(), 1);
        let guard = task.try_begin_run().unwrap();
        assert!(!task.trace_frames(&mut |_| {}));
        let result = guard.run();
        seen.clear();
        assert!(task.trace_frames(&mut |o| seen.push(o)));
        assert!(seen.is_empty());
        match result {
            RunOutcome::Complete(r) => assert!(r.same_object(&obj)),
            other => panic!("{other:?}"),
        }
    }

    /// Corrupted code that bypasses validation still fails cleanly.
    #[test]
    fn invariant_violations_fail_the_task_instead_of_panicking() {
        let programs: Vec<Vec<Instr>> = vec![
            vec![Instr::Pop],
            vec![Instr::Add],
            vec![Instr::Dup],
            vec![Instr::Return],
            vec![Instr::Const(9)],
            vec![Instr::GetLocal(9)],
            vec![Instr::Undefined, Instr::SetLocal(9)],
            vec![Instr::Jump(99)],
            vec![Instr::Undefined],
        ];
        let rt = Runtime::new();
        for instrs in programs {
            let code = Code {
                name: "corrupt".into(),
                params: 0,
                locals: 0,
                constants: Box::new([]),
                offsets: (0..instrs.len() as u32).collect(),
                instrs: instrs.clone().into(),
                max_stack: 0,
            };
            let exe = Executable(Arc::new(code));
            let err = failed(rt.spawn(&exe, &[]).unwrap().run().unwrap());
            assert_eq!(err.kind(), VmErrorKind::Internal, "{instrs:?}");
        }
    }

    #[test]
    fn malformed_bytecode_never_reaches_a_task() {
        let mut asm = Assembler::new();
        asm.raw(0xEE).op(Opcode::Return);
        let e = asm.finish("bad", 0, 0).unwrap_err();
        assert_eq!(e.kind, ValidationErrorKind::UnknownOpcode { byte: 0xEE });
        let mut asm = Assembler::new();
        let never_bound = asm.label();
        asm.jump(never_bound);
        let e = asm.finish("bad", 0, 0).unwrap_err();
        assert_eq!(
            e.kind,
            ValidationErrorKind::BadBranchTarget { target: u32::MAX }
        );
    }
}

/// Budgeted yield/resume and leaf host effects. Early VM tests: hand-assembled
/// bytecode, not JavaScript source support.
#[cfg(test)]
mod resume_tests {
    use super::*;
    use crate::host::HostCall;
    use crate::value::{Primitive, ValueView};
    use crate::vm::{Assembler, Opcode};
    use crate::{Runtime, TaskId};

    fn num(n: f64) -> Value {
        Value::Number(n)
    }

    fn complete(outcome: RunOutcome) -> ValueView {
        match outcome {
            RunOutcome::Complete(v) => v.view(),
            other => panic!("expected Complete, got {other:?}"),
        }
    }

    fn failed(outcome: RunOutcome) -> VmError {
        match outcome {
            RunOutcome::Failed(e) => e,
            other => panic!("expected Failed, got {other:?}"),
        }
    }

    fn yielded(outcome: RunOutcome) {
        assert!(matches!(outcome, RunOutcome::Yielded), "{outcome:?}");
    }

    /// Host whose effect increments a counter and returns its new value.
    #[derive(Default)]
    struct Counter {
        log: Vec<HostCall>,
    }

    impl Host for Counter {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.log.push(call);
            Ok(Primitive::Number(self.log.len() as f64))
        }
    }

    impl Counter {
        fn args_of(&self, task: TaskId) -> Vec<f64> {
            self.log
                .iter()
                .filter(|c| c.task == task)
                .map(|c| match c.arg {
                    Primitive::Number(n) => n,
                    other => panic!("{other:?}"),
                })
                .collect()
        }
    }

    const EFFECT_OP: u16 = 7;
    /// Instruction index of the loop head (`top`) in [`effect_loop`].
    const TOP: usize = 4;
    /// Instruction index of the loop exit (`done`) in [`effect_loop`].
    const DONE: usize = 16;

    /// `function f(n) { let i = 0, last; while (i < n) { last = effect(i); i = i + 1; } return last; }`
    ///
    /// Locals: 0 = n, 1 = i, 2 = last. Executes `4 + 12 n + 6` instructions.
    fn effect_loop() -> Executable {
        let mut asm = Assembler::new();
        let (top, done) = (asm.label(), asm.label());
        asm.constant(Primitive::Number(0.0)).set_local(1); // 0, 1
        asm.op(Opcode::Undefined).set_local(2); // 2, 3
        asm.bind(top); // 4
        asm.get_local(1).get_local(0).op(Opcode::Lt); // 4, 5, 6
        asm.jump_if_false(done); // 7
        asm.get_local(1).host_effect(EFFECT_OP).set_local(2); // 8, 9, 10
        asm.get_local(1)
            .constant(Primitive::Number(1.0))
            .op(Opcode::Add)
            .set_local(1); // 11..14
        asm.jump(top); // 15
        asm.bind(done); // 16
        asm.get_local(2).op(Opcode::Return); // 16, 17
        asm.finish("effect-loop", 1, 3).unwrap()
    }

    fn effect_loop_len(n: u64) -> u64 {
        4 + 12 * n + 6
    }

    #[test]
    fn yield_resumes_at_the_saved_instruction() {
        // (2 + 3) -> local 0; return local0 * 4
        let mut asm = Assembler::new();
        asm.constant(Primitive::Number(2.0))
            .constant(Primitive::Number(3.0))
            .op(Opcode::Add)
            .set_local(0);
        asm.get_local(0)
            .constant(Primitive::Number(4.0))
            .op(Opcode::Mul)
            .op(Opcode::Return);
        let exe = asm.finish("straight", 0, 1).unwrap();
        let rt = Runtime::new();
        let task = rt.spawn(&exe, &[]).unwrap();
        let mut host = Counter::default();

        yielded(task.run_with(Budget::instructions(2), &mut host).unwrap());
        assert_eq!(task.status(), TaskStatus::Ready);
        let (pc, operands, locals) = task.saved_state().unwrap();
        assert_eq!(pc, 2);
        assert_eq!(operands, vec![num(2.0), num(3.0)]);
        assert_eq!(locals, vec![Value::Undefined]);

        yielded(task.run_with(Budget::instructions(3), &mut host).unwrap());
        let (pc, operands, locals) = task.saved_state().unwrap();
        assert_eq!(pc, 5);
        assert_eq!(operands, vec![num(5.0)]);
        assert_eq!(locals, vec![num(5.0)]);

        let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(20.0));
        assert_eq!(task.status(), TaskStatus::Finished);
        assert!(task.saved_state().is_none());
    }

    #[test]
    fn zero_budget_yields_without_progress() {
        let rt = Runtime::new();
        let n = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&n]).unwrap();
        let mut host = Counter::default();
        for _ in 0..3 {
            yielded(task.run_with(Budget::instructions(0), &mut host).unwrap());
            assert_eq!(task.saved_state().unwrap().0, 0);
        }
        assert!(host.log.is_empty());
        let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(1.0));
    }

    /// Every possible yield point: for each budget, the run sequence executes
    /// each instruction exactly once (the run count is exact) and each effect
    /// exactly once, in program order.
    #[test]
    fn host_effects_run_once_across_yields() {
        let n = 4;
        let total = effect_loop_len(n);
        let rt = Runtime::new();
        let exe = effect_loop();
        let arg = rt.root(Primitive::Number(n as f64));
        for b in 1..=total + 1 {
            let task = rt.spawn(&exe, &[&arg]).unwrap();
            let mut host = Counter::default();
            let mut runs = 0;
            let result = loop {
                runs += 1;
                match task.run_with(Budget::instructions(b), &mut host).unwrap() {
                    RunOutcome::Yielded => continue,
                    other => break complete(other),
                }
            };
            assert_eq!(runs, total.div_ceil(b), "budget {b}");
            assert_eq!(
                host.args_of(task.id()),
                vec![0.0, 1.0, 2.0, 3.0],
                "budget {b}"
            );
            assert!(host.log.iter().all(|c| c.op == EFFECT_OP));
            // The last effect returned the counter value 4.
            assert_eq!(result, ValueView::Number(4.0), "budget {b}");
        }
    }

    #[test]
    fn budget_exhaustion_at_branch_targets() {
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let mut host = Counter::default();

        // Setup plus one iteration ends with the backward `jump top`.
        yielded(task.run_with(Budget::instructions(16), &mut host).unwrap());
        let (pc, operands, locals) = task.saved_state().unwrap();
        assert_eq!(pc, TOP, "stopped at the backward branch target");
        assert!(operands.is_empty());
        assert_eq!(locals, vec![num(1.0), num(1.0), num(1.0)]);
        assert_eq!(host.log.len(), 1);

        // `1 < 1` is false: the taken `jump_if_false` lands on `done`.
        yielded(task.run_with(Budget::instructions(4), &mut host).unwrap());
        let (pc, operands, _) = task.saved_state().unwrap();
        assert_eq!(pc, DONE, "stopped at the forward branch target");
        assert!(operands.is_empty());

        let outcome = task.run_with(Budget::instructions(2), &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(1.0));
        assert_eq!(host.log.len(), 1, "no effect replayed after branch yields");
    }

    #[test]
    fn independent_tasks_resume_in_interleaved_order() {
        let rt = Runtime::new();
        let exe = effect_loop();
        let sizes = [2.0, 3.0, 5.0];
        let roots: Vec<Rooted> = sizes
            .iter()
            .map(|&n| rt.root(Primitive::Number(n)))
            .collect();
        let tasks: Vec<Task> = roots
            .iter()
            .map(|r| rt.spawn(&exe, &[r]).unwrap())
            .collect();
        let mut host = Counter::default();
        let mut results: Vec<Option<ValueView>> = vec![None; tasks.len()];
        let mut round = 0;
        while results.iter().any(Option::is_none) {
            // Rotate and alternate direction so the run order differs per round.
            let mut order: Vec<usize> = (0..tasks.len())
                .map(|i| (i + round) % tasks.len())
                .collect();
            if round % 2 == 1 {
                order.reverse();
            }
            for i in order {
                if results[i].is_some() {
                    continue;
                }
                match tasks[i]
                    .run_with(Budget::instructions(7), &mut host)
                    .unwrap()
                {
                    RunOutcome::Yielded => {}
                    other => results[i] = Some(complete(other)),
                }
            }
            round += 1;
            assert!(round < 100, "tasks failed to finish");
        }
        for (task, &n) in tasks.iter().zip(&sizes) {
            let expected: Vec<f64> = (0..n as usize).map(|i| i as f64).collect();
            assert_eq!(host.args_of(task.id()), expected);
        }
        // Effects really interleaved: the log does not run task by task.
        let switches = host
            .log
            .windows(2)
            .filter(|w| w[0].task != w[1].task)
            .count();
        assert!(switches > tasks.len(), "{switches} switches");
        assert_eq!(host.log.len(), 10);
        // Each task's result is the shared counter value at its last effect.
        for (i, task) in tasks.iter().enumerate() {
            let last = host.log.iter().rposition(|c| c.task == task.id()).unwrap();
            assert_eq!(results[i], Some(ValueView::Number((last + 1) as f64)));
        }
    }

    #[test]
    fn yielded_state_stays_traceable_and_owned_by_the_task() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.op(Opcode::NewObject).set_local(0);
        asm.get_local(0).op(Opcode::Return);
        let task = rt.spawn(&asm.finish("hold", 0, 1).unwrap(), &[]).unwrap();
        yielded(
            task.run_with(Budget::instructions(2), &mut Counter::default())
                .unwrap(),
        );
        let mut seen = Vec::new();
        assert!(task.trace_frames(&mut |o| seen.push(o)));
        assert_eq!(
            seen.len(),
            1,
            "the object is reachable from the saved locals"
        );
        let r = match task.run().unwrap() {
            RunOutcome::Complete(r) => r,
            other => panic!("{other:?}"),
        };
        assert_eq!(r.view(), ValueView::Object);
        assert_eq!(rt.heap_stats().objects, 1);
    }

    #[test]
    fn host_errors_and_missing_host_fail_the_task() {
        struct Refuse;
        impl Host for Refuse {
            fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
                Err(format!("refused op {}", call.op))
            }
        }
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let err = failed(task.run_with(Budget::UNLIMITED, &mut Refuse).unwrap());
        assert_eq!(err.kind(), VmErrorKind::HostEffect);
        assert_eq!(err.message(), "refused op 7");
        assert_eq!(err.diagnostic().instruction, 9);
        assert_eq!(err.diagnostic().opcode, Some(Opcode::HostEffect));
        assert_eq!(task.status(), TaskStatus::Finished);

        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let err = failed(task.run().unwrap());
        assert_eq!(err.kind(), VmErrorKind::HostEffect);
        assert!(err.message().contains("no host"), "{}", err.message());
    }

    #[test]
    fn panicking_host_clears_the_effect_guard_and_poisons_only_its_task() {
        struct Panic;
        impl Host for Panic {
            fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
                panic!("host bug")
            }
        }
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let task = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let run = std::panic::catch_unwind(|| task.run_with(Budget::UNLIMITED, &mut Panic));
        assert!(run.is_err());
        assert!(!crate::host::inside_effect(), "flag cleared on unwind");
        assert_eq!(task.run().unwrap_err(), RunError::Poisoned);
        // The same thread can run other tasks and open heap scopes again.
        let other = rt.spawn(&effect_loop(), &[&one]).unwrap();
        let mut host = Counter::default();
        let outcome = other.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Number(1.0));
        rt.scope(|_| ());
    }

    #[test]
    fn host_effect_rejects_object_arguments_before_calling_the_host() {
        let rt = Runtime::new();
        let mut asm = Assembler::new();
        asm.op(Opcode::NewObject).host_effect(0).op(Opcode::Return);
        let task = rt
            .spawn(&asm.finish("obj-effect", 0, 0).unwrap(), &[])
            .unwrap();
        let mut host = Counter::default();
        let err = failed(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
        assert_eq!(err.kind(), VmErrorKind::UnsupportedOperand);
        assert!(host.log.is_empty());
    }

    #[test]
    fn host_effects_cannot_reenter_the_vm_or_open_heap_scopes() {
        struct Reenter<'t> {
            rt: Runtime,
            own: Task,
            other: Task,
            other_guard: Option<RunGuard<'t>>,
            seen: Vec<String>,
        }
        impl Host for Reenter<'_> {
            fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
                self.seen.push(format!("{:?}", self.own.run().err()));
                self.seen.push(format!("{:?}", self.other.run().err()));
                let guard = self.other_guard.take().unwrap();
                self.seen.push(format!("{:?}", failed(guard.run()).kind()));
                let rt = self.rt.clone();
                let scope = std::panic::catch_unwind(move || rt.scope(|_| ()));
                self.seen.push(format!("scope ok: {}", scope.is_ok()));
                Ok(Primitive::Null)
            }
        }
        let rt = Runtime::new();
        let one = rt.root(Primitive::Number(1.0));
        let exe = effect_loop();
        let own = rt.spawn(&exe, &[&one]).unwrap();
        let other = rt.spawn(&exe, &[&one]).unwrap();
        let third = rt.spawn(&exe, &[&one]).unwrap();
        let mut host = Reenter {
            rt: rt.clone(),
            own: own.clone(),
            other: other.clone(),
            other_guard: Some(third.try_begin_run().unwrap()),
            seen: Vec::new(),
        };
        let outcome = own.run_with(Budget::UNLIMITED, &mut host).unwrap();
        assert_eq!(complete(outcome), ValueView::Null);
        assert_eq!(
            host.seen,
            [
                "Some(InsideHostEffect)",
                "Some(InsideHostEffect)",
                "HostEffect",
                "scope ok: false",
            ]
        );
        // The untouched task is still runnable once no effect is executing;
        // the one entered through a pre-acquired guard was failed.
        drop(host);
        assert_eq!(other.status(), TaskStatus::Ready);
        assert_eq!(third.status(), TaskStatus::Finished);
        rt.scope(|_| ());
    }
}
