//! Embedding-API tests for the VM core.
//!
//! Early VM tests: programs are hand-assembled bytecode, not JavaScript
//! source support. The test plays the embedder: it creates tasks and drives
//! them on its own threads; the runtime owns no threads.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use clj_quickjs::{
    Assembler, Executable, ExecutableSpec, HandleError, Opcode, Primitive, Rooted, RunError,
    RunOutcome, Runtime, TaskStatus, ValidationErrorKind, ValueView,
};

/// Bound for every cross-thread handshake: a broken handshake fails the test
/// instead of hanging it.
const HANDSHAKE: Duration = Duration::from_secs(10);

fn identity() -> Executable {
    let mut asm = Assembler::new();
    asm.get_local(0).op(Opcode::Return);
    asm.finish("identity", 1, 1).unwrap()
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn value_handle_keeps_stable_identity_across_runs() {
    let rt = Runtime::new();
    let exe = identity();
    let input = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });

    let first = complete(rt.spawn(&exe, &[&input]).unwrap().run().unwrap());
    let second = complete(rt.spawn(&exe, &[&input]).unwrap().run().unwrap());
    assert!(first.same_object(&input));
    assert!(second.same_object(&input));
    assert!(first.same_object(&second));
    assert_eq!(first.strict_equals(&second), Ok(true));

    // An object allocated by one task, returned, and passed to a later task
    // comes back as the same object, not an equal copy.
    let mut asm = Assembler::new();
    asm.op(Opcode::NewObject).op(Opcode::Return);
    let fresh = complete(
        rt.spawn(&asm.finish("fresh", 0, 0).unwrap(), &[])
            .unwrap()
            .run()
            .unwrap(),
    );
    assert!(!fresh.same_object(&input));
    let echoed = complete(rt.spawn(&exe, &[&fresh]).unwrap().run().unwrap());
    assert!(echoed.same_object(&fresh));

    // Inside a scope the same identity is visible through local handles.
    rt.scope(|s| {
        let a = s.local(&input).unwrap();
        let b = s.local(&second).unwrap();
        let c = s.local(&fresh).unwrap();
        assert!(s.same_object(a, b));
        assert!(!s.same_object(a, c));
        assert_eq!(s.view(a), ValueView::Object);
    });
}

#[test]
fn results_are_persistent_roots_that_outlive_their_task() {
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.op(Opcode::NewObject).op(Opcode::Return);
    let exe = asm.finish("alloc", 0, 0).unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let result = complete(task.run().unwrap());
    drop(task);
    assert_eq!(rt.heap_stats().persistent_roots, 1);
    assert_eq!(result.view(), ValueView::Object);
    let copy = result.clone();
    assert!(copy.same_object(&result));
    assert_eq!(rt.heap_stats().persistent_roots, 2);
    drop((result, copy));
    assert_eq!(rt.heap_stats().persistent_roots, 0);
}

#[test]
fn handles_from_another_runtime_are_rejected() {
    let rt = Runtime::new();
    let other = Runtime::new();
    let foreign = other.root(Primitive::Number(1.0));
    assert_eq!(
        rt.spawn(&identity(), &[&foreign]).unwrap_err(),
        HandleError::ForeignRuntime
    );
    rt.scope(|s| assert_eq!(s.local(&foreign).unwrap_err(), HandleError::ForeignRuntime));
}

#[test]
fn invalid_bytecode_is_rejected_at_load_time() {
    let spec = |code: Vec<u8>| ExecutableSpec {
        name: "bad".into(),
        params: 0,
        locals: 1,
        code,
        constants: vec![Primitive::Null],
    };
    let cases = [
        (
            vec![0xFF],
            ValidationErrorKind::UnknownOpcode { byte: 0xFF },
        ),
        (
            vec![Opcode::GetLocal as u8, 0],
            ValidationErrorKind::TruncatedOperand {
                opcode: Opcode::GetLocal,
            },
        ),
        (
            vec![Opcode::GetLocal as u8, 5, 0, Opcode::Return as u8],
            ValidationErrorKind::LocalOutOfRange { index: 5, count: 1 },
        ),
        (
            vec![Opcode::Const as u8, 1, 0, Opcode::Return as u8],
            ValidationErrorKind::ConstantOutOfRange { index: 1, count: 1 },
        ),
        (
            vec![Opcode::Add as u8, Opcode::Return as u8],
            ValidationErrorKind::StackUnderflow {
                opcode: Opcode::Add,
                depth: 0,
            },
        ),
        (
            vec![Opcode::NewObject as u8],
            ValidationErrorKind::FallsOffEnd,
        ),
    ];
    for (code, expected) in cases {
        let err = Executable::load(spec(code.clone())).unwrap_err();
        assert_eq!(err.kind, expected, "{code:?}");
    }
}

