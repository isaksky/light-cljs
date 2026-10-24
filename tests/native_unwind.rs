//! Failure, cancellation and migration of tasks inside resumable natives
//! (pellet clj-quickjs-28, `docs/design.md` §§10–11, 13; contract in
//! `src/vm/native.rs`, "Errors", and `src/task.rs`, "Cancellation and
//! finally blocks"). Programs are inline JavaScript source snippets
//! (synthetic fixtures, not light-cljs output).
//!
//! The tests play a manually driven host as in `tests/native_fold.rs`: they
//! run a task until it waits, take the producer request it claimed, publish
//! a result or a failure, and run the task the completion made runnable,
//! usually on another thread and with a collection in between; one test
//! uses the two-worker executor with a placement hook that moves the woken
//! task to the other worker. `__rt.effect(1, x)` logs `x` (and returns it),
//! so every log entry is one execution of that effect. Every cross-thread
//! step is sequenced by a join or an observed event, never by sleeping.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clj_quickjs::exec::{
    EnqueueReason, EventKind, ExecutorConfig, OutcomeKind, Placement, WorkerId,
};
use clj_quickjs::frontend::compile_function;
use clj_quickjs::{
    Budget, Cancel, CellStatus, Completion, DependencyId, DependencyKey, Host, HostCall, Primitive,
    ProducerRequest, ResultKind, Rooted, RunOutcome, Runtime, Task, TaskStatus, ValueView,
    VmErrorKind, Wakeups, WeakHandle,
};

const TIMEOUT: Duration = Duration::from_secs(30);

/// Host operation 1 logs its numeric argument and returns it.
#[derive(Default)]
struct Log(Vec<f64>);

impl Host for Log {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => {
                self.0.push(n);
                Ok(call.arg)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

fn key(rt: &Runtime, def: u64, kind: u32) -> DependencyKey {
    DependencyKey::new(rt.world(), DependencyId::new(def), ResultKind::new(kind))
}

fn spawn(rt: &Runtime, src: &str, args: &[&Rooted]) -> Task {
    let exe = compile_function(src).unwrap_or_else(|e| panic!("{src}: {e}"));
    rt.spawn(&exe, args).unwrap()
}

fn run(task: &Task, log: &mut Log) -> RunOutcome {
    task.run_with(Budget::UNLIMITED, log).unwrap()
}

/// Run on a new thread: the task migrates, and the log goes with it.
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

fn number(v: &Rooted) -> f64 {
    match v.view() {
        ValueView::Number(n) => n,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// The numbers in a vector result.
fn numbers(rt: &Runtime, v: &Rooted) -> Vec<f64> {
    rt.scope(|s| {
        let vec = s.local(v).unwrap();
        s.items(vec)
            .unwrap()
            .into_iter()
            .map(|x| match s.view(x) {
                ValueView::Number(n) => n,
                other => panic!("expected a number, got {other:?}"),
            })
            .collect()
    })
}

/// A persistent vector of numbers, rooted, and a weak handle to it.
fn number_vector(rt: &Runtime, items: &[f64]) -> (Rooted, WeakHandle) {
    let v = rt.scope(|s| {
        let items: Vec<_> = items
            .iter()
            .map(|&n| s.primitive(Primitive::Number(n)))
            .collect();
        let v = s.new_vector(&items).unwrap();
        s.persist(v)
    });
    let weak = v.downgrade();
    (v, weak)
}

fn one_request(task: &Task, expected: DependencyKey) -> ProducerRequest {
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.pop().unwrap();
    assert_eq!(request.key(), expected);
    request
}

fn only_runnable(wakeups: Wakeups) -> Task {
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    runnable.pop().unwrap()
}

fn publish(rt: &Runtime, request: &ProducerRequest, value: &Rooted) -> Task {
    only_runnable(
        rt.complete(request.key(), request.producer(), Completion::Ready(value))
            .unwrap(),
    )
}

fn publish_number(rt: &Runtime, request: &ProducerRequest, n: f64) -> Task {
    publish(rt, request, &rt.root(Primitive::Number(n)))
}

fn fail(rt: &Runtime, request: &ProducerRequest, error: &Rooted) -> Task {
    only_runnable(
        rt.complete(request.key(), request.producer(), Completion::Failed(error))
            .unwrap(),
    )
}

/// The source line of a failure.
fn line(error: &clj_quickjs::VmError) -> u32 {
    error.diagnostic().source.expect("a source location").line
}

/// Acceptance 1, callback exception: a callback throws after the task
/// waited inside it and resumed on another thread; the exception unwinds
/// through the fold into the catch block of the function that called
/// `reduce`, its finally block runs, the remaining element is never
/// visited, and the fold's frame is released: while the task is parked
/// again after the catch, the vector it alone held is reclaimed.
#[test]
fn a_callback_exception_unwinds_through_the_fold_into_the_callers_catch() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "function main(items) {
  function first(a, b) { return a; }
  function step(acc, x) {
    __rt.effect(1, x);
    const y = __rt.need(60, 1);
    if (x === 2) { throw acc + y; }
    return acc + y;
  }
  let r;
  try { r = __rt.reduce(step, 0, first(items, items = null)); __rt.effect(1, 99); }
  catch (e) { __rt.effect(1, 50); r = e; }
  finally { __rt.effect(1, 60); }
  return r + __rt.need(61, 1);
}",
        &[&number_vector(&rt, &[1.0, 2.0, 3.0]).0],
    );
    // The argument's only root is gone; the task holds the vector.
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), key(&rt, 60, 1));
    let request = one_request(&task, key(&rt, 60, 1));
    rt.collect().unwrap();

    let resumed = publish_number(&rt, &request, 10.0);
    let (outcome, log) = run_elsewhere(resumed, log);
    assert_eq!(waiting(outcome), key(&rt, 61, 1));
    assert_eq!(log.0, [1.0, 2.0, 50.0, 60.0], "element 3 never visited");
    let request = one_request(&task, key(&rt, 61, 1));
    let result = complete(run_elsewhere(publish_number(&rt, &request, 100.0), log).0);
    // The thrown value: acc (10) + y (10).
    assert_eq!(number(&result), 120.0);
}

