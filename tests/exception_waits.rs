//! Exception and finally state across dependency waits (pellet
//! clj-quickjs-27, `docs/design.md` §10: "A dependency wait is not an
//! exception. It must not prematurely run finally blocks").
//!
//! Programs are inline JavaScript source snippets (synthetic fixtures, not
//! light-cljs output). The tests play a manually driven host: they run a
//! task until it waits, take the producer request its `need` claimed,
//! publish a result, failure or cancellation, and run the task the
//! completion made runnable, sometimes on another thread and with a
//! collection in between. Effects are observed through a recording host:
//! `__rt.effect(1, x)` logs `x` and returns it, so every log entry is one
//! execution of that effect (a replay would log it twice).

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clj_quickjs::exec::{
    EnqueueReason, Event, EventKind, Executor, ExecutorConfig, OutcomeKind, Placement, WorkerId,
};
use clj_quickjs::frontend::{compile_function, compile_script};
use clj_quickjs::{
    Budget, Cancel, CellStatus, Completion, DependencyId, DependencyKey, Host, HostCall, Primitive,
    ResultKind, Rooted, RunOutcome, Runtime, Task, TaskId, TaskStatus, ValueView, VmError,
    VmErrorKind, Wake, Wakeups,
};

/// `__rt.need(5, 3)`.
const A: (DependencyId, ResultKind) = (DependencyId::new(5), ResultKind::new(3));
/// `__rt.need(6, 3)`.
const B: (DependencyId, ResultKind) = (DependencyId::new(6), ResultKind::new(3));

fn key(rt: &Runtime, (def, kind): (DependencyId, ResultKind)) -> DependencyKey {
    DependencyKey::new(rt.world(), def, kind)
}

/// Host operation 1 logs its numeric argument and returns it.
#[derive(Default)]
struct Log(Vec<f64>);