/// The embedder holds a task's execution guard on one of its threads; a
/// second thread's attempt to run the same task is rejected, not queued.
#[test]
fn overlapping_run_from_another_thread_is_rejected() {
    let rt = Runtime::new();
    let n = rt.root(Primitive::Number(41.0));
    let mut asm = Assembler::new();
    asm.get_local(0)
        .constant(Primitive::Number(1.0))
        .op(Opcode::Add)
        .op(Opcode::Return);
    let task = rt.spawn(&asm.finish("inc", 1, 1).unwrap(), &[&n]).unwrap();

    let (held_tx, held_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let worker_task = task.clone();
    let worker = thread::spawn(move || {
        let guard = worker_task
            .try_begin_run()
            .expect("first run acquires the guard");
        held_tx.send(()).unwrap();
        // Proceeds on the signal, or when the other side drops its sender.
        let _ = go_rx.recv_timeout(HANDSHAKE);
        guard.run()
    });

    held_rx
        .recv_timeout(HANDSHAKE)
        .expect("worker acquired the guard");
    let contender = task.clone();
    let rejected = thread::spawn(move || contender.run().map(|_| ()))
        .join()
        .unwrap();
    assert_eq!(rejected, Err(RunError::AlreadyRunning));
    assert_eq!(task.status(), TaskStatus::Running);
    go_tx.send(()).unwrap();

    let result = complete(worker.join().unwrap());
    assert_eq!(result.view(), ValueView::Number(42.0));
    assert_eq!(task.run().unwrap_err(), RunError::Finished);
}

/// Many tasks over one shared executable, driven on embedder-owned threads.
#[test]
fn embedder_drives_tasks_on_its_own_threads() {
    let rt = Runtime::new();
    // function f(n) { let acc = 0; while (0 < n) { acc = acc + n; n = n - 1; } return acc; }
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.constant(Primitive::Number(0.0)).set_local(1);
    asm.bind(top);
    asm.constant(Primitive::Number(0.0))
        .get_local(0)
        .op(Opcode::Lt)
        .jump_if_false(done);
    asm.get_local(1).get_local(0).op(Opcode::Add).set_local(1);
    asm.get_local(0)
        .constant(Primitive::Number(1.0))
        .op(Opcode::Sub)
        .set_local(0);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(1).op(Opcode::Return);
    let exe = asm.finish("triangle", 1, 2).unwrap();

    let tasks: Vec<_> = (0..8u32)
        .map(|i| {
            let n = rt.root(Primitive::Number(f64::from(100 * i)));
            rt.spawn(&exe, &[&n]).unwrap()
        })
        .collect();
    let results: Vec<f64> = thread::scope(|s| {
        let handles: Vec<_> = tasks
            .chunks(2)
            .map(|pair| {
                s.spawn(move || {
                    pair.iter()
                        .map(|t| match complete(t.run().unwrap()).view() {
                            ValueView::Number(x) => x,
                            other => panic!("{other:?}"),
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        handles
            .into_iter()
            .flat_map(|h| h.join().unwrap())
            .collect()
    });
    for (i, got) in results.into_iter().enumerate() {
        let n = 100.0 * i as f64;
        assert_eq!(got, n * (n + 1.0) / 2.0);
    }
    assert!(
        tasks
            .iter()
            .all(|t| Executable::same_code(t.executable(), &exe))
    );
}