/// The fold's state is released by the unwinding itself, not only when the
/// task ends: parked after the catch, the task no longer holds the vector
/// that only the fold reached.
#[test]
fn unwinding_releases_what_only_the_native_held() {
    let rt = Runtime::new();
    let (items, weak) = number_vector(&rt, &[1.0, 2.0]);
    let task = spawn(
        &rt,
        "function main(items) {
  function first(a, b) { return a; }
  function step(acc, x) { throw x; }
  let r;
  try { r = __rt.reduce(step, 0, first(items, items = null)); }
  catch (e) { r = e; }
  return r + __rt.need(62, 1);
}",
        &[&items],
    );
    drop(items);
    assert_eq!(waiting(run(&task, &mut Log::default())), key(&rt, 62, 1));
    rt.collect().unwrap();
    assert!(
        !weak.is_live(),
        "released when the exception passed the fold"
    );
    assert_eq!(task.status(), TaskStatus::Waiting);
    let request = one_request(&task, key(&rt, 62, 1));
    let resumed = publish_number(&rt, &request, 5.0);
    assert_eq!(number(&complete(run(&resumed, &mut Log::default()))), 6.0);
}

/// Acceptance 1, producer failure: `needAll` waits for its second
/// definition, whose producer fails while the task is parked; the run that
/// resumes it on another thread delivers the failure to the native, which
/// unwinds into the caller's catch block with the error object (its cause
/// is the producer's published error value, by identity; its position is
/// the `needAll` call), then the finally block runs. The definitions vector
/// only the native held is released before the task ends.
#[test]
fn a_producer_failure_unwinds_through_need_all_into_the_callers_catch() {
    let rt = Runtime::new();
    let (defs, weak) = number_vector(&rt, &[70.0, 71.0]);
    let task = spawn(
        &rt,
        "function main(defs) {
  function first(a, b) { return a; }
  let r;
  try { r = __rt.needAll(first(defs, defs = null), 3); __rt.effect(1, 99); }
  catch (e) { __rt.effect(1, 50); r = e; }
  finally { __rt.effect(1, 60); }
  __rt.need(72, 3);
  return r;
}",
        &[&defs],
    );
    drop(defs);
    // Definition 70 is ready, as a heap value the results so far hold.
    let ready = rt.claim(key(&rt, 70, 3)).unwrap();
    let _ = rt
        .complete(
            ready.key(),
            ready.producer(),
            Completion::Ready(&number_vector(&rt, &[700.0]).0),
        )
        .unwrap();
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), key(&rt, 71, 3));
    let request = one_request(&task, key(&rt, 71, 3));
    rt.collect().unwrap();
    assert!(weak.is_live(), "held by the waiting native");

    let error_value = number_vector(&rt, &[-71.0]).0;
    let (outcome, log) = run_elsewhere(fail(&rt, &request, &error_value), log);
    assert_eq!(waiting(outcome), key(&rt, 72, 3));
    assert_eq!(log.0, [50.0, 60.0]);
    rt.collect().unwrap();
    assert!(!weak.is_live(), "released by the unwinding");

    let request = one_request(&task, key(&rt, 72, 3));
    let caught = complete(run_elsewhere(publish_number(&rt, &request, 0.0), log).0);
    let error = caught.error().expect("an error object");
    assert_eq!(error.kind(), VmErrorKind::DependencyFailed(key(&rt, 71, 3)));
    assert_eq!(line(&error), 4, "reported at the needAll call: {error}");
    let cause = caught.error_cause().expect("a cause");
    assert!(
        cause.same_object(&error_value),
        "the published value itself"
    );
    assert_eq!(
        task.dependency_log().edges(),
        [70, 71, 72].map(|d| key(&rt, d, 3))
    );
}