impl Host for Log {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, call.arg) {
            (1, Primitive::Number(n)) => {
                self.0.push(n);
                Ok(call.arg)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

fn spawn(rt: &Runtime, src: &str) -> Task {
    let exe = compile_script("test", src).unwrap_or_else(|e| panic!("{src:?}: {e}"));
    rt.spawn(&exe, &[]).unwrap()
}

fn run(task: &Task, log: &mut Log) -> RunOutcome {
    task.run_with(Budget::UNLIMITED, log).unwrap()
}

/// Run on a new thread (the task migrates; the log goes with it).
fn run_elsewhere(task: Task, log: Log) -> (RunOutcome, Log) {
    thread::spawn(move || {
        let mut log = log;
        let outcome = run(&task, &mut log);
        (outcome, log)
    })
    .join()
    .unwrap()
}

fn waiting(outcome: RunOutcome) -> DependencyKey {
    match outcome {
        RunOutcome::Waiting(key) => key,
        other => panic!("expected Waiting, got {other:?}"),
    }
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn number(outcome: RunOutcome) -> f64 {
    match complete(outcome).view() {
        ValueView::Number(n) => n,
        other => panic!("expected a number, got {other:?}"),
    }
}

fn failed(outcome: RunOutcome) -> VmError {
    match outcome {
        RunOutcome::Failed(e) => e,
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// The single task a completion made runnable.
fn only_runnable(wakeups: Wakeups) -> Task {
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    runnable.pop().unwrap()
}

/// Publish `n` as the result the task's single claimed request is for.
fn publish(rt: &Runtime, task: &Task, n: f64) -> Task {
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.pop().unwrap();
    let value = rt.root(Primitive::Number(n));
    only_runnable(
        rt.complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap(),
    )
}

/// Fail the result the task's single claimed request is for with `error`.
fn fail(rt: &Runtime, task: &Task, error: &Rooted) -> Task {
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.pop().unwrap();
    only_runnable(
        rt.complete(request.key(), request.producer(), Completion::Failed(error))
            .unwrap(),
    )
}

/// Run until the task waits on `expected`; nothing else may happen.
fn run_to_wait(rt: &Runtime, task: &Task, log: &mut Log, expected: (DependencyId, ResultKind)) {
    assert_eq!(waiting(run(task, log)), key(rt, expected));
    assert_eq!(task.status(), TaskStatus::Waiting);
}

/// Source line and column of a failure.
fn line_column(e: &VmError) -> (u32, u32) {
    let at = e.diagnostic().source.expect("source location");
    (at.line, at.column)
}

/// A wait in a protected block, a catch block, and a finally block: parking
/// runs no handler, the finally block runs once after the resumed code, and
/// nothing before the wait is replayed.
#[test]
fn waits_in_try_catch_and_finally_run_no_handler_while_parked() {
    let cases: [(&str, &[f64], &[f64], f64); 3] = [
        (
            "let r = 0;
             try { __rt.effect(1, 1); r = __rt.need(5, 3); __rt.effect(1, 2); }
             catch (e) { __rt.effect(1, 8); }
             finally { __rt.effect(1, 9); }
             r",
            &[1.0],
            &[1.0, 2.0, 9.0],
            7.0,
        ),
        (
            "let r = 0;
             try { __rt.effect(1, 1); throw 10; }
             catch (e) { __rt.effect(1, e); r = e + __rt.need(5, 3); __rt.effect(1, 2); }
             finally { __rt.effect(1, 9); }
             r",
            &[1.0, 10.0],
            &[1.0, 10.0, 2.0, 9.0],
            17.0,
        ),
        (
            "let r = 0;
             try { __rt.effect(1, 1); }
             finally { __rt.effect(1, 9); r = __rt.need(5, 3); __rt.effect(1, 2); }
             r",
            &[1.0, 9.0],
            &[1.0, 9.0, 2.0],
            7.0,
        ),
    ];
    for (src, before, after, result) in cases {
        let rt = Runtime::new();
        let task = spawn(&rt, src);
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        assert_eq!(log.0, before, "{src}");
        // Parked: a collection and a refused run change nothing.
        rt.collect().unwrap();
        assert!(task.run().is_err());
        assert_eq!(log.0, before, "{src}");
        let resumed = publish(&rt, &task, 7.0);
        let (outcome, log) = run_elsewhere(resumed, log);
        assert_eq!(number(outcome), result, "{src}");
        assert_eq!(log.0, after, "{src}");
    }
}

/// A wait inside a function called from a protected block, with a finally
/// block in the callee and one in the caller: both run once, innermost
/// first, after the resumed callee code, and only then does the caller
/// continue.
#[test]
fn a_wait_in_a_callee_of_a_protected_block_keeps_every_handler() {
    let src = "
        function inner(x) {
          try { __rt.effect(1, x); return x + __rt.need(5, 3); }
          finally { __rt.effect(1, 20); }
        }
        let r = 0;
        try { __rt.effect(1, 1); r = inner(2); __rt.effect(1, r); }
        catch (e) { __rt.effect(1, 99); }
        finally { __rt.effect(1, 30); }
        __rt.effect(1, 40);
        r";
    let rt = Runtime::new();
    let task = spawn(&rt, src);
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0, 2.0]);
    rt.collect().unwrap();
    let resumed = publish(&rt, &task, 7.0);
    let (outcome, log) = run_elsewhere(resumed, log);
    assert_eq!(number(outcome), 9.0);
    assert_eq!(log.0, [1.0, 2.0, 20.0, 9.0, 30.0, 40.0]);
}

/// A finally block that waits while a `return` is pending completes that
/// return after the wait; a pending exception is re-raised after the wait
/// and caught outside. The finally block's code before the wait ran once.
#[test]
fn a_wait_inside_finally_then_completes_the_pending_completion() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "function f() {
           try { __rt.effect(1, 1); return __rt.effect(1, 2); __rt.effect(1, 99); }
           finally { __rt.effect(1, 3); __rt.effect(1, __rt.need(5, 3)); }
           return 98;
         }
         f() * 10",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0, 2.0, 3.0]);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
    assert_eq!(number(outcome), 20.0, "the pending return completed");
    assert_eq!(log.0, [1.0, 2.0, 3.0, 7.0]);

    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let r = 0;
         try {
           try { __rt.effect(1, 1); throw 5; }
           finally { __rt.effect(1, __rt.need(5, 3)); }
           __rt.effect(1, 99);
         } catch (e) { r = __rt.effect(1, e); }
         r",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0]);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
    assert_eq!(number(outcome), 5.0, "the pending exception was re-raised");
    assert_eq!(log.0, [1.0, 7.0, 5.0]);
}

/// Two waits in a row inside one finally block, with a pending return of a
/// value no other root holds: a collection at each wait keeps it, and the
/// task returns that same object.
#[test]
fn a_pending_return_value_is_rooted_across_waits() {
    let rt = Runtime::new();
    let arg = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let weak = arg.downgrade();
    // After `return o` saved the object, the finally block clears `o`: the
    // pending completion is its only holder in the task.
    let exe = compile_function(
        "function keep(o) {
           try { return o; }
           finally { o = null; __rt.need(5, 3); __rt.need(6, 3); }
         }",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[&arg]).unwrap();
    drop(arg);
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    assert!(weak.is_live(), "held only by the pending return completion");
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 1.0), log);
    assert_eq!(waiting(outcome), key(&rt, B));
    rt.collect().unwrap();
    assert!(weak.is_live());
    let (outcome, _) = run_elsewhere(publish(&rt, &task, 2.0), log);
    let result = complete(outcome);
    assert!(result.same_object(&weak.upgrade().unwrap()));
}

