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

use std::thread;

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

/// The fixture's line of `__rt.need(5, 3)` and the column of `__rt`.
const NUMBER_NEED_AT: (u32, u32) = (12, 25);

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
        assert_eq!(line_column(&e), (1, 1), "{src}");
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
