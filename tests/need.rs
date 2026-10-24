//! The reserved `__rt.need` intrinsic lowered to the resumable VM `need`
//! instruction (pellet clj-quickjs-23, `docs/design.md` §§7, 10), through
//! the public embedding API. Programs are JavaScript source: the synthetic
//! fixtures `fixtures/js/need_consumer.js` and `need_producer.js` (not
//! light-cljs output) and inline snippets.
//!
//! The tests play a deterministic, manually driven host: they run tasks on
//! threads they own, take the producer requests a suspended task's needs
//! claimed, run producer tasks, publish results, and enqueue (here: run) the
//! tasks the completion's wakeups return. No promise, callback or retained
//! Rust frame stands for a waiting task: a suspended task is plain VM state,
//! and the run that resumes it may be on another thread.

use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use clj_quickjs::exec::{
    EnqueueReason, Event, EventKind, Executor, ExecutorConfig, OutcomeKind, Placement, WorkerId,
};
use clj_quickjs::frontend::{SyntaxErrorKind, compile_function, compile_script};
use clj_quickjs::{
    Budget, Cancel, CellStatus, Completion, DependencyId, DependencyKey, Executable, Host,
    HostCall, Primitive, ProducerRequest, ResultKind, RunOutcome, Runtime, Task, TaskStatus,
    ValueView, VmError, VmErrorKind, Wakeups,
};

/// `__rt.need(5, 3)`: a number.
const NUMBER: (DependencyId, ResultKind) = (DependencyId::new(5), ResultKind::new(3));
/// `__rt.need(6, 2)`: a callable.
const CALLABLE: (DependencyId, ResultKind) = (DependencyId::new(6), ResultKind::new(2));

fn key(rt: &Runtime, (def, kind): (DependencyId, ResultKind)) -> DependencyKey {
    DependencyKey::new(rt.world(), def, kind)
}

/// Host operation 1 (the pre-need effect) returns its argument plus 100;
/// operation 2 reports a value and returns 0. Every call is recorded.
#[derive(Default)]
struct Recorder {
    effects: Vec<f64>,
    reports: Vec<f64>,
}

impl Host for Recorder {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            return Err(format!("unexpected argument {:?}", call.arg));
        };
        match call.op {
            1 => {
                self.effects.push(n);
                Ok(Primitive::Number(n + 100.0))
            }
            2 => {
                self.reports.push(n);
                Ok(Primitive::Number(0.0))
            }
            op => Err(format!("unexpected host operation {op}")),
        }
    }
}

fn consumer_exe() -> Executable {
    compile_function(include_str!("fixtures/js/need_consumer.js")).unwrap()
}

fn consumer(rt: &Runtime, base: f64) -> Task {
    let base = rt.root(Primitive::Number(base));
    rt.spawn(&consumer_exe(), &[&base]).unwrap()
}

fn run(task: &Task, host: &mut Recorder) -> RunOutcome {
    task.run_with(Budget::UNLIMITED, host).unwrap()
}

fn waiting(outcome: RunOutcome) -> DependencyKey {
    match outcome {
        RunOutcome::Waiting(key) => key,
        other => panic!("expected Waiting, got {other:?}"),
    }
}