/// A producer failure is a catchable error at the need, whether it arrives
/// while the task waits or was published before: the catch block gets an
/// error object naming the key, the need's source position and call chain,
/// with the producer's published error value as its cause (same object).
/// Finally blocks between the need and the catch run once, in order.
#[test]
fn a_producer_failure_is_caught_at_the_need() {
    let src = "
        function inner() {
          try { __rt.effect(1, 1); return __rt.need(5, 3); }
          finally { __rt.effect(1, 2); }
        }
        let caught;
        try { inner(); __rt.effect(1, 99); }
        catch (e) { caught = e; __rt.effect(1, 3); }
        finally { __rt.effect(1, 4); }
        caught";
    let rt = Runtime::new();
    // A shareable error value: a persistent vector made by another task.
    let cause = complete(spawn(&rt, "__rt.vector(13)").run().unwrap());
    let task = spawn(&rt, src);
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0]);
    let resumed = fail(&rt, &task, &cause);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(resumed, log);
    assert_eq!(log.0, [1.0, 2.0, 3.0, 4.0]);
    let caught = complete(outcome);
    let error = caught.error().expect("an error object");
    assert_eq!(error.kind(), VmErrorKind::DependencyFailed(key(&rt, A)));
    assert_eq!(error.diagnostic().world, rt.world());
    assert_eq!(line_column(&error), (3, 43), "{error}");
    assert_eq!(error.diagnostic().call_chain.len(), 1, "{error}");
    let caller = error.diagnostic().call_chain[0].source.unwrap();
    assert_eq!((caller.line, caller.column), (7, 15), "{error}");
    assert!(caught.error_cause().unwrap().same_object(&cause));

    // Already failed before the need: caught in the same run, no wait.
    let rt = Runtime::new();
    let request = rt.claim(key(&rt, A)).unwrap();
    let error_value = rt.root(Primitive::Number(13.0));
    let _ = rt
        .complete(
            request.key(),
            request.producer(),
            Completion::Failed(&error_value),
        )
        .unwrap();
    let task = spawn(&rt, src);
    let mut log = Log::default();
    let caught = complete(run(&task, &mut log));
    assert_eq!(log.0, [1.0, 2.0, 3.0, 4.0]);
    assert_eq!(
        caught.error().unwrap().kind(),
        VmErrorKind::DependencyFailed(key(&rt, A))
    );
    assert_eq!(
        caught.error_cause().unwrap().view(),
        ValueView::Number(13.0)
    );
    assert_eq!(task.dependency_log().edges(), &[key(&rt, A)]);
}

