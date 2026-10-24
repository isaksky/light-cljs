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

use std::fmt;
use std::sync::{Arc, Mutex, MutexGuard, TryLockError};

use crate::error::VmError;
use crate::heap::gc::Trace;
use crate::heap::{Heap, ObjectId, Rooted};
use crate::ids::{TaskId, WaitKey, WorldId};
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
    /// Reserved: the task stopped on its execution budget and can be run
    /// again. Not produced by this slice (pellet clj-quickjs-16).
    Yielded,
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
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            RunError::AlreadyRunning => "task is already running",
            RunError::Finished => "task has finished",
            RunError::Poisoned => "task state is poisoned by an earlier panic",
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

    /// Acquire the guard and run on the calling thread.
    pub fn run(&self) -> Result<RunOutcome, RunError> {
        Ok(self.try_begin_run()?.run())
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
    pub fn run(mut self) -> RunOutcome {
        let TaskBody::Ready(vm) = &mut *self.body else {
            // `try_begin_run` only hands out guards for ready tasks, and only
            // `run` (which consumes the guard) changes the body.
            return RunOutcome::Failed(unreachable_finished(self.task));
        };
        let outcome = match execute(vm, &self.task.executable.0, &self.task.heap) {
            Exit::Return(value) => RunOutcome::Complete(Rooted::new(&self.task.heap, value)),
            Exit::Fail(error) => RunOutcome::Failed(error),
        };
        // Completion or failure releases the task's frames; a returned value
        // stays alive through its persistent root, not through the task.
        *self.body = TaskBody::Finished;
        outcome
    }
}

#[cold]
fn unreachable_finished(task: &TaskInner) -> VmError {
    VmError {
        kind: crate::error::VmErrorKind::Internal,
        message: "run guard held for a finished task".to_string(),
        diagnostic: crate::error::Diagnostic {
            executable: task.executable.0.name.clone(),
            task: task.id,
            world: task.world,
            instruction: 0,
            byte_offset: None,
            opcode: None,
        },
    }
}

/// Early VM tests: hand-assembled bytecode, not JavaScript source support.
#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::VmErrorKind;
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