fn number(outcome: RunOutcome) -> f64 {
    match outcome {
        RunOutcome::Complete(v) => match v.view() {
            ValueView::Number(n) => n,
            other => panic!("expected a number, got {other:?}"),
        },
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn failed(outcome: RunOutcome) -> VmError {
    match outcome {
        RunOutcome::Failed(e) => e,
        other => panic!("expected Failed, got {other:?}"),
    }
}

/// The one producer request a run claimed, for `expected`.
fn one_request(task: &Task, expected: DependencyKey) -> ProducerRequest {
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.pop().unwrap();
    assert_eq!(request.key(), expected);
    request
}

/// Publish `n` for `request` and return the wakeups.
fn publish_number(rt: &Runtime, request: &ProducerRequest, n: f64) -> Wakeups {
    let value = rt.root(Primitive::Number(n));
    rt.complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap()
}

/// The single task a completion made runnable.
fn only_runnable(wakeups: Wakeups) -> Task {
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    runnable.pop().unwrap()
}

/// Source line and column of a failure.
fn line_column(e: &VmError) -> (u32, u32) {
    let at = e.diagnostic().source.expect("source location");
    (at.line, at.column)
}

/// The fixture's line of `__rt.need(5, 3)` and the column of its `(`.
const NUMBER_NEED_AT: (u32, u32) = (12, 34);

/// The whole protocol, driven by hand: the consumer runs its pre-need
/// effect and suspends at a need inside a nested call; the run returns
/// `Waiting` and hands back the producer request; the task is collected
/// around while parked; publication only wakes it; a run on another thread
/// continues at the need (the inner frame's live operand and the outer
/// locals intact), suspends again on a callable produced by another VM task,
/// and finally invokes that callable. The pre-need effect ran once and the
/// function was never restarted.
#[test]
fn a_manually_driven_host_suspends_and_resumes_at_the_need() {
    let rt = Runtime::new();
    let task = consumer(&rt, 3.0);
    let mut host = Recorder::default();

    // Run 1: pre-need effect, then the need of definition 5 is pending.
    let number_key = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    assert_eq!(task.status(), TaskStatus::Waiting);
    assert_eq!(host.effects, [3.0]);
    let number_request = one_request(&task, number_key);
    assert!(task.take_producer_requests().is_empty(), "taken once");
    let at = task.source_location().expect("suspended at the need");
    assert_eq!((at.line, at.column), NUMBER_NEED_AT);
    let log = task.dependency_log();
    assert_eq!(log.waiting(), Some(number_key));
    assert!(matches!(
        rt.cell_status(number_key),
        CellStatus::Pending { producer: Some(_), ref waiters } if waiters == &[task.id()]
    ));

    // While it waits: a collection keeps its frames (including the binding
    // cell of `lookups` and the closure `lookup`), and nothing can run it.
    rt.collect().unwrap();
    assert!(task.run().is_err());

    // Publication runs no JavaScript inline: it only reports the wake.
    let resumed = only_runnable(publish_number(&rt, &number_request, 7.0));
    assert_eq!(resumed.id(), task.id());
    assert_eq!(task.status(), TaskStatus::Ready);
    assert_eq!((host.effects.len(), host.reports.len()), (1, 0));

    // Run 2, on another thread: continues at the need with `3 * 10` still
    // on the inner frame's operand stack, then suspends on the callable.
    let callable_key = key(&rt, CALLABLE);
    let (outcome, mut host) = thread::spawn(move || {
        let outcome = run(&resumed, &mut host);
        (outcome, host)
    })
    .join()
    .unwrap();
    assert_eq!(waiting(outcome), callable_key);
    assert_eq!(host.effects, [3.0], "the pre-need effect is not replayed");
    assert_eq!(host.reports, [37.0]);
    let callable_request = one_request(&task, callable_key);

    // The producer is another VM task whose result is a function compiled
    // by this runtime; the host publishes it as the callable result.
    let producer_exe = compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
    let offset = rt.root(Primitive::Number(1000.0));
    let producer = rt.spawn(&producer_exe, &[&offset]).unwrap();
    let RunOutcome::Complete(helper) = producer.run().unwrap() else {
        panic!("the producer returns its function");
    };
    assert_eq!(helper.view(), ValueView::Object);
    let wakeups = rt
        .complete(
            callable_request.key(),
            callable_request.producer(),
            Completion::Ready(&helper),
        )
        .unwrap();
    let resumed = only_runnable(wakeups);
    assert_eq!(task.status(), TaskStatus::Ready);

    // Run 3: the published callable is invoked: pre + helper(37).
    assert_eq!(
        number(run(&resumed, &mut host)),
        103.0 + (37.0 * 2.0 + 1000.0)
    );
    assert_eq!(host.effects, [3.0]);
    assert_eq!(host.reports, [37.0, 1.0], "`lookup` ran once");
    let log = task.dependency_log();
    assert_eq!(log.edges(), &[number_key, callable_key]);
    assert_eq!(log.waiting(), None);
    assert!(task.take_producer_requests().is_empty());
}

/// A template literal (clj-quickjs-57) keeps its partial string across
/// budget yields and a dependency wait: the run suspends at a need in the
/// second substitution with `"a101b"` built, is collected while parked,
/// resumes on another thread, and finishes without replaying the host
/// effect of the first substitution.
#[test]
fn template_literal_resumes_after_wait() {
    let rt = Runtime::new();
    let exe = compile_script("t", "`a${__rt.effect(1, 1)}b${__rt.need(5, 3)}c`").unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    let mut runs = 0;
    let request = loop {
        runs += 1;
        assert!(runs < 1_000, "bounded");
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                assert_eq!(k, number_key);
                break one_request(&task, number_key);
            }
            other => panic!("expected the wait, got {other:?}"),
        }
    };
    assert!(runs > 3, "the budget split the run ({runs} runs)");
    assert_eq!(host.effects, [1.0]);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 42.0));
    let (outcome, host) = thread::spawn(move || {
        let outcome = loop {
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                other => break other,
            }
        };
        (outcome, host)
    })
    .join()
    .unwrap();
    match outcome {
        RunOutcome::Complete(v) => {
            assert_eq!(v.string().expect("a string").to_string(), "a101b42c");
        }
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(host.effects, [1.0], "the effect is not replayed");
}