/// The error object raised for a failed dependency is pending in a finally
/// block that waits on another dependency: it is held only by the pending
/// completion while the task is parked, collected around and migrated, and
/// the catch block then gets the same error, with its key, source position,
/// call chain and cause intact. Every effect runs once.
#[test]
fn a_dependency_error_survives_a_wait_in_finally() {
    let src = "
        function inner() {
          try { return __rt.need(5, 3); }
          finally { __rt.effect(1, 1); __rt.effect(1, __rt.need(6, 3)); }
        }
        let caught;
        try { inner(); __rt.effect(1, 91); }
        catch (e) { caught = e; __rt.effect(1, 3); }
        caught";
    let rt = Runtime::new();
    let task = spawn(&rt, src);
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    let cause = complete(spawn(&rt, "__rt.vector(13)").run().unwrap());
    let weak_cause = cause.downgrade();
    let resumed = fail(&rt, &task, &cause);
    drop(cause);
    let (outcome, log) = run_elsewhere(resumed, log);
    assert_eq!(waiting(outcome), key(&rt, B));
    assert_eq!(log.0, [1.0]);
    rt.collect().unwrap();
    assert!(weak_cause.is_live());
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 2.0), log);
    assert_eq!(log.0, [1.0, 2.0, 3.0]);
    let caught = complete(outcome);
    let error = caught.error().expect("the dependency error object");
    assert_eq!(error.kind(), VmErrorKind::DependencyFailed(key(&rt, A)));
    assert_eq!(line_column(&error), (3, 24), "{error}");
    assert_eq!(error.diagnostic().call_chain.len(), 1, "{error}");
    let cause = caught.error_cause().unwrap();
    assert!(cause.same_object(&weak_cause.upgrade().unwrap()));
}

/// A cancelled cell is a catchable error at the need too, with an
/// `undefined` cause; the consumer itself is not cancelled and carries on.
#[test]
fn a_cancelled_cell_is_caught_at_the_need() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let r = 0;
         try { r = __rt.need(5, 3); } catch (e) { r = e; }
         __rt.effect(1, 1);
         r",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    let _ = task.take_producer_requests();
    let resumed = only_runnable(rt.cancel_cell(key(&rt, A)).unwrap());
    let caught = complete(run(&resumed, &mut log));
    assert_eq!(log.0, [1.0]);
    assert_eq!(task.status(), TaskStatus::Finished);
    assert_eq!(
        caught.error().unwrap().kind(),
        VmErrorKind::DependencyCancelled(key(&rt, A))
    );
    assert_eq!(caught.error_cause().unwrap().view(), ValueView::Undefined);
}

/// Unhandled dependency failures report the dependency (key and world) and
/// the need's source position and call chain: with no handler at all, after
/// finally blocks ran, and after a catch block rethrew the error object.
#[test]
fn an_unhandled_dependency_failure_reports_key_world_and_source() {
    let cases = [
        // No handler: the run fails at the need.
        "function inner() { return __rt.need(5, 3); }
         inner()",
        // Only finally blocks: they run, then the failure stands.
        "function inner() { try { return __rt.need(5, 3); } finally { __rt.effect(1, 2); } }
         try { inner(); } finally { __rt.effect(1, 3); }",
        // Caught and rethrown from an outer function's catch block.
        "function inner() { return __rt.need(5, 3); }
         function outer() { try { return inner(); } catch (e) { __rt.effect(1, 4); throw e; } }
         outer()",
    ];
    let expected_logs: [&[f64]; 3] = [&[], &[2.0, 3.0], &[4.0]];
    for (src, expected_log) in cases.into_iter().zip(expected_logs) {
        let rt = Runtime::new();
        let world = rt.new_world();
        let exe = compile_script("test", src).unwrap();
        let task = rt.spawn_in(world, &exe, &[]).unwrap();
        let mut log = Log::default();
        let expected_key = DependencyKey::new(world, A.0, A.1);
        assert_eq!(waiting(run(&task, &mut log)), expected_key);
        let error_value = rt.root(Primitive::Number(13.0));
        let resumed = fail(&rt, &task, &error_value);
        let error = failed(run(&resumed, &mut log));
        assert_eq!(log.0, expected_log, "{src}");
        assert_eq!(error.kind(), VmErrorKind::DependencyFailed(expected_key));
        assert_eq!(error.diagnostic().world, world);
        assert_eq!(error.diagnostic().task, task.id());
        assert_eq!(line_column(&error).0, 1, "{error}");
        assert!(!error.diagnostic().call_chain.is_empty(), "{error}");
        let shown = error.to_string();
        assert!(shown.contains(&expected_key.to_string()), "{shown}");
        assert!(shown.contains("13"), "{shown}");
    }
}