/// A producer failure observed inside a callback (its `need`) unwinds out
/// of the callback and through the fold into the caller's catch block,
/// like any other callback exception; also when the failure was already
/// published before the callback needed it.
#[test]
fn a_producer_failure_inside_a_callback_unwinds_through_the_fold() {
    let rt = Runtime::new();
    let src = "function main(items) {
  function step(acc, x) { __rt.effect(1, x); return acc + __rt.need(x, 1); }
  try { return __rt.reduce(step, 0, items); }
  catch (e) { __rt.effect(1, 50); return e; }
}";
    let (items, _) = number_vector(&rt, &[1.0, 2.0, 3.0]);
    // Definition 1 is ready; definition 2 fails after the task waits.
    let ready = rt.claim(key(&rt, 1, 1)).unwrap();
    let _ = rt
        .complete(
            ready.key(),
            ready.producer(),
            Completion::Ready(&rt.root(Primitive::Number(5.0))),
        )
        .unwrap();
    let task = spawn(&rt, src, &[&items]);
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), key(&rt, 2, 1));
    let request = one_request(&task, key(&rt, 2, 1));
    let error_value = rt.root(Primitive::Number(-2.0));
    let (outcome, log) = run_elsewhere(fail(&rt, &request, &error_value), log);
    let caught = complete(outcome);
    assert_eq!(log.0, [1.0, 2.0, 50.0]);
    let error = caught.error().expect("an error object");
    assert_eq!(error.kind(), VmErrorKind::DependencyFailed(key(&rt, 2, 1)));
    assert_eq!(line(&error), 2, "reported at the callback's need: {error}");
    assert_eq!(error.diagnostic().call_chain.len(), 1, "{error}");
    assert_eq!(
        caught.error_cause().unwrap().view(),
        ValueView::Number(-2.0)
    );

    // Already failed when a second task's callback needs it.
    let task = spawn(&rt, src, &[&items]);
    let mut log = Log::default();
    let caught = complete(run(&task, &mut log));
    assert_eq!(log.0, [1.0, 2.0, 50.0]);
    let error = caught.error().expect("an error object");
    assert_eq!(error.kind(), VmErrorKind::DependencyFailed(key(&rt, 2, 1)));
}