/// A property update (clj-quickjs-76) keeps its reference across budget
/// yields and a dependency wait in its computed key: the run suspends at a
/// need in `get()[key]++` with the base on the operand stack, is collected
/// while parked, resumes on another thread one instruction at a time (the
/// postfix old value in its scratch local across yields), and finishes with
/// the base and the key evaluated once and the effect not replayed.
#[test]
fn property_update_resumes_after_wait() {
    let rt = Runtime::new();
    let exe = compile_script(
        "t",
        "let n = 0, m = 0; const p = {k: 1}; \
         const get = () => { m = m + __rt.effect(1, 1) - 100; return p; }; \
         const r = get()[(n = n + 1, __rt.need(5, 3) === 42 ? 'k' : 'z')]++; \
         const s = ++p.k; \
         `${r},${s},${p.k},${p.z},${n},${m}`",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    let mut runs = 0;
    let request = loop {
        runs += 1;
        assert!(runs < 1_000, "bounded");
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                assert_eq!(k, number_key);
                break one_request(&task, number_key);
            }
            other => panic!("expected the wait, got {other:?}"),
        }
    };
    assert!(runs > 3, "the budget split the run ({runs} runs)");
    assert_eq!(host.effects, [1.0]);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 42.0));
    let (outcome, host) = thread::spawn(move || {
        let mut runs = 0;
        let outcome = loop {
            runs += 1;
            assert!(runs < 1_000, "bounded");
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                other => break other,
            }
        };
        (outcome, host)
    })
    .join()
    .unwrap();
    match outcome {
        RunOutcome::Complete(v) => {
            assert_eq!(
                v.string().expect("a string").to_string(),
                "1,3,3,undefined,1,1"
            );
        }
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(host.effects, [1.0], "the effect is not replayed");
}

/// A compound assignment on a property (clj-quickjs-260) keeps its
/// reference and the value it read across budget yields and a dependency
/// wait in its right operand: the run suspends at a need in
/// `get()[key] += ...` with the base, the converted key and the old value
/// on the operand stack, is collected while parked, resumes on another
/// thread one instruction at a time, and finishes with the base and the key
/// evaluated once, the old value used, and the effect not replayed.
#[test]
fn property_compound_assignment_resumes_after_wait() {
    let rt = Runtime::new();
    let exe = compile_script(
        "t",
        "let n = 0, m = 0; const p = {k: 'v'}; \
         const get = () => { m = m + __rt.effect(1, 1) - 100; return p; }; \
         const r = get()[(n = n + 1, 'k')] += (p.k = 'w', __rt.need(5, 3)); \
         p.k *= 1; \
         `${r},${p.k},${n},${m}`",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    let mut runs = 0;
    let request = loop {
        runs += 1;
        assert!(runs < 1_000, "bounded");
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                assert_eq!(k, number_key);
                break one_request(&task, number_key);
            }
            other => panic!("expected the wait, got {other:?}"),
        }
    };
    assert!(runs > 3, "the budget split the run ({runs} runs)");
    assert_eq!(host.effects, [1.0]);
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 42.0));
    let (outcome, host) = thread::spawn(move || {
        let mut runs = 0;
        let outcome = loop {
            runs += 1;
            assert!(runs < 1_000, "bounded");
            match resumed
                .run_with(Budget::instructions(1), &mut host)
                .unwrap()
            {
                RunOutcome::Yielded => {}
                other => break other,
            }
        };
        (outcome, host)
    })
    .join()
    .unwrap();
    match outcome {
        RunOutcome::Complete(v) => {
            // `'v' + 42`, written over the right operand's `'w'`, then
            // `'v42' * 1` is NaN.
            assert_eq!(v.string().expect("a string").to_string(), "v42,NaN,1,1");
        }
        other => panic!("expected Complete, got {other:?}"),
    }
    assert_eq!(host.effects, [1.0], "the effect is not replayed");
}