/// A dependency failure inside a resumable native (`__rt.needAll`) unwinds
/// through the native's frame into the enclosing catch block
/// (clj-quickjs-28, `crate::vm::native`, "Errors"; until then it was
/// terminal and neither block ran): the catch block gets the error object,
/// whose cause is the published error value, and the finally block runs.
#[test]
fn a_dependency_failure_inside_a_native_is_caught_around_it() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let caught;
         try { __rt.needAll(__rt.vector(5), 3); }
         catch (e) { __rt.effect(1, 1); caught = e; }
         finally { __rt.effect(1, 2); }
         caught",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    let error_value = rt.root(Primitive::Number(13.0));
    let resumed = fail(&rt, &task, &error_value);
    let caught = complete(run(&resumed, &mut log));
    let error = caught.error().expect("an error object");
    assert_eq!(error.kind(), VmErrorKind::DependencyFailed(key(&rt, A)));
    let cause = caught.error_cause().expect("a cause");
    assert_eq!(cause.view(), ValueView::Number(13.0));
    assert_eq!(log.0, [1.0, 2.0]);
}

// ---------------------------------------------------------------------------
// Consumer cancellation and finally blocks (`clj_quickjs::task`, module docs,
// "Cancellation and finally blocks"): not catchable; observed by the task's
// own run, which runs the pending finally blocks innermost first and then
// ends `Cancelled`. Nothing may override it or wait during it.
// ---------------------------------------------------------------------------

/// Cancel a parked task: the canceller gets it back to enqueue.
fn cancel_parked(task: &Task) -> Task {
    match task.cancel() {
        Cancel::Runnable(t) => t,
        other => panic!("expected Runnable, got {other:?}"),
    }
}

fn cancelled(outcome: RunOutcome) {
    assert!(
        matches!(outcome, RunOutcome::Cancelled),
        "expected Cancelled, got {outcome:?}"
    );
}

/// A task parked in a protected block inside a callee is cancelled: its
/// finally blocks run once, innermost first, in the next run; no catch block
/// and no code after a protected block runs; the run ends `Cancelled`. Its
/// waiter entry is gone, so the cell's later completion wakes only the other
/// consumer, which still gets the result: no task is revived and no waiter
/// is stranded.
#[test]
fn cancellation_runs_finally_blocks_but_no_catch_block() {
    let src = "
        function inner() {
          try { __rt.effect(1, 1); return __rt.need(5, 3); }
          catch (e) { __rt.effect(1, 91); }
          finally { __rt.effect(1, 2); }
        }
        try { inner(); __rt.effect(1, 92); }
        catch (e) { __rt.effect(1, 93); }
        finally { __rt.effect(1, 3); }
        __rt.effect(1, 94);";
    let rt = Runtime::new();
    let task = spawn(&rt, src);
    let other = spawn(&rt, "__rt.need(5, 3) + 1");
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    run_to_wait(&rt, &other, &mut Log::default(), A);
    let request = task.take_producer_requests().pop().unwrap();

    let resumed = cancel_parked(&task);
    assert_eq!(task.status(), TaskStatus::Ready);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(resumed, log);
    cancelled(outcome);
    assert_eq!(log.0, [1.0, 2.0, 3.0]);
    assert_eq!(task.status(), TaskStatus::Cancelled);
    assert_eq!(task.dependency_log().waiting(), None);
    assert!(matches!(
        rt.cell_status(key(&rt, A)),
        CellStatus::Pending { ref waiters, .. } if waiters == &[other.id()]
    ));

    // The producer request the cancelled task claimed is still served.
    let value = rt.root(Primitive::Number(7.0));
    let woken = only_runnable(
        rt.complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap(),
    );
    assert_eq!(woken.id(), other.id());
    assert_eq!(number(run(&woken, &mut Log::default())), 8.0);
    assert!(task.run().is_err(), "a cancelled task never runs again");
    assert_eq!(task.status(), TaskStatus::Cancelled);
}