/// A callback call the native's step cannot make (entering the callback
/// would exceed the task's call-depth limit) raises a catchable
/// `StackOverflow` at the `reduce` call before any callback frame exists:
/// the native frame, still awaiting the call's result with the callee and
/// arguments in its operand area, is unwound into the caller's catch, the
/// finally block runs once, and the task can still wait and resume after.
#[test]
fn a_refused_callback_call_unwinds_the_native_into_the_callers_catch() {
    let rt = Runtime::with_limits(clj_quickjs::VmLimits {
        max_call_depth: 1,
        ..clj_quickjs::VmLimits::DEFAULT
    });
    let task = spawn(
        &rt,
        "function main(items) {
  function step(acc, x) { __rt.effect(1, 99); return acc + x; }
  let r;
  try { r = __rt.reduce(step, 0, items); }
  catch (e) { __rt.effect(1, 50); r = e; }
  finally { __rt.effect(1, 60); }
  return __rt.vector(r, __rt.need(63, 1));
}",
        &[&number_vector(&rt, &[1.0, 2.0]).0],
    );
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), key(&rt, 63, 1));
    assert_eq!(log.0, [50.0, 60.0], "the callback never ran");
    let request = one_request(&task, key(&rt, 63, 1));
    let (outcome, log) = run_elsewhere(publish_number(&rt, &request, 3.0), log);
    let result = complete(outcome);
    assert_eq!(log.0, [50.0, 60.0]);
    let caught = rt.scope(|s| {
        let v = s.local(&result).unwrap();
        let items = s.items(v).unwrap();
        assert_eq!(s.view(items[1]), ValueView::Number(3.0));
        s.persist(items[0])
    });
    let error = caught.error().expect("an error object");
    assert_eq!(error.kind(), VmErrorKind::StackOverflow, "{error}");
    assert_eq!(line(&error), 4, "reported at the reduce call: {error}");
}

/// Nested natives: an exception raised in the inner fold's callback passes
/// both natives (innermost first) into the outermost caller's catch; a
/// finally block of the outer callback, between the two natives, runs on
/// the way.
#[test]
fn an_exception_unwinds_through_nested_natives() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "function main(rows) {
  function cell(a, x) { if (x === 3) { throw 30 + a; } return a + x; }
  function row(acc, r) {
    try { return acc + __rt.reduce(cell, 0, r); }
    finally { __rt.effect(1, 40 + acc); }
  }
  try { return __rt.reduce(row, 0, rows); }
  catch (e) { __rt.effect(1, e); return -e; }
}",
        &[&rt.scope(|s| {
            let rows: Vec<_> = [2, 4]
                .into_iter()
                .map(|n| {
                    let items: Vec<_> = (1..=n)
                        .map(|i| s.primitive(Primitive::Number(f64::from(i))))
                        .collect();
                    s.new_vector(&items).unwrap()
                })
                .collect();
            let v = s.new_vector(&rows).unwrap();
            s.persist(v)
        })],
    );
    let mut log = Log::default();
    // Row 1 sums to 3; row 2 throws 30 + (1 + 2) at its third cell.
    assert_eq!(number(&complete(run(&task, &mut log))), -33.0);
    assert_eq!(log.0, [40.0, 43.0, 33.0]);
}