/// A need of a completed cell continues in the same run: no suspension, no
/// producer request, and the read is still recorded.
#[test]
fn a_ready_read_continues_in_the_same_run() {
    let rt = Runtime::new();
    let number_request = rt.claim(key(&rt, NUMBER)).unwrap();
    assert!(
        publish_number(&rt, &number_request, 7.0)
            .deliveries()
            .is_empty()
    );
    let callable = rt.claim(key(&rt, CALLABLE)).unwrap();
    let producer_exe = compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
    let offset = rt.root(Primitive::Number(-5.0));
    let RunOutcome::Complete(helper) = rt.spawn(&producer_exe, &[&offset]).unwrap().run().unwrap()
    else {
        panic!("the producer returns its function");
    };
    let wakeups = rt
        .complete(
            callable.key(),
            callable.producer(),
            Completion::Ready(&helper),
        )
        .unwrap();
    assert!(wakeups.deliveries().is_empty());

    let task = consumer(&rt, 2.0);
    let mut host = Recorder::default();
    // 2 + 100 + helper(2 * 10 + 7) = 102 + 54 - 5
    assert_eq!(number(run(&task, &mut host)), 151.0);
    assert_eq!(
        (host.effects.as_slice(), host.reports.as_slice()),
        (&[2.0][..], &[27.0, 1.0][..])
    );
    assert!(task.take_producer_requests().is_empty());
    let log = task.dependency_log();
    assert_eq!(log.edges(), &[key(&rt, NUMBER), key(&rt, CALLABLE)]);
    assert_eq!(log.waiting(), None);
}

/// Budgeted runs cut the consumer into many pieces; the need's suspension
/// is one of them, and no piece replays work.
#[test]
fn budgeted_runs_and_the_wait_never_replay_work() {
    let rt = Runtime::new();
    let task = consumer(&rt, 4.0);
    let mut host = Recorder::default();
    let mut runs = 0;
    let number_key = key(&rt, NUMBER);
    let mut requests = Vec::new();
    let result = loop {
        runs += 1;
        assert!(runs < 10_000, "bounded");
        match task.run_with(Budget::instructions(2), &mut host).unwrap() {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(k) => {
                requests.extend(task.take_producer_requests());
                let request = requests.pop().expect("one request per pending key");
                assert_eq!(request.key(), k);
                if k == number_key {
                    // The wake makes it runnable; the loop runs it again.
                    let _ = only_runnable(publish_number(&rt, &request, 1.0));
                } else {
                    let exe =
                        compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
                    let offset = rt.root(Primitive::Number(0.0));
                    let RunOutcome::Complete(helper) =
                        rt.spawn(&exe, &[&offset]).unwrap().run().unwrap()
                    else {
                        panic!("the producer returns its function");
                    };
                    let wakeups = rt
                        .complete(k, request.producer(), Completion::Ready(&helper))
                        .unwrap();
                    let _ = only_runnable(wakeups);
                }
            }
            other => break number(other),
        }
    };
    // 4 + 100 + helper(41) = 104 + 82
    assert_eq!(result, 186.0);
    assert_eq!(
        (host.effects.as_slice(), host.reports.as_slice()),
        (&[4.0][..], &[41.0, 1.0][..])
    );
    assert!(runs > 10, "the budget split the run ({runs} runs)");
}