/// Cancelled while waiting inside a finally block that has a pending return:
/// the cancellation is an abrupt completion at the wait, so the rest of that
/// block and the pending return are discarded, and only enclosing finally
/// blocks run.
#[test]
fn cancellation_while_waiting_in_finally_discards_its_rest() {
    let src = "
        function f() {
          try { try { return 1; } finally { __rt.effect(1, 1); __rt.need(5, 3); __rt.effect(1, 91); } }
          finally { __rt.effect(1, 2); }
        }
        try { f(); __rt.effect(1, 92); } finally { __rt.effect(1, 3); }";
    let rt = Runtime::new();
    let task = spawn(&rt, src);
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0]);
    cancelled(run(&cancel_parked(&task), &mut log));
    assert_eq!(log.0, [1.0, 2.0, 3.0]);
}

/// Cleanup code cannot override or outlive the cancellation: a `return` or
/// an exception escaping a finally block, or a `need` in it, ends the cleanup
/// there (enclosing finally blocks do not run; the run ends `Cancelled`). A
/// `try`/`catch` entirely inside a finally block still works, and the cleanup
/// carries on after it. A `need` during cleanup reads and records nothing.
#[test]
fn cleanup_code_cannot_override_the_cancellation_or_wait() {
    let cases: [(&str, &[f64]); 4] = [
        (
            "function f() {
               try { __rt.need(5, 3); } finally { __rt.effect(1, 1); return 5; }
             }
             try { f(); __rt.effect(1, 91); } finally { __rt.effect(1, 92); }",
            &[1.0],
        ),
        (
            "try {
               try { __rt.need(5, 3); } finally { __rt.effect(1, 1); throw 5; }
             } catch (e) { __rt.effect(1, 91); } finally { __rt.effect(1, 92); }",
            &[1.0],
        ),
        (
            "try {
               try { __rt.need(5, 3); }
               finally {
                 try { __rt.effect(1, 1); throw 5; } catch (e) { __rt.effect(1, e); }
                 __rt.effect(1, 2);
               }
             } catch (e) { __rt.effect(1, 91); } finally { __rt.effect(1, 3); }",
            &[1.0, 5.0, 2.0, 3.0],
        ),
        (
            "try {
               try { __rt.need(5, 3); }
               finally { __rt.effect(1, 1); __rt.need(6, 3); __rt.effect(1, 91); }
             } finally { __rt.effect(1, 92); }",
            &[1.0],
        ),
    ];
    for (src, expected) in cases {
        let rt = Runtime::new();
        let task = spawn(&rt, src);
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        assert!(log.0.is_empty(), "{src}");
        cancelled(run(&cancel_parked(&task), &mut log));
        assert_eq!(log.0, expected, "{src}");
        assert_eq!(task.status(), TaskStatus::Cancelled, "{src}");
        assert_eq!(task.dependency_log().edges(), &[key(&rt, A)], "{src}");
        assert!(matches!(rt.cell_status(key(&rt, B)), CellStatus::Absent));
    }
}

/// Cleanup code may call functions: a helper's own `return` (even through
/// its own finally block) and an exception thrown by a helper and caught
/// inside the cancelled finally block leave the cancellation pending, and
/// the cleanup carries on to the enclosing finally block.
#[test]
fn cancellation_cleanup_allows_calls_and_local_catches() {
    let src = "
        function ok(x) { __rt.effect(1, x); return x; }
        function guarded() { try { return 4; } finally { __rt.effect(1, 5); } }
        function bad() { throw 7; }
        try {
          try { __rt.need(5, 3); }
          finally {
            ok(1);
            try { bad(); } catch (e) { __rt.effect(1, e); }
            __rt.effect(1, guarded());
            ok(2);
          }
          __rt.effect(1, 91);
        } catch (e) { __rt.effect(1, 92); } finally { __rt.effect(1, 3); }
        __rt.effect(1, 93);";
    let rt = Runtime::new();
    let task = spawn(&rt, src);
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    let resumed = cancel_parked(&task);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(resumed, log);
    cancelled(outcome);
    assert_eq!(log.0, [1.0, 7.0, 5.0, 4.0, 2.0, 3.0]);
    assert_eq!(task.status(), TaskStatus::Cancelled);
    assert_eq!(task.dependency_log().waiting(), None);
    assert!(matches!(
        rt.cell_status(key(&rt, A)),
        CellStatus::Pending { ref waiters, .. } if waiters.is_empty()
    ));
}