/// Acceptance 2: a task waiting inside the native callback resumes on a
/// different recorded worker of the two-worker executor and completes
/// correctly, and the transient builder the task created before the fold
/// (captured by the callback) is still the task's: the callback adds to it
/// before the wait on one worker and after it on the other, and the task
/// freezes it at the end.
#[test]
fn a_task_waiting_in_a_callback_resumes_on_another_worker_with_its_builder() {
    let rt = Runtime::new();
    let effects: Arc<Mutex<Vec<(WorkerId, f64)>>> = Arc::default();
    struct WorkerLog {
        worker: WorkerId,
        effects: Arc<Mutex<Vec<(WorkerId, f64)>>>,
    }
    impl Host for WorkerLog {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            let Primitive::Number(n) = call.arg else {
                return Err(format!("unexpected argument {:?}", call.arg));
            };
            self.effects.lock().unwrap().push((self.worker, n));
            Ok(call.arg)
        }
    }
    let host_effects = Arc::clone(&effects);
    let exec = ExecutorConfig::new()
        .budget(Budget::instructions(7))
        .host(move |worker| {
            Box::new(WorkerLog {
                worker,
                effects: Arc::clone(&host_effects),
            })
        })
        .placement(|r| match (r.reason, r.last_worker) {
            // A task that waited resumes on the other worker.
            (EnqueueReason::Woken, Some(last)) => {
                Placement::Worker(WorkerId::new((last.index() + 1) % r.workers))
            }
            // Budget yields stay put, keeping the trace deterministic.
            (EnqueueReason::Yielded, Some(last)) => Placement::Worker(last),
            _ => Placement::Any,
        })
        .start(&rt)
        .unwrap();
    let (items, _) = number_vector(&rt, &[1.0, 2.0, 3.0]);
    let task = spawn(
        &rt,
        "function main(items) {
  const b = __rt.vectorBuilder();
  __rt.builderConj(b, 0);
  function step(acc, x) {
    __rt.effect(1, x);
    __rt.builderConj(b, x * __rt.need(80, 1));
    return acc + x;
  }
  const sum = __rt.reduce(step, 0, items);
  __rt.builderConj(b, sum);
  return __rt.freeze(b);
}",
        &[&items],
    );
    let need = key(&rt, 80, 1);
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let id = task.id();
    let parked = |events: &[clj_quickjs::exec::Event]| {
        events.iter().any(|e| {
            e.task == Some(id) && e.kind == EventKind::Released(OutcomeKind::Waiting(need))
        })
    };
    assert!(exec.wait_for(TIMEOUT, parked), "{:#?}", exec.events());
    assert_eq!(task.status(), TaskStatus::Waiting);
    // Collected while parked inside the callback inside the fold.
    rt.collect().unwrap();

    let request = one_request(&task, need);
    let value = rt.root(Primitive::Number(10.0));
    let wakeups = rt
        .complete(need, request.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    let result = complete(exec.take_outcome(id, TIMEOUT).expect("an outcome"));
    assert_eq!(numbers(&rt, &result), [0.0, 10.0, 20.0, 30.0, 6.0]);

    let events = exec.events();
    let workers: Vec<WorkerId> = events
        .iter()
        .filter(|e| e.task == Some(id))
        .filter_map(|e| match e.kind {
            EventKind::Dispatched { .. } => e.worker,
            _ => None,
        })
        .collect();
    let park = events
        .iter()
        .position(|e| {
            e.task == Some(id) && e.kind == EventKind::Released(OutcomeKind::Waiting(need))
        })
        .unwrap();
    let before = events[..park]
        .iter()
        .filter(|e| e.task == Some(id) && matches!(e.kind, EventKind::Dispatched { .. }))
        .count();
    assert!(before >= 2, "the prefix yields at least once: {events:#?}");
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    assert!(workers[..before].iter().all(|&w| w == w0), "{workers:?}");
    assert!(workers[before..].iter().all(|&w| w == w1), "{workers:?}");
    assert!(!workers[before..].is_empty());
    // One effect per element, the first before the wait on worker 0, the
    // rest after it on worker 1: nothing replayed.
    assert_eq!(*effects.lock().unwrap(), [(w0, 1.0), (w1, 2.0), (w1, 3.0)]);
    let down = exec.shutdown(TIMEOUT).unwrap();
    assert!(down.pending.is_empty() && down.parked.is_empty());
}