/// A published failure reaches the run failure boundary at the need, with
/// the need's source location, whether the failure was already published
/// (same run) or arrives while the task waits (resumed run).
#[test]
fn a_failed_dependency_fails_the_run_at_the_need() {
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let request = rt.claim(number_key).unwrap();
    let error = rt.root(Primitive::Number(13.0));
    let _ = rt
        .complete(number_key, request.producer(), Completion::Failed(&error))
        .unwrap();
    let mut host = Recorder::default();
    let e = failed(run(&consumer(&rt, 1.0), &mut host));
    assert_eq!(e.kind(), VmErrorKind::DependencyFailed(number_key));
    assert!(e.message().contains("failed with Number(13.0)"), "{e}");
    assert_eq!(line_column(&e), NUMBER_NEED_AT);
    assert_eq!(e.diagnostic().opcode, Some(clj_quickjs::Opcode::Need));
    assert_eq!(e.diagnostic().call_chain.len(), 1, "inside `lookup`");

    // Published while the consumer waits.
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let task = consumer(&rt, 1.0);
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    let request = one_request(&task, number_key);
    let error = rt.root(Primitive::Number(14.0));
    let wakeups = rt
        .complete(number_key, request.producer(), Completion::Failed(&error))
        .unwrap();
    let e = failed(run(&only_runnable(wakeups), &mut host));
    assert_eq!(e.kind(), VmErrorKind::DependencyFailed(number_key));
    assert_eq!(line_column(&e), NUMBER_NEED_AT);
    assert_eq!(task.status(), TaskStatus::Finished);
    assert_eq!(host.effects, [1.0]);
}

/// A cancelled cell fails the needing run (the task itself is not
/// cancelled); a cancelled consumer ends `Cancelled` and leaves the cell and
/// its producer to the other consumers.
#[test]
fn cell_and_consumer_cancellation_reach_the_run_boundary() {
    // The embedder cancels the cell while the consumer waits.
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let task = consumer(&rt, 1.0);
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    let _request = one_request(&task, number_key);
    let e = failed(run(
        &only_runnable(rt.cancel_cell(number_key).unwrap()),
        &mut host,
    ));
    assert_eq!(e.kind(), VmErrorKind::DependencyCancelled(number_key));
    assert_eq!(line_column(&e), NUMBER_NEED_AT);
    assert_eq!(task.status(), TaskStatus::Finished);

    // A parked consumer is cancelled; another consumer of the same key keeps
    // waiting and the producer request (claimed by the first) still settles
    // the cell for it.
    let rt = Runtime::new();
    let number_key = key(&rt, NUMBER);
    let (first, second) = (consumer(&rt, 1.0), consumer(&rt, 2.0));
    let mut host = Recorder::default();
    assert_eq!(waiting(run(&first, &mut host)), number_key);
    assert_eq!(waiting(run(&second, &mut host)), number_key);
    let request = one_request(&first, number_key);
    assert!(
        second.take_producer_requests().is_empty(),
        "one producer per cell"
    );
    let Cancel::Runnable(cancelled) = first.cancel() else {
        panic!("a parked consumer is handed back to run its cleanup");
    };
    assert!(matches!(run(&cancelled, &mut host), RunOutcome::Cancelled));
    assert_eq!(first.status(), TaskStatus::Cancelled);
    assert_eq!(first.dependency_log().waiting(), None);
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    assert_eq!(resumed.id(), second.id());
    assert_eq!(waiting(run(&resumed, &mut host)), key(&rt, CALLABLE));
    assert_eq!(host.reports, [27.0]);

    // Cancelled before its need (by its own pre-need effect): the need is
    // refused, so nothing is subscribed or claimed, and the run ends
    // `Cancelled`.
    struct CancelSelf(Option<Task>);
    impl Host for CancelSelf {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            let task = self.0.take().expect("one effect");
            assert!(matches!(task.cancel(), Cancel::Requested));
            Ok(Primitive::Number(0.0))
        }
    }
    let rt = Runtime::new();
    let task = consumer(&rt, 1.0);
    let outcome = task
        .run_with(Budget::UNLIMITED, &mut CancelSelf(Some(task.clone())))
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Cancelled), "{outcome:?}");
    assert_eq!(task.status(), TaskStatus::Cancelled);
    assert!(matches!(
        rt.cell_status(key(&rt, NUMBER)),
        CellStatus::Absent
    ));
    assert!(task.take_producer_requests().is_empty());
}