/// A task without finally blocks is cancelled without running any code (the
/// clj-quickjs-24 terminal cleanup), and a catch block never sees a
/// cancellation.
#[test]
fn cancellation_without_finally_blocks_runs_no_code() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "try { __rt.need(5, 3); } catch (e) { __rt.effect(1, 91); } __rt.effect(1, 92);",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    cancelled(run(&cancel_parked(&task), &mut log));
    assert!(log.0.is_empty());
    assert_eq!(task.status(), TaskStatus::Cancelled);
}

/// Host operation 2 cancels the running task itself (a leaf effect may call
/// `Task::cancel`); operation 1 logs.
struct CancelHost {
    log: Vec<f64>,
    task: Option<Task>,
}

impl Host for CancelHost {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, call.arg) {
            (1, Primitive::Number(n)) => self.log.push(n),
            (2, _) => {
                let task = self.task.as_ref().ok_or("no task")?;
                assert!(matches!(task.cancel(), Cancel::Requested));
            }
            other => return Err(format!("unexpected host effect {other:?}")),
        }
        Ok(call.arg)
    }
}

/// A cancellation requested while the task runs is observed at the budget
/// boundary; the cleanup then runs under budgets too, yielding between runs
/// (the task stays runnable and is not revived by a stale wake), and ends
/// `Cancelled` once its last finally block finishes. Collections between the
/// cleanup's runs keep its state.
#[test]
fn cleanup_is_observed_at_the_budget_boundary_and_runs_under_budgets() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let i = 0;
         try {
           try { __rt.effect(2, 0); while (true) { i = i + 1; } }
           finally { let j = 0; while (j < 20) { j = j + 1; } __rt.effect(1, j); }
         } catch (e) { __rt.effect(1, 91); }
         finally { __rt.effect(1, 2); }",
    );
    let mut host = CancelHost {
        log: Vec::new(),
        task: Some(task.clone()),
    };
    let handle = task.try_begin_run().unwrap().wake_handle();
    let mut runs = 0;
    let outcome = loop {
        runs += 1;
        assert!(runs < 1000, "the cleanup must end");
        match task.run_with(Budget::instructions(10), &mut host).unwrap() {
            RunOutcome::Yielded => {
                assert_eq!(task.status(), TaskStatus::Ready);
                assert!(matches!(task.wake(handle), Wake::Stale));
                rt.collect().unwrap();
            }
            other => break other,
        }
    };
    cancelled(outcome);
    assert!(runs > 3, "the cleanup spanned several runs ({runs})");
    assert_eq!(host.log, [20.0, 2.0]);
    assert_eq!(task.status(), TaskStatus::Cancelled);
}

// ---------------------------------------------------------------------------
// Under the two-worker executor: migration and collection while a handler
// and a pending completion are live.
// ---------------------------------------------------------------------------

const TIMEOUT: Duration = Duration::from_secs(30);

/// Effects of every worker: (worker, task, argument), in execution order.
type Effects = Arc<Mutex<Vec<(WorkerId, TaskId, f64)>>>;

struct WorkerLog {
    worker: WorkerId,
    effects: Effects,
}

impl Host for WorkerLog {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, call.arg) {
            (1, Primitive::Number(n)) => {
                self.effects
                    .lock()
                    .unwrap()
                    .push((self.worker, call.task, n));
                Ok(call.arg)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

/// An executor whose woken tasks resume on the worker after the one they
/// parked on, and whose workers log effects into `effects`.
fn migrating_executor(rt: &Runtime, effects: &Effects) -> Executor {
    let effects = Arc::clone(effects);
    ExecutorConfig::new()
        .workers(2)
        .budget(Budget::instructions(1000))
        .host(move |worker| {
            Box::new(WorkerLog {
                worker,
                effects: Arc::clone(&effects),
            })
        })
        .placement(|r| match (r.reason, r.last_worker) {
            (EnqueueReason::Woken | EnqueueReason::Cancelled, Some(last)) => {
                Placement::Worker(WorkerId::new((last.index() + 1) % r.workers))
            }
            (_, Some(last)) => Placement::Worker(last),
            _ => Placement::Any,
        })
        .start(rt)
        .unwrap()
}

fn released(events: &[Event], task: TaskId, outcome: OutcomeKind) -> bool {
    events
        .iter()
        .any(|e| e.task == Some(task) && e.kind == EventKind::Released(outcome))
}

/// Request a collection on a worker and wait until it is recorded.
fn collect_on_worker(exec: &Executor, worker: WorkerId) {
    let before = exec
        .events()
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Collected(_)))
        .count();
    exec.schedule_collection(Placement::Worker(worker));
    assert!(exec.wait_for(TIMEOUT, |ev| {
        ev.iter()
            .filter(|e| matches!(e.kind, EventKind::Collected(_)))
            .count()
            > before
    }));
}