/// Acceptance 3, inside a callback: a task suspended in a protected block
/// of a fold's callback is cancelled and the cancellation is run on another
/// thread. The documented policy holds: the callback's finally block runs,
/// then the caller's (the fold is dropped on the way, running no native
/// code), no catch block and nothing after them runs, and the run ends
/// `Cancelled`. Its waiter entry is gone, so the producer's later
/// completion wakes only the other consumer, which completes; the vector
/// only the fold held is reclaimed.
#[test]
fn cancellation_inside_a_callback_runs_finally_blocks_and_releases_the_fold() {
    let rt = Runtime::new();
    let (items, weak) = number_vector(&rt, &[1.0, 2.0]);
    let task = spawn(
        &rt,
        "function main(items) {
  function first(a, b) { return a; }
  function step(acc, x) {
    try { return acc + __rt.need(90, 1); }
    catch (e) { __rt.effect(1, 91); }
    finally { __rt.effect(1, 2); }
  }
  try { return __rt.reduce(step, 0, first(items, items = null)); }
  catch (e) { __rt.effect(1, 92); }
  finally { __rt.effect(1, 3); }
  __rt.effect(1, 94);
}",
        &[&items],
    );
    drop(items);
    let other = spawn(
        &rt,
        "function other() { return __rt.need(90, 1) + 1; }",
        &[],
    );
    let need = key(&rt, 90, 1);
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), need);
    assert_eq!(waiting(run(&other, &mut Log::default())), need);
    let request = one_request(&task, need);

    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("a parked task is made runnable to observe its cancellation");
    };
    rt.collect().unwrap();
    assert!(
        weak.is_live(),
        "still held until the cancellation is observed"
    );
    let (outcome, log) = run_elsewhere(cancelled, log);
    assert!(matches!(outcome, RunOutcome::Cancelled), "{outcome:?}");
    assert_eq!(log.0, [2.0, 3.0]);
    assert_eq!(task.status(), TaskStatus::Cancelled);
    assert_eq!(task.dependency_log().waiting(), None);
    assert!(matches!(
        rt.cell_status(need),
        CellStatus::Pending { ref waiters, .. } if waiters == &[other.id()]
    ));
    rt.collect().unwrap();
    assert!(!weak.is_live(), "nothing the fold held is a root");

    // The producer request the cancelled task claimed still serves the
    // other consumer.
    let woken = publish_number(&rt, &request, 7.0);
    assert_eq!(woken.id(), other.id());
    assert_eq!(number(&complete(run(&woken, &mut Log::default()))), 8.0);
}

/// Acceptance 3, at a dependency the native demanded: a task suspended in
/// `needAll` is cancelled. The cleanup unwinds to the caller's finally
/// block, dropping the native frame first: one cleanup instruction per run,
/// and a collection after the first run already finds the definitions
/// vector (held only by the native) reclaimed while the task is still
/// cleaning up. Another consumer of the same producer is not blocked.
#[test]
fn cancellation_inside_need_all_drops_the_native_before_the_finally_block() {
    let rt = Runtime::new();
    let (defs, weak) = number_vector(&rt, &[95.0, 96.0]);
    let task = spawn(
        &rt,
        "function main(defs) {
  function first(a, b) { return a; }
  try { return __rt.needAll(first(defs, defs = null), 3); }
  catch (e) { __rt.effect(1, 91); }
  finally { __rt.effect(1, 1); __rt.effect(1, 2); }
}",
        &[&defs],
    );
    drop(defs);
    let other = spawn(&rt, "function other() { return __rt.need(95, 3); }", &[]);
    let need = key(&rt, 95, 3);
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), need);
    assert_eq!(waiting(run(&other, &mut Log::default())), need);
    let request = one_request(&task, need);
    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("a parked task is made runnable to observe its cancellation");
    };

    let mut runs = 0;
    let outcome = loop {
        let outcome = cancelled
            .run_with(Budget::instructions(1), &mut log)
            .unwrap();
        runs += 1;
        if !matches!(outcome, RunOutcome::Yielded) {
            break outcome;
        }
        assert_eq!(task.status(), TaskStatus::Ready, "still cleaning up");
        rt.collect().unwrap();
        assert!(!weak.is_live(), "the native was dropped by run {runs}");
    };
    assert!(matches!(outcome, RunOutcome::Cancelled), "{outcome:?}");
    assert!(runs > 2, "{runs}");
    assert_eq!(log.0, [1.0, 2.0]);
    assert!(matches!(
        rt.cell_status(need),
        CellStatus::Pending { ref waiters, .. } if waiters == &[other.id()]
    ));
    let woken = publish_number(&rt, &request, 4.0);
    assert_eq!(number(&complete(run(&woken, &mut Log::default()))), 4.0);
}