/// The world is execution context: the same source in another world needs
/// that world's key, and a result published in the first world does not
/// satisfy it.
#[test]
fn the_need_reads_the_task_world() {
    let rt = Runtime::new();
    let first = rt.world();
    let second = rt.new_world();
    let request = rt.claim(key(&rt, NUMBER)).unwrap();
    let _ = publish_number(&rt, &request, 7.0);
    let base = rt.root(Primitive::Number(1.0));
    let task = rt.spawn_in(second, &consumer_exe(), &[&base]).unwrap();
    let k = waiting(run(&task, &mut Recorder::default()));
    assert_eq!(k, DependencyKey::new(second, NUMBER.0, NUMBER.1));
    assert_ne!(k.world(), first);
}

/// `__rt.need` compiles to the `need` instruction; its operands are checked
/// at compile time when literal and at run time otherwise.
#[test]
fn need_operands_are_validated() {
    let exe = compile_script("t", "__rt.need(1, 2);").unwrap();
    // const 0, const 1, need, …: no operand byte of this program is 0x36.
    assert!(
        exe.to_spec()
            .code
            .contains(&(clj_quickjs::Opcode::Need as u8))
    );
    for (src, column) in [
        ("__rt.need(1.5, 2);", 11),
        ("__rt.need(1, 4294967296);", 14),
        ("__rt.need(9007199254740992, 1);", 11),
    ] {
        let e = compile_script("t", src).expect_err(src);
        assert_eq!(
            (e.kind, e.line, e.column),
            (SyntaxErrorKind::Invalid, 1, column),
            "{src}"
        );
    }
    let rt = Runtime::new();
    for src in [
        "__rt.need(0 - 1, 2);",
        "__rt.need(1, 0.5 + 0);",
        "__rt.need(true, 2);",
        "__rt.need(1, __rt.vector());",
    ] {
        let task = rt.spawn(&compile_script("t", src).unwrap(), &[]).unwrap();
        let e = failed(task.run().unwrap());
        assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand, "{src}");
        assert!(e.message().contains("non-negative integer"), "{e}");
        assert_eq!(line_column(&e), (1, 10), "{src}: at the `(`");
    }
    // Inside a loop, a ready need is read on every iteration, recorded once.
    let rt = Runtime::new();
    let request = rt
        .claim(DependencyKey::new(
            rt.world(),
            DependencyId::new(1),
            ResultKind::new(2),
        ))
        .unwrap();
    let _ = publish_number(&rt, &request, 5.0);
    let src =
        "let i = 0; let sum = 0; while (i < 3) { sum = sum + __rt.need(1, 2); i = i + 1; } sum;";
    let task = rt.spawn(&compile_script("t", src).unwrap(), &[]).unwrap();
    assert_eq!(number(task.run().unwrap()), 15.0);
    assert_eq!(task.dependency_log().edges().len(), 1);
}