/// A task parks inside a finally block whose `return` of a fresh vector is
/// pending (the vector is held by nothing else), inside a function called
/// from a protected block with its own catch and finally blocks. Collections
/// run on both workers while it is parked; the woken task resumes on the
/// other worker and completes the pending return. Each effect ran exactly
/// once, on the worker the language's control flow put it on.
#[test]
fn a_pending_return_survives_migration_and_collection_under_the_executor() {
    let rt = Runtime::new();
    let effects = Effects::default();
    let exec = migrating_executor(&rt, &effects);
    let task = spawn(
        &rt,
        "function f() {
           try { __rt.effect(1, 1); return __rt.vector(__rt.effect(1, 2), 30); }
           finally { __rt.effect(1, 3); __rt.effect(1, __rt.need(5, 3)); }
         }
         let r;
         try { r = f(); } catch (e) { __rt.effect(1, 91); } finally { __rt.effect(1, 5); }
         __rt.nth(r, 0) + __rt.nth(r, 1)",
    );
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, A));
    assert!(exec.wait_for(TIMEOUT, |ev| released(ev, task.id(), parked)));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    assert_eq!(task.status(), TaskStatus::Waiting);

    let request = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::Number(4.0));
    let wakeups = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    let outcome = exec.take_outcome(task.id(), TIMEOUT).expect("an outcome");
    assert_eq!(number(outcome), 32.0);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    let id = task.id();
    assert_eq!(
        *effects.lock().unwrap(),
        [
            (w0, id, 1.0),
            (w0, id, 2.0),
            (w0, id, 3.0),
            (w1, id, 4.0),
            (w1, id, 5.0)
        ]
    );
    exec.shutdown(TIMEOUT).unwrap();
}

/// The executor cancels a task parked in a finally block with a pending
/// exception: the canceller enqueues it, a collection runs first, and its
/// cleanup run (on the other worker) runs only the enclosing finally block;
/// the catch block never sees the pending exception or the cancellation.
/// The cell's later completion finds no waiter to wake.
#[test]
fn the_executor_cancels_a_task_parked_in_finally() {
    let rt = Runtime::new();
    let effects = Effects::default();
    let exec = migrating_executor(&rt, &effects);
    let task = spawn(
        &rt,
        "try {
           try { __rt.effect(1, 1); throw 9; }
           finally { __rt.effect(1, 2); __rt.need(5, 3); __rt.effect(1, 91); }
         } catch (e) { __rt.effect(1, 92); } finally { __rt.effect(1, 3); }",
    );
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, A));
    assert!(exec.wait_for(TIMEOUT, |ev| released(ev, task.id(), parked)));
    collect_on_worker(&exec, WorkerId::new(1));
    assert!(exec.cancel(&task).unwrap(), "the canceller enqueued it");
    let outcome = exec.take_outcome(task.id(), TIMEOUT).expect("an outcome");
    cancelled(outcome);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    let id = task.id();
    assert_eq!(
        *effects.lock().unwrap(),
        [(w0, id, 1.0), (w0, id, 2.0), (w1, id, 3.0)]
    );
    let request = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::Number(4.0));
    let wakeups = rt
        .complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap();
    assert!(wakeups.deliveries().is_empty(), "no waiter left");
    assert_eq!(task.status(), TaskStatus::Cancelled);
    exec.shutdown(TIMEOUT).unwrap();
}