/// A published persistent graph reaches consumers through `need` as the
/// very objects the producer published, not copies, whether the consumer
/// waited for it or read it ready, and stays alive through its cell alone
/// after the producer drops its handle and collections run.
#[test]
fn a_published_object_keeps_its_identity_through_need() {
    let rt = Runtime::new();
    let k = DependencyKey::new(rt.world(), DependencyId::new(9), ResultKind::new(1));
    let (graph, leaf) = rt.scope(|s| {
        let leaf = s
            .new_vector(&[s.primitive(Primitive::Number(1.0))])
            .unwrap();
        let outer = s
            .new_vector(&[leaf, s.primitive(Primitive::Number(2.0))])
            .unwrap();
        (s.persist(outer), s.downgrade(leaf))
    });
    // Returns the needed graph and its first element.
    let exe = compile_script(
        "t",
        "const g = __rt.need(9, 1); __rt.vector(g, __rt.nth(g, 0));",
    )
    .unwrap();
    let waiter = rt.spawn(&exe, &[]).unwrap();
    assert_eq!(waiting(waiter.run().unwrap()), k);
    let request = one_request(&waiter, k);
    let wakeups = rt
        .complete(k, request.producer(), Completion::Ready(&graph))
        .unwrap();
    let graph_weak = graph.downgrade();
    drop(graph);
    rt.collect().unwrap();
    assert!(graph_weak.is_live() && leaf.is_live(), "the cell keeps it");

    let reader = rt.spawn(&exe, &[]).unwrap();
    for task in [only_runnable(wakeups), reader] {
        let RunOutcome::Complete(result) = task.run().unwrap() else {
            panic!("the consumer returns the pair");
        };
        rt.collect().unwrap();
        rt.scope(|s| {
            let items = s.items(s.local(&result).unwrap()).unwrap();
            assert!(s.same_object(items[0], s.upgrade(&graph_weak).unwrap()));
            assert!(s.same_object(items[1], s.upgrade(&leaf).unwrap()));
        });
    }
}

/// A method call (clj-quickjs-117) keeps its receiver below the callee
/// while an argument waits: the receiver is a temporary object only the
/// suspended frame holds, it survives a collection during the wait, and
/// after publication the run on another thread passes the very object to
/// the built-in `valueOf`. A JavaScript method gets its argument the same
/// way (its receiver becomes the frame's `this`, clj-quickjs-116). Each
/// `make` ran once.
#[test]
fn method_calls_preserve_receiver_across_wait_and_collection() {
    let rt = Runtime::new();
    let exe = compile_script(
        "method_need",
        "function make(n) { __rt.effect(1, n); \
           return {x: 5, m: Object.prototype.valueOf, j: function (a) { return a * 2; }}; } \
         let r = make(1).m(__rt.need(5, 3)); \
         let j = make(2).j(__rt.need(5, 3)); \
         r.x * 100 + j",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Recorder::default();
    let number_key = key(&rt, NUMBER);
    assert_eq!(waiting(run(&task, &mut host)), number_key);
    assert_eq!(host.effects, [1.0]);
    let request = one_request(&task, number_key);
    // Only the suspended frame holds the receiver.
    rt.collect().unwrap();
    rt.collect().unwrap();
    let resumed = only_runnable(publish_number(&rt, &request, 7.0));
    let (outcome, host) = thread::spawn(move || {
        let outcome = run(&resumed, &mut host);
        (outcome, host)
    })
    .join()
    .unwrap();
    assert_eq!(number(outcome), 5.0 * 100.0 + 14.0);
    assert_eq!(host.effects, [1.0, 2.0], "each make ran once");
}

/// Effects of every worker of [`migrating_executor`]: (worker, argument),
/// in execution order.
type WorkerEffects = Arc<Mutex<Vec<(WorkerId, f64)>>>;

/// Logs host operation 1 with its worker and returns its argument.
struct WorkerLog {
    worker: WorkerId,
    effects: WorkerEffects,
}

impl Host for WorkerLog {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => {
                self.effects.lock().unwrap().push((self.worker, n));
                Ok(call.arg)
            }
            other => Err(format!("unexpected host effect {other:?}")),
        }
    }
}

/// The two-worker executor, with woken tasks resuming on the worker after
/// the one they parked on.
fn migrating_executor(rt: &Runtime, effects: &WorkerEffects) -> Executor {
    let effects = Arc::clone(effects);
    ExecutorConfig::new()
        .workers(2)
        .host(move |worker| {
            Box::new(WorkerLog {
                worker,
                effects: Arc::clone(&effects),
            })
        })
        .placement(|r| match (r.reason, r.last_worker) {
            (EnqueueReason::Woken, Some(last)) => {
                Placement::Worker(WorkerId::new((last.index() + 1) % r.workers))
            }
            (_, Some(last)) => Placement::Worker(last),
            _ => Placement::Any,
        })
        .start(rt)
        .unwrap()
}

/// Request a collection on `worker` and wait until it is recorded.
fn collect_on_worker(exec: &Executor, worker: WorkerId) {
    let collections = |ev: &[Event]| {
        ev.iter()
            .filter(|e| matches!(e.kind, EventKind::Collected(_)))
            .count()
    };
    let before = collections(&exec.events());
    exec.schedule_collection(Placement::Worker(worker));
    assert!(exec.wait_for(TIMEOUT, |ev| collections(ev) > before));
}

const TIMEOUT: Duration = Duration::from_secs(30);

/// A method's `this` (clj-quickjs-116) is a traced slot of its task-owned
/// frame: the method writes a property through `this`, suspends at
/// `__rt.need` on a pending cell, and is collected around on both workers
/// while only that slot holds the receiver (a temporary object). After
/// publication the task resumes on the other worker and reads `this.x` and
/// the property it wrote from the same object. Each effect ran once.
#[test]
fn a_method_resumes_with_the_same_this_after_a_wait_and_migration() {
    let rt = Runtime::new();
    let effects = WorkerEffects::default();
    let exec = migrating_executor(&rt, &effects);
    let exe = compile_script(
        "this_need",
        "function make() {
           return {x: 5, m: function (a) {
             this.y = __rt.effect(1, a);
             let v = __rt.need(5, 3);
             return this.x * 1000 + this.y * 10 + v + __rt.effect(1, 2);
           }};
         }
         make().m(1)",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, NUMBER));
    assert!(exec.wait_for(TIMEOUT, |ev| {
        ev.iter()
            .any(|e| e.task == Some(task.id()) && e.kind == EventKind::Released(parked))
    }));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    assert_eq!(task.status(), TaskStatus::Waiting);

    let request = one_request(&task, key(&rt, NUMBER));
    assert_eq!(exec.wake(publish_number(&rt, &request, 7.0)).unwrap(), 1);
    let outcome = exec.take_outcome(task.id(), TIMEOUT).expect("an outcome");
    assert_eq!(number(outcome), 5.0 * 1000.0 + 10.0 + 7.0 + 2.0);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    assert_eq!(*effects.lock().unwrap(), [(w0, 1.0), (w1, 2.0)]);
    exec.shutdown(TIMEOUT).unwrap();
}

/// A construct frame (clj-quickjs-71) is task-owned state like any frame:
/// the constructor writes through `this` (the new object, held only by the
/// frame's traced `this` slot), suspends at `__rt.need` on a pending cell,
/// and is collected around on both workers. After publication it resumes
/// on the other worker, finishes, and its non-object `return 5` is
/// replaced by the same new object, which still inherits from
/// `F.prototype`. Each effect ran once.
#[test]
fn a_constructor_resumes_with_its_new_object_after_a_wait_and_migration() {
    let rt = Runtime::new();
    let effects = WorkerEffects::default();
    let exec = migrating_executor(&rt, &effects);
    let exe = compile_script(
        "construct_need",
        "function F(a) {
           this.y = __rt.effect(1, a);
           let v = __rt.need(5, 3);
           this.v = v + __rt.effect(1, 2);
           return 5;
         }
         F.prototype.get = function () { return this.y * 100 + this.v; };
         const o = new F(1);
         o.constructor === F ? o.get() : -1",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    exec.submit_with(task.clone(), Placement::Worker(WorkerId::new(0)))
        .unwrap();
    let parked = OutcomeKind::Waiting(key(&rt, NUMBER));
    assert!(exec.wait_for(TIMEOUT, |ev| {
        ev.iter()
            .any(|e| e.task == Some(task.id()) && e.kind == EventKind::Released(parked))
    }));
    collect_on_worker(&exec, WorkerId::new(0));
    collect_on_worker(&exec, WorkerId::new(1));
    assert_eq!(task.status(), TaskStatus::Waiting);

    let request = one_request(&task, key(&rt, NUMBER));
    assert_eq!(exec.wake(publish_number(&rt, &request, 7.0)).unwrap(), 1);
    let outcome = exec.take_outcome(task.id(), TIMEOUT).expect("an outcome");
    assert_eq!(number(outcome), 100.0 + 7.0 + 2.0);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    assert_eq!(*effects.lock().unwrap(), [(w0, 1.0), (w1, 2.0)]);
    exec.shutdown(TIMEOUT).unwrap();
}
