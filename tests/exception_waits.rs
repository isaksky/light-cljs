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
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => {
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

/// Synthetic fixture, not light-cljs output. The disposal activation and its
/// remaining resources stay rooted while the callback waits and migrates.
#[test]
fn async_disposable_stack_callback_waits_and_resumes() {
    let rt = Runtime::new();
    // Realm construction runs on the host; VM work runs on small workers.
    spawn(&rt, "Object").run().unwrap();
    let task = spawn(
        &rt,
        r#"
        let s = new AsyncDisposableStack(), settled = 0;
        s.adopt({n: 1}, async v => { await null; __rt.effect(1, v.n); });
        s.defer(async () => {
            __rt.effect(1, 2); __rt.need(5, 3);
            await null; __rt.effect(1, 3);
        });
        s.disposeAsync().then(v => {
            settled++; __rt.effect(1, v === undefined && settled === 1 ? 4 : 99);
        });
        42;
    "#,
    );
    let run_small = |task: Task, mut log: Log| {
        thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let outcome = run(&task, &mut log);
                (task, outcome, log)
            })
            .unwrap()
            .join()
            .unwrap()
    };
    let (task, outcome, log) = run_small(task, Log::default());
    assert_eq!(waiting(outcome), key(&rt, A));
    assert_eq!(task.status(), TaskStatus::Waiting);
    assert_eq!(log.0, [2.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    rt.collect().unwrap();
    let (task, outcome, log) = run_small(task, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [2.0, 3.0, 1.0, 4.0]);
    assert!(task.suspended_async_activations().is_empty());
}

/// Synthetic fixture, not light-cljs output. An earlier anonymous error and
/// remaining resource are held only by the detached disposal continuation.
#[test]
fn async_disposable_stack_detached_disposal_survives_collection() {
    let rt = Runtime::new();
    spawn(&rt, "Object").run().unwrap();
    let mut task = spawn(
        &rt,
        r#"
        let s = new AsyncDisposableStack(), settled = 0;
        s.adopt({tag: 'remaining'}, async v => {
            await null; __rt.effect(1, v.tag === 'remaining' ? 1 : 99);
            throw {tag: 'last'};
        });
        s.defer(async () => {
            await null; __rt.effect(1, 2); __rt.need(5, 3);
            await null; __rt.effect(1, 3); throw {tag: 'middle'};
        });
        s.defer(() => { throw {tag: 'first'}; });
        s.disposeAsync().then(() => __rt.effect(1, 99), e => {
            settled++;
            let ok = e instanceof SuppressedError && e.error.tag === 'last' &&
                e.suppressed instanceof SuppressedError &&
                e.suppressed.error.tag === 'middle' &&
                e.suppressed.suppressed.tag === 'first';
            __rt.effect(1, ok && settled === 1 ? 4 : 99);
        });
        s = null;
        __rt.effect(1, 0);
        42;
    "#,
    );
    let mut log = Log::default();
    let mut waited = false;
    for _ in 0..5000 {
        let (next_task, next_log, outcome) = thread::Builder::new()
            .stack_size(64 * 1024)
            .spawn(move || {
                let outcome = task.run_with(Budget::instructions(5), &mut log).unwrap();
                (task, log, outcome)
            })
            .unwrap()
            .join()
            .unwrap();
        task = next_task;
        log = next_log;
        rt.collect().unwrap();
        match outcome {
            RunOutcome::Yielded => {}
            RunOutcome::Waiting(dependency) => {
                assert!(!waited, "disposal callback was replayed");
                assert_eq!(dependency, key(&rt, A));
                // The entry's effect precedes the callback job: disposeAsync
                // returned at its first await. The disposal continuation is
                // now detached again, awaiting this callback's pending promise.
                assert_eq!(log.0, [0.0, 2.0]);
                waited = true;
                task = publish(&rt, &task, 0.0);
                rt.collect().unwrap();
            }
            outcome @ RunOutcome::Complete(_) => {
                assert!(waited);
                assert_eq!(number(outcome), 42.0);
                assert_eq!(log.0, [0.0, 2.0, 3.0, 1.0, 4.0]);
                assert!(task.suspended_async_activations().is_empty());
                return;
            }
            other => panic!("unexpected outcome: {other:?}"),
        }
    }
    panic!("async disposal did not finish within the instruction bound");
}

/// Synthetic fixture, not light-cljs output. The pending return is an object
/// rooted only by the finally completion while a disposer waits.
#[test]
fn a_wait_inside_a_dispose_method_resumes_disposal() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        function f() {
            using a = {[Symbol.dispose]() { __rt.effect(1, 1); }};
            using b = {[Symbol.dispose]() { __rt.effect(1, 2); __rt.need(5, 3); __rt.effect(1, 3); }};
            return {answer: 42};
        }
        f().answer
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [2.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [2.0, 3.0, 1.0]);
}

#[test]
fn disposable_stack_callback_waits_and_resumes() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let s = new DisposableStack();
        s.adopt({n: 1}, v => __rt.effect(1, v.n));
        s.defer(() => { __rt.effect(1, 2); __rt.need(5, 3); __rt.effect(1, 3); });
        s.dispose();
        s.dispose();
        s.disposed ? 42 : 0;
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [2.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [2.0, 3.0, 1.0]);
}

#[test]
fn disposable_stack_use_getter_waits_and_resumes() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let s = new DisposableStack();
        s.use({n: 42, get [Symbol.dispose]() {
            __rt.effect(1, 1); __rt.need(5, 3);
            return function() { __rt.effect(1, this.n); };
        }});
        s.dispose(); 42;
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [1.0, 42.0]);
}

/// Synthetic fixtures, not light-cljs output. The temporary receiver has
/// no source binding; anonymous errors have no holder outside the native
/// catch/disposal state until the final catch receives the complete chain.
#[test]
fn disposable_stack_continuation_roots_survive_collection() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        new DisposableStack().use({n: 42, get [Symbol.dispose]() {
            __rt.effect(1, 1); __rt.need(5, 3);
            return function() {};
        }}).n;
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [1.0]);

    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let s = new DisposableStack(), caught;
        s.defer(() => { __rt.effect(1, 1); throw {tag: 'first'}; });
        s.defer(() => { __rt.effect(1, 2); throw {tag: 'second'}; });
        s.defer(() => { __rt.effect(1, 3); throw {tag: 'third'}; });
        try { s.dispose(); } catch(e) { caught = e; }
        caught instanceof SuppressedError && caught.error.tag === 'first' &&
        caught.suppressed instanceof SuppressedError &&
        caught.suppressed.error.tag === 'second' &&
        caught.suppressed.suppressed.tag === 'third' ? 42 : 0;
    "#,
    );
    let mut log = Log::default();
    for _ in 0..10_000 {
        match task.run_with(Budget::instructions(1), &mut log).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            outcome @ RunOutcome::Complete(_) => {
                assert_eq!(number(outcome), 42.0);
                assert_eq!(log.0, [3.0, 2.0, 1.0]);
                return;
            }
            other => panic!("expected yield or completion, got {other:?}"),
        }
    }
    panic!("disposal did not finish within the instruction bound");
}

#[test]
fn cancellation_ends_await_using_at_cleanup_await() {
    // Synthetic fixture, not light-cljs output. An await during cancellation
    // ends this cleanup and unwinds outer scopes without reviving the task.
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        try {
            using outer = {[Symbol.dispose]() { __rt.effect(1, 1); }};
            (async () => {
                await using a = {[Symbol.asyncDispose]() { __rt.effect(1, 98); }},
                    b = {[Symbol.asyncDispose]() { __rt.effect(1, 2); }};
                __rt.need(5, 3);
            })();
        } catch(e) { __rt.effect(1, 99); }
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    let task = cancel_parked(&task);
    let (outcome, log) = run_elsewhere(task, log);
    cancelled(outcome);
    assert_eq!(log.0, [2.0, 1.0]);
}

#[test]
fn await_using_nullish_records_cancel_at_first_cleanup_await() {
    // Synthetic fixtures, not light-cljs output. Nullish records defer the
    // cancellation boundary to a real async call, or before a sync call.
    for (declarations, expected) in [
        (
            "await using a = {[Symbol.asyncDispose]() { __rt.effect(1, 2); }}, b = null, c = undefined;",
            vec![2.0, 1.0],
        ),
        (
            "using a = {[Symbol.dispose]() { __rt.effect(1, 2); }}; await using b = null, c = undefined;",
            vec![1.0],
        ),
    ] {
        let rt = Runtime::new();
        let source = format!(
            "try {{
                using outer = {{[Symbol.dispose]() {{ __rt.effect(1, 1); }}}};
                (async () => {{ {declarations} __rt.need(5, 3); }})();
            }} catch(e) {{ __rt.effect(1, 99); }}
            __rt.effect(1, 98);"
        );
        let task = spawn(&rt, &source);
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        assert!(log.0.is_empty());
        rt.collect().unwrap();
        let task = cancel_parked(&task);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(task, log);
        cancelled(outcome);
        assert_eq!(log.0, expected, "{source}");
    }
}

#[test]
fn cancellation_disposes_using_resources() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        try {
            using a = {[Symbol.dispose]() { __rt.effect(1, 1); }};
            using b = {[Symbol.dispose]() { __rt.effect(1, 2); throw Error('dispose'); }};
            using c = {[Symbol.dispose]() { __rt.effect(1, 3); }};
            __rt.need(5, 3);
        } catch(e) { __rt.effect(1, 99); }
        __rt.effect(1, 98);
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    let task = cancel_parked(&task);
    let (outcome, log) = run_elsewhere(task, log);
    cancelled(outcome);
    assert_eq!(log.0, [3.0, 2.0, 1.0]);
}

#[test]
fn cancellation_disposes_nested_scopes_after_disposal_errors() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        function f() {
            using outer = {[Symbol.dispose]() { __rt.effect(1, 1); throw 1; }};
            {
                using inner = {[Symbol.dispose]() { __rt.effect(1, 2); throw 2; }};
                __rt.need(5, 3);
            }
        }
        try { using caller = {[Symbol.dispose]() { __rt.effect(1, 0); }}; f(); }
        catch(e) { __rt.effect(1, 99); }
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    let task = cancel_parked(&task);
    let (outcome, log) = run_elsewhere(task, log);
    cancelled(outcome);
    assert_eq!(log.0, [2.0, 1.0, 0.0]);
}

#[test]
fn cancellation_during_dispose_getter_disposes_only_prior_resources() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        try {
            using a = {n: 1, [Symbol.dispose]() { __rt.effect(1, this.n); }};
            using b = {n: 2, [Symbol.dispose]() { __rt.effect(1, this.n); }};
            using c = {get [Symbol.dispose]() {
                __rt.need(5, 3);
                return function() { __rt.effect(1, 99); };
            }};
            using d = {[Symbol.dispose]() { __rt.effect(1, 98); }};
            __rt.effect(1, 97);
        } catch(e) { __rt.effect(1, 96); }
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    assert!(log.0.is_empty());
    rt.collect().unwrap();
    let task = cancel_parked(&task);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(task, log);
    cancelled(outcome);
    assert_eq!(log.0, [2.0, 1.0]);
}

#[test]
fn using_dispose_getter_waits_before_binding_initialization() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let caught = false, reads = 0;
        {
            using x = {get [Symbol.dispose]() {
                reads++; __rt.need(5, 3);
                try { eval('x'); } catch(e) { caught = e instanceof ReferenceError; }
                return function() { __rt.effect(1, this.answer); };
            }, answer: 7};
        }
        caught && reads === 1 ? 42 : 0
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [7.0]);
}

#[test]
fn using_disposal_errors_remain_rooted_across_waits() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let original = {tag: 'body'}, caught;
        function resource(tag, dependency) {
            return {[Symbol.dispose]() { __rt.effect(1, dependency); __rt.need(dependency, 3); throw {tag: tag}; }};
        }
        try { using a = resource('a', 5), b = resource('b', 6), c = resource('c', 7); throw original; }
        catch(e) { caught = e; }
        caught.error.tag === 'a' && caught.suppressed.error.tag === 'b' &&
        caught.suppressed.suppressed.error.tag === 'c' &&
        caught.suppressed.suppressed.suppressed === original ? 42 : 0
    "#,
    );
    let mut current = task;
    let mut log = Log::default();
    for dependency in [7, 6, 5] {
        let (outcome, next_log) = run_elsewhere(current.clone(), log);
        log = next_log;
        assert_eq!(
            waiting(outcome),
            key(&rt, (DependencyId::new(dependency), ResultKind::new(3)))
        );
        rt.collect().unwrap();
        current = publish(&rt, &current, 0.0);
        rt.collect().unwrap();
    }
    let (outcome, log) = run_elsewhere(current, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, [7.0, 6.0, 5.0]);
}

/// Source line and column of a failure.
fn line_column(e: &VmError) -> (u32, u32) {
    let at = e.diagnostic().source.expect("source location");
    (at.line, at.column)
}

/// Synthetic fixture, not light-cljs output. Every protocol callback parks;
/// all retained iterator state must survive collection and thread migration.
#[test]
fn for_using_of_waits_dispose_before_next_and_close() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let n = 0;
        let it = {
            next() {
                n++; __rt.effect(1, 10 + n); __rt.need(10 + n, 3);
                return {value: {n: n, get [Symbol.dispose]() {
                    __rt.effect(1, 20 + this.n); __rt.need(20 + this.n, 3);
                    return function() { __rt.effect(1, 40 + this.n); __rt.need(40 + this.n, 3); };
                }}, done: false};
            },
            return() { __rt.effect(1, 50); __rt.need(50, 3); return {}; }
        };
        it[Symbol.iterator] = function() { return this; };
        let fs = [];
        for (using x of it) {
            fs.push(() => x.n);
            __rt.effect(1, 30 + x.n); __rt.need(30 + x.n, 3);
            if (x.n === 2) break;
        }
        fs[0]() + fs[1]() === 3 ? 42 : 0
        "#,
    );
    let dependencies = [11, 21, 31, 41, 12, 22, 32, 42, 50];
    let mut log = Log::default();
    let mut current = task;
    for (index, dependency) in dependencies.into_iter().enumerate() {
        let (outcome, next_log) = run_elsewhere(current.clone(), log);
        log = next_log;
        assert_eq!(
            waiting(outcome),
            key(&rt, (DependencyId::new(dependency), ResultKind::new(3)))
        );
        assert_eq!(log.0.len(), index + 1);
        rt.collect().unwrap();
        current = publish(&rt, &current, 0.0);
        rt.collect().unwrap();
    }
    let (outcome, log) = run_elsewhere(current, log);
    assert_eq!(number(outcome), 42.0);
    assert_eq!(log.0, dependencies.map(|dependency| dependency as f64));
}

#[test]
fn for_using_of_cancellation_disposes_before_close() {
    for cleanup in ["", "throw 42;"] {
        let rt = Runtime::new();
        let task = spawn(
            &rt,
            &format!(
                r#"
                let it = {{
                    next() {{ return {{value: {{[Symbol.dispose]() {{ __rt.effect(1, 2); {cleanup} }}}}, done: false}}; }},
                    return() {{ __rt.effect(1, 3); return {{}}; }}
                }};
                it[Symbol.iterator] = function() {{ return this; }};
                try {{ for (using x of it) {{ try {{ __rt.need(5, 3); }} finally {{ __rt.effect(1, 1); }} }} }}
                catch(e) {{ __rt.effect(1, 99); }}
                "#
            ),
        );
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(cancel_parked(&task), log);
        cancelled(outcome);
        assert_eq!(log.0, [1.0, 2.0, 3.0]);
    }
}

#[test]
fn for_of_waits_resume_iteration_and_close() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let sum = 0, n = 0, closes = 0;
        let iterable = {}; iterable[Symbol.iterator] = function() { return {
            next() { n++; __rt.effect(1, 10 + n); __rt.need(10 + n, 3); return { value: n, done: false }; },
            return() { closes++; __rt.effect(1, 30); __rt.need(30, 3); return {}; }
        }; };
        for (const x of iterable) {
            __rt.effect(1, 20 + x); sum += x + __rt.need(20 + x, 3);
            if (x === 2) break;
        }
        sum + closes * 100
    "#,
    );
    let mut log = Log::default();
    let mut current = task;
    for (index, dependency) in [11, 21, 12, 22, 30].into_iter().enumerate() {
        let (outcome, next_log) = run_elsewhere(current.clone(), log);
        log = next_log;
        assert_eq!(
            waiting(outcome),
            key(&rt, (DependencyId::new(dependency), ResultKind::new(3)))
        );
        assert_eq!(log.0.len(), index + 1);
        rt.collect().unwrap();
        current = publish(&rt, &current, 0.0);
        rt.collect().unwrap();
    }
    let (outcome, log) = run_elsewhere(current, log);
    assert_eq!(number(outcome), 103.0);
    assert_eq!(log.0, [11.0, 21.0, 12.0, 22.0, 30.0]);
}

#[test]
fn for_of_cancellation_closes_body_and_pending_next() {
    for (wait_in_next, cleanup) in [
        (false, "return {};"),
        (true, "return {};"),
        (false, "throw 42;"),
        (false, "return 42;"),
    ] {
        let rt = Runtime::new();
        let task = spawn(
            &rt,
            &format!(
                r#"
            let it = {{
                next() {{ {} return {{value: 1, done: false}}; }},
                return() {{ __rt.effect(1, 2); {} }}
            }};
            it[Symbol.iterator] = function() {{ return this; }};
            try {{ for (let x of it) {{ try {{ __rt.need(5, 3); }} finally {{ __rt.effect(1, 1); }} }} }}
            catch (e) {{ __rt.effect(1, 99); }}
            __rt.effect(1, 98);
        "#,
                if wait_in_next { "__rt.need(5, 3);" } else { "" },
                cleanup
            ),
        );
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        rt.collect().unwrap();
        let resumed = cancel_parked(&task);
        let (outcome, log) = run_elsewhere(resumed, log);
        cancelled(outcome);
        assert_eq!(
            log.0,
            if wait_in_next {
                vec![2.0]
            } else {
                vec![1.0, 2.0]
            }
        );
    }
}

#[test]
fn for_of_builtin_iterator_getters_survive_waits() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let sum = 0;
        for (const x of Array.prototype.values.call({
            get length() { return __rt.need(5, 3); },
            get 0() { return __rt.need(6, 3); }
        })) sum += x;
        sum
    "#,
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 1.0);
    let (outcome, log) = run_elsewhere(task.clone(), log);
    assert_eq!(waiting(outcome), key(&rt, B));
    rt.collect().unwrap();
    let task = publish(&rt, &task, 42.0);
    let (outcome, _) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 42.0);
}

#[test]
fn for_of_builtin_cursor_survives_body_wait() {
    let rt = Runtime::new();
    // Once next returns, only the cursor's internal slot holds this array.
    let task = spawn(
        &rt,
        "let sum = 0; for (const x of [1, 2]) { sum += x; __rt.need(5, 3); } sum",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 0.0);
    let (outcome, _) = run_elsewhere(task, log);
    assert_eq!(number(outcome), 3.0);
}

#[test]
fn for_of_get_iterator_waits_resume_once() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let iterable = {}, n = 0;
        Object.defineProperty(iterable, Symbol.iterator, { get() {
            __rt.effect(1, 1); __rt.need(5, 3);
            return function() {
                __rt.effect(1, 2); __rt.need(6, 3);
                return { get next() {
                    __rt.effect(1, 3); __rt.need(7, 3);
                    return function() { n++; return {value: n, done: n > 2}; };
                }};
            };
        }});
        let sum = 0; for (const x of iterable) sum += x; sum
    "#,
    );
    let mut current = task;
    let mut log = Log::default();
    for (index, dependency) in [5, 6, 7].into_iter().enumerate() {
        let (outcome, next_log) = run_elsewhere(current.clone(), log);
        log = next_log;
        assert_eq!(
            waiting(outcome),
            key(&rt, (DependencyId::new(dependency), ResultKind::new(3)))
        );
        assert_eq!(log.0.len(), index + 1);
        rt.collect().unwrap();
        current = publish(&rt, &current, 0.0);
    }
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(current, log);
    assert_eq!(number(outcome), 3.0);
    assert_eq!(log.0, [1.0, 2.0, 3.0]);
}

/// A `do`-`while` loop (clj-quickjs-80) parks in its test after a
/// `continue` through a finally block, then in its body: each time a
/// collection runs while it is parked and it resumes on another thread
/// exactly where it stopped, with every effect run once.
#[test]
fn do_while_waits_resume_body_and_test() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let n = 0;
         do {
           n = n + 1;
           try { __rt.effect(1, n); if (n === 1) continue; __rt.effect(1, __rt.need(5, 3)); }
           finally { __rt.effect(1, 10 + n); }
         } while (__rt.need(6, 3) && n < 2);
         n",
    );
    let mut log = Log::default();
    // Iteration 1: `continue` runs the finally block, then the test waits.
    run_to_wait(&rt, &task, &mut log, B);
    assert_eq!(log.0, [1.0, 11.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 1.0);
    // Iteration 2 waits in the body.
    let (outcome, log) = run_elsewhere(task.clone(), log);
    assert_eq!(waiting(outcome), key(&rt, A));
    assert_eq!(log.0, [1.0, 11.0, 2.0]);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
    // The test reads the ready result of `need(6, 3)` and ends the loop.
    assert_eq!(number(outcome), 2.0);
    assert_eq!(log.0, [1.0, 11.0, 2.0, 7.0, 12.0]);
}

/// A `for` loop with a `let` head (clj-quickjs-81) parks in its update
/// after a `continue` through a finally block, then in its body, with
/// closures over two iterations' copies of the head binding live: each time
/// a collection runs while it is parked and it resumes on another thread
/// exactly where it stopped, with every effect run once and each closure
/// still seeing its own iteration's binding.
#[test]
fn for_waits_resume_body_and_update_with_per_iteration_bindings() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "const b = __rt.vectorBuilder();
         for (let i = 0; i < 2; i = i + __rt.need(6, 3)) {
           __rt.builderConj(b, () => i);
           try { __rt.effect(1, i); if (i === 0) continue; __rt.effect(1, __rt.need(5, 3)); }
           finally { __rt.effect(1, 10 + i); }
         }
         const v = __rt.freeze(b);
         __rt.count(v) * 100 + __rt.nth(v, 0)() * 10 + __rt.nth(v, 1)()",
    );
    let mut log = Log::default();
    // Iteration 1: `continue` runs the finally block, then the update waits.
    run_to_wait(&rt, &task, &mut log, B);
    assert_eq!(log.0, [0.0, 10.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 1.0);
    // Iteration 2 (i = 1) waits in the body.
    let (outcome, log) = run_elsewhere(task.clone(), log);
    assert_eq!(waiting(outcome), key(&rt, A));
    assert_eq!(log.0, [0.0, 10.0, 1.0]);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
    // The update reads the ready result of `need(6, 3)` and the test ends
    // the loop; the closures saw 0 and 1, not the final 2.
    assert_eq!(number(outcome), 201.0);
    assert_eq!(log.0, [0.0, 10.0, 1.0, 7.0, 11.0]);
}

/// Labelled jumps (clj-quickjs-82) through finally blocks: a `continue` to
/// a labelled `do`-`while` parks in its test, then a `break` out of a
/// labelled block parks in the finally block it runs on the way. Each time
/// a collection runs while it is parked and it resumes on another thread
/// exactly where it stopped, completing the pending jump, with every effect
/// run once.
#[test]
fn labelled_jumps_wait_and_resume_after_migration() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let n = 0;
         outer: do {
           n = n + 1;
           L: {
             try { if (n === 1) continue outer; break L; }
             finally { __rt.effect(1, 10 + n); if (n === 2) __rt.effect(1, __rt.need(5, 3)); }
             __rt.effect(1, 91);
           }
           __rt.effect(1, n);
           if (n === 2) break outer;
           __rt.effect(1, 92);
         } while (__rt.need(6, 3));
         n",
    );
    let mut log = Log::default();
    // Iteration 1: `continue outer` runs the finally block; the test waits.
    run_to_wait(&rt, &task, &mut log, B);
    assert_eq!(log.0, [11.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 1.0);
    // Iteration 2: `break L` runs the finally block, which waits.
    let (outcome, log) = run_elsewhere(task.clone(), log);
    assert_eq!(waiting(outcome), key(&rt, A));
    assert_eq!(log.0, [11.0, 12.0]);
    rt.collect().unwrap();
    // The jump completes past `L`, then `break outer` ends the loop.
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
    assert_eq!(number(outcome), 2.0);
    assert_eq!(log.0, [11.0, 12.0, 7.0, 2.0]);
}

/// `switch` (clj-quickjs-83) state across waits: the discriminant (a
/// function object, kept in the switch's scratch local) is evaluated once,
/// a `case` test parks in a `need`, then a clause body parks while a case
/// binding lives in its binding cell (control fell through past its
/// declaration) and a closure in an earlier clause captures it. Each time a
/// collection runs while it is parked and it resumes on another thread
/// exactly where it stopped: no test or effect is replayed, the selection
/// holds, and the closure sees the assignment made after the wait.
#[test]
fn switch_waits_survive_migration_and_collection() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let f = 0, r = 0, d = 0;
         const k = () => 40;
         switch ((d = d + 1, k)) {
           case __rt.effect(1, 1): r = 99; break;
           case (__rt.need(5, 3), k):
             __rt.effect(1, 2);
             let y = __rt.effect(1, 3);
             f = () => y;
           case 0:
             y = y + __rt.need(6, 3);
             __rt.effect(1, y);
             break;
           default: r = 98;
         }
         r * 10000 + d * 1000 + f() * 100 + k()",
    );
    let mut log = Log::default();
    // The second test waits.
    run_to_wait(&rt, &task, &mut log, A);
    assert_eq!(log.0, [1.0]);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 7.0);
    // It matches; the clause body runs into the next clause, which waits.
    let (outcome, log) = run_elsewhere(task.clone(), log);
    assert_eq!(waiting(outcome), key(&rt, B));
    assert_eq!(log.0, [1.0, 2.0, 3.0]);
    rt.collect().unwrap();
    let (outcome, log) = run_elsewhere(publish(&rt, &task, 4.0), log);
    assert_eq!(number(outcome), 1740.0);
    assert_eq!(log.0, [1.0, 2.0, 3.0, 7.0]);
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

/// Synthetic fixtures, not light-cljs output. A pending object completion
/// survives detached and attached collections, then a dependency wait in
/// the resumed finally block. Effects must run once across both migrations.
#[test]
fn generator_abrupt_resume_cleanup_waits() {
    // Synthetic fixture, not light-cljs output. The only holder of {n:17}
    // is the pending completion, detached at B and attached at A/disposal.
    for method in ["return", "throw"] {
        let rt = Runtime::new();
        let source = format!(
            r#"
            function* f() {{
                using resource = {{[Symbol.dispose]() {{
                    __rt.effect(1, 40); __rt.effect(1, __rt.need(7, 3)); __rt.effect(1, 50);
                }}}};
                try {{ yield 1; throw 'unreachable'; }}
                finally {{
                    __rt.effect(1, 10); yield 2;
                    __rt.effect(1, 20); __rt.effect(1, __rt.need(5, 3)); __rt.effect(1, 30);
                }}
            }}
            let g = f();
            g.next();
            function deep() {{ return deeper(); }}
            function deeper() {{ return g.{method}({{n:17}}); }}
            let a = deep(); if (a.done || a.value !== 2) throw 'cleanup yield';
            __rt.need(6, 3);
            let result;
            try {{ result = g.next().value; }} catch(e) {{ result = e; }}
            if (!g.next().done) throw 'completed';
            __rt.effect(1, 60); result.n
        "#
        );
        let task = spawn(&rt, &source);
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, B);
        assert_eq!(log.0, [10.0]);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(publish(&rt, &task, 0.0), log);
        assert_eq!(waiting(outcome), key(&rt, A));
        assert_eq!(log.0, [10.0, 20.0]);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
        assert_eq!(
            waiting(outcome),
            key(&rt, (DependencyId::new(7), ResultKind::new(3)))
        );
        assert_eq!(log.0, [10.0, 20.0, 7.0, 30.0, 40.0]);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(publish(&rt, &task, 8.0), log);
        assert_eq!(number(outcome), 17.0, "{method}");
        assert_eq!(log.0, [10.0, 20.0, 7.0, 30.0, 40.0, 8.0, 50.0, 60.0]);
    }
}

#[test]
fn generator_abrupt_delegate_protocol_waits() {
    // Synthetic fixtures, not light-cljs output. Each getter/call can park
    // below the generator boundary; neither the method nor effects replay.
    for method in ["return", "throw", "missing-throw"] {
        let max_stage = if method == "missing-throw" { 2 } else { 4 };
        for stage in 1..=max_stage {
            let rt = Runtime::new();
            let property = if method == "missing-throw" {
                "return"
            } else {
                method
            };
            let source = format!(
                r#"
                function pause(at) {{ __rt.effect(1, at); if (at === {stage}) __rt.need(5, 3); }}
                let token = {{n:17}}, it = {{next() {{ return {{done:false, value:1}}; }},
                    get {property}() {{ pause(1); return function(v) {{
                        pause(2);
                        if (this !== it || arguments.length !== {arity}) throw 'method call';
                        return {{get done() {{ pause(3); return true; }},
                            get value() {{ pause(4); return v; }}}};
                    }}; }}
                }};
                it[Symbol.iterator] = function() {{ return this; }};
                function* f() {{ return yield* it; }}
                let g = f(); g.next(); let answer;
                try {{ answer = g.{call}(token).value; }}
                catch(e) {{ answer = {caught}; }}
                answer === token ? 17 : 0
            "#,
                arity = if method == "missing-throw" { 0 } else { 1 },
                call = if method == "return" {
                    "return"
                } else {
                    "throw"
                },
                caught = if method == "missing-throw" {
                    "e instanceof TypeError ? token : e"
                } else {
                    "e"
                },
            );
            let task = spawn(&rt, &source);
            let mut log = Log::default();
            run_to_wait(&rt, &task, &mut log, A);
            assert_eq!(log.0, (1..=stage).map(f64::from).collect::<Vec<_>>());
            rt.collect().unwrap();
            let (outcome, log) = run_elsewhere(publish(&rt, &task, 0.0), log);
            assert_eq!(number(outcome), 17.0, "{method} stage {stage}");
            assert_eq!(log.0, (1..=max_stage).map(f64::from).collect::<Vec<_>>());
        }
    }
}

#[test]
fn generator_finally_waits_after_resume() {
    for (completion, checks) in [
        (
            "return {n: 17};",
            "let last = first(g); if (!last.done) throw 'not done'; result = last.value;",
        ),
        (
            "throw {n: 17};",
            "try { first(g); } catch(e) { result = e; }",
        ),
    ] {
        for (first, second) in [("one", "three"), ("three", "one")] {
            let rt = Runtime::new();
            let source = format!(
                "function keep(padding, value) {{ return value; }}
                 function one(g) {{ return g.next(); }}
                 function three(g) {{ let pad = 11; return keep(pad, two(g)); }}
                 function two(g) {{ return keep(12, one(g)); }}
                 let first = {first}, second = {second};
                 function* f() {{
                   try {{ yield 1; {completion} }}
                   finally {{
                     __rt.effect(1, 10);
                     yield 2;
                     __rt.effect(1, 20);
                     __rt.effect(1, __rt.need(5, 3));
                     __rt.effect(1, 30);
                   }}
                 }}
                 let g = f();
                 let a = first(g), b = second(g);
                 if (a.value !== 1 || a.done || b.value !== 2 || b.done) throw 'yield rows';
                 __rt.need(6, 3);
                 let result;
                 try {{ {checks} }} finally {{ __rt.effect(1, 40); }}
                 let end = second(g);
                 if (!end.done || end.value !== undefined) throw 'completed next';
                 result.n"
            );
            let task = spawn(&rt, &source);
            let mut log = Log::default();
            run_to_wait(&rt, &task, &mut log, B);
            assert_eq!(log.0, [10.0]);
            // The generator is detached with its pending completion as the
            // only holder of {n:17}. No finally tail has run yet.
            rt.collect().unwrap();
            assert_eq!(rt.heap_usage().objects.generator, 1);
            let (outcome, log) = run_elsewhere(publish(&rt, &task, 0.0), log);
            assert_eq!(waiting(outcome), key(&rt, A));
            assert_eq!(log.0, [10.0, 20.0]);
            // Now attached above a different caller base, with the pending
            // return/throw still rooted while need parks the whole task.
            rt.collect().unwrap();
            let (outcome, log) = run_elsewhere(publish(&rt, &task, 7.0), log);
            assert_eq!(number(outcome), 17.0, "{completion}: {first}");
            assert_eq!(log.0, [10.0, 20.0, 7.0, 30.0, 40.0]);
        }
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
    assert_eq!(line_column(&error), (3, 52), "{error}");
    assert_eq!(error.diagnostic().call_chain.len(), 1, "{error}");
    let caller = error.diagnostic().call_chain[0].source.unwrap();
    assert_eq!((caller.line, caller.column), (7, 20), "{error}");
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
    assert_eq!(line_column(&error), (3, 33), "{error}");
    assert_eq!(error.diagnostic().call_chain.len(), 1, "{error}");
    let cause = caught.error_cause().unwrap();
    assert!(cause.same_object(&weak_cause.upgrade().unwrap()));
}

/// A cancelled cell is a catchable error at the need too, with an
/// `undefined` cause; the consumer itself is not cancelled and carries on.
#[test]
fn dependency_errors_are_error_instances() {
    // Test both ready failures and failures delivered after suspension, and
    // both direct needs and needs inside native frames. Synthetic fixtures.
    for already_failed in [false, true] {
        for need in ["__rt.need(5, 3)", "__rt.needAll(__rt.vector(5), 3)"] {
            let rt = Runtime::new();
            let cause = complete(spawn(&rt, "__rt.vector(13)").run().unwrap());
            if already_failed {
                let request = rt.claim(key(&rt, A)).unwrap();
                let _ = rt
                    .complete(
                        request.key(),
                        request.producer(),
                        Completion::Failed(&cause),
                    )
                    .unwrap();
            }
            let task = spawn(
                &rt,
                &format!(
                    "let saved; try {{ {need}; }} catch (e) {{ saved = e; }}
                 let same = false;
                 try {{ __rt.need(5, 3); }} catch (e) {{ same = saved.cause === e.cause; }}
                 let d = Object.getOwnPropertyDescriptor(saved, 'cause');
                 if (!(same && saved instanceof Error && saved.name === 'Error' &&
                    saved.message.length > 0 && d.value === saved.cause &&
                    d.writable && d.configurable && !d.enumerable)) throw 99;
                 saved.cause = null; delete saved.cause; saved.message = 'changed';
                 saved;"
                ),
            );
            let mut log = Log::default();
            let task = if already_failed {
                task
            } else {
                run_to_wait(&rt, &task, &mut log, A);
                fail(&rt, &task, &cause)
            };
            rt.collect().unwrap();
            let (outcome, _) = run_elsewhere(task, log);
            let caught = complete(outcome);
            assert_eq!(
                caught.error().unwrap().kind(),
                VmErrorKind::DependencyFailed(key(&rt, A))
            );
            assert!(caught.error_cause().unwrap().same_object(&cause));
        }
    }
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let saved; try { __rt.need(5, 3); } catch (e) { saved = e; } if (!(saved instanceof Error && saved.cause === undefined && !('cause' in saved))) throw 99; saved",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    let task = only_runnable(rt.cancel_cell(key(&rt, A)).unwrap());
    let caught = complete(run(&task, &mut log));
    assert_eq!(
        caught.error().unwrap().kind(),
        VmErrorKind::DependencyCancelled(key(&rt, A))
    );
    assert_eq!(caught.error_cause().unwrap().view(), ValueView::Undefined);
}

#[test]
fn engine_error_properties_resume_callbacks_and_cleanup() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        "let saved; try { nope; } catch (e) { saved = e; } Object.defineProperty(saved, 'name', {get() { __rt.need(5, 3); return 'N'; }}); Object.defineProperty(saved, 'message', {get() { __rt.need(6, 3); return 'M'; }}); saved.toString() === 'N: M'",
    );
    let mut log = Log::default();
    run_to_wait(&rt, &task, &mut log, A);
    rt.collect().unwrap();
    let task = publish(&rt, &task, 1.0);
    let (outcome, log) = run_elsewhere(task.clone(), log);
    assert_eq!(waiting(outcome), key(&rt, B));
    rt.collect().unwrap();
    let (outcome, _) = run_elsewhere(publish(&rt, &task, 2.0), log);
    assert_eq!(complete(outcome).view(), ValueView::Bool(true));

    for body in [
        "try { nope; } catch (e) { if (e instanceof ReferenceError) __rt.effect(1, 1); } __rt.effect(1, 2);",
        "try { Number({valueOf() { return null(); }}); } catch (e) { if (e instanceof TypeError) __rt.effect(1, 1); } __rt.effect(1, 2);",
        "null(); __rt.effect(1, 99);",
    ] {
        let rt = Runtime::new();
        let task = spawn(
            &rt,
            &format!("try {{ __rt.need(5, 3); }} finally {{ {body} }}"),
        );
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        let task = cancel_parked(&task);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(task, log);
        cancelled(outcome);
        assert_eq!(
            log.0,
            if body.starts_with("try") {
                vec![1.0, 2.0]
            } else {
                vec![]
            }
        );
    }
}

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

/// `break` and `continue` during cancellation cleanup (clj-quickjs-79): a
/// jump out of a finally block running for the cancellation would discard
/// it, so it ends the cleanup there, as a `return` does (enclosing finally
/// blocks do not run). A loop entirely inside the cleanup block jumps
/// through its own finally blocks as usual. A task cancelled while it waits
/// in a finally block that a `break` runs loses the pending jump: only the
/// enclosing finally blocks run.
#[test]
fn a_jump_out_of_a_cleanup_finally_block_ends_the_cleanup() {
    let cases: [(&str, &[f64], &[f64]); 4] = [
        (
            "try {
               while (true) { try { __rt.need(5, 3); } finally { __rt.effect(1, 1); break; } }
               __rt.effect(1, 91);
             } finally { __rt.effect(1, 92); }",
            &[],
            &[1.0],
        ),
        (
            "let i = 0;
             try {
               while (i < 5) {
                 i = i + 1;
                 try { __rt.need(5, 3); } finally { __rt.effect(1, i); continue; }
               }
               __rt.effect(1, 91);
             } finally { __rt.effect(1, 92); }",
            &[],
            &[1.0],
        ),
        (
            "try {
               try { __rt.need(5, 3); }
               finally {
                 while (true) { try { __rt.effect(1, 1); break; } finally { __rt.effect(1, 2); } }
                 __rt.effect(1, 3);
               }
               __rt.effect(1, 91);
             } finally { __rt.effect(1, 4); }",
            &[],
            &[1.0, 2.0, 3.0, 4.0],
        ),
        (
            "try {
               while (true) {
                 try { __rt.effect(1, 1); break; }
                 finally { __rt.need(5, 3); __rt.effect(1, 91); }
               }
               __rt.effect(1, 92);
             } finally { __rt.effect(1, 2); }",
            &[1.0],
            &[1.0, 2.0],
        ),
    ];
    for (src, before, after) in cases {
        let rt = Runtime::new();
        let task = spawn(&rt, src);
        let mut log = Log::default();
        run_to_wait(&rt, &task, &mut log, A);
        assert_eq!(log.0, before, "{src}");
        let resumed = cancel_parked(&task);
        rt.collect().unwrap();
        let (outcome, log) = run_elsewhere(resumed, log);
        cancelled(outcome);
        assert_eq!(log.0, after, "{src}");
        assert_eq!(task.status(), TaskStatus::Cancelled, "{src}");
        assert_eq!(task.dependency_log().waiting(), None, "{src}");
    }
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
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => self.log.push(n),
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
        match (call.op, &call.arg) {
            (1, &Primitive::Number(n)) => {
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

/// A `break` and a `continue` through two finally blocks of a callee
/// (clj-quickjs-79): the outer finally block that the `break` runs parks on
/// a pending cell, with the jump pending in the task's handler stack.
/// Collections run on both workers while it is parked; the woken task
/// resumes on the other worker, finishes the finally block and completes
/// the jump (leaving the loop, then the function). Each effect ran exactly
/// once, on the worker the language's control flow put it on.
#[test]
fn a_break_through_finally_survives_a_wait_and_migration_under_the_executor() {
    let rt = Runtime::new();
    let effects = Effects::default();
    let exec = migrating_executor(&rt, &effects);
    let task = spawn(
        &rt,
        "function f() {
           let n = 0;
           while (true) {
             n = n + 1;
             try {
               try { __rt.effect(1, n); if (n === 2) break; continue; }
               finally { __rt.effect(1, 10 + n); }
             } finally { if (n === 2) __rt.effect(1, __rt.need(5, 3)); }
             __rt.effect(1, 91);
           }
           __rt.effect(1, 5);
           return n;
         }
         let r;
         try { r = f(); } catch (e) { __rt.effect(1, 92); } finally { __rt.effect(1, 6); }
         r",
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
    assert_eq!(number(outcome), 2.0);
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    let id = task.id();
    assert_eq!(
        *effects.lock().unwrap(),
        [
            (w0, id, 1.0),
            (w0, id, 11.0),
            (w0, id, 2.0),
            (w0, id, 12.0),
            (w1, id, 4.0),
            (w1, id, 5.0),
            (w1, id, 6.0)
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

#[test]
fn to_primitive_dependency_failure_unwinds_to_the_operator_handler() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        let result = 0;
        try {
            ({valueOf: function () {
                try { return __rt.need(5, 3); }
                finally { __rt.effect(1, 1); }
            }}) * 2;
        } catch (e) { result = 7; }
        finally { __rt.effect(1, 2); }
        result + 1;
    "#,
    );
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), key(&rt, A));
    rt.collect().unwrap();
    let error = rt.root(Primitive::String("conversion failed".into()));
    let resumed = fail(&rt, &task, &error);
    let (outcome, log) = run_elsewhere(resumed, log);
    assert_eq!(number(outcome), 8.0);
    assert_eq!(log.0, [1.0, 2.0]);
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.function, 0);
}

#[test]
fn cancelling_to_primitive_runs_finally_and_discards_the_conversion() {
    let rt = Runtime::new();
    let task = spawn(
        &rt,
        r#"
        try {
            ({valueOf: function () {
                try { return __rt.need(5, 3); }
                finally { __rt.effect(1, 1); }
            }, toString: function () { __rt.effect(1, 99); return '1'; }}) + 2;
        } catch (e) { __rt.effect(1, 98); }
        finally { __rt.effect(1, 2); }
        __rt.effect(1, 97);
    "#,
    );
    let mut log = Log::default();
    assert_eq!(waiting(run(&task, &mut log)), key(&rt, A));
    rt.collect().unwrap();
    let resumed = cancel_parked(&task);
    let (outcome, log) = run_elsewhere(resumed, log);
    cancelled(outcome);
    assert_eq!(log.0, [1.0, 2.0]);
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.function, 0);
    assert!(
        matches!(rt.cell_status(key(&rt, A)), CellStatus::Pending { ref waiters, .. } if waiters.is_empty())
    );
}

/// Synthetic fixtures, not light-cljs output. Pending constructor conversions
/// share the native unwind rules, including nested built-in callbacks.
#[test]
fn primitive_wrapper_conversion_dependency_failure_unwinds() {
    for expression in [
        "Number(o)",
        "new Number(o)",
        "String(o)",
        "new String(o)",
        "+({valueOf: Number.bind(null, o)})",
        "String({toString: String.bind(null, o)})",
    ] {
        let rt = Runtime::new();
        let task = spawn(
            &rt,
            &format!(
                r#"
            function convert() {{
                try {{ return __rt.need(5, 3); }}
                finally {{ __rt.effect(1, 1); }}
            }}
            let o = {{valueOf: convert, toString: convert}};
            let result = 0;
            try {{ {expression}; }}
            catch(e) {{ result = e; }}
            finally {{ __rt.effect(1, 2); }}
            result;
        "#
            ),
        );
        let mut log = Log::default();
        assert_eq!(waiting(run(&task, &mut log)), key(&rt, A));
        assert!(log.0.is_empty());
        rt.collect().unwrap();
        let error = rt.root(Primitive::String("conversion failed".into()));
        let resumed = fail(&rt, &task, &error);
        let (outcome, log) = run_elsewhere(resumed, log);
        let caught = complete(outcome);
        assert_eq!(
            caught.error().expect("dependency error").kind(),
            VmErrorKind::DependencyFailed(key(&rt, A)),
            "{expression}"
        );
        assert_eq!(
            caught.error_cause().unwrap().string().unwrap().to_string(),
            "conversion failed",
            "{expression}"
        );
        assert_eq!(log.0, [1.0, 2.0], "{expression}");
        rt.collect().unwrap();
        assert_eq!(rt.heap_usage().objects.function, 0);
    }
}

#[test]
fn primitive_wrapper_conversion_cancellation_discards_nested_natives() {
    for expression in [
        "Number(o)",
        "new Number(o)",
        "String(o)",
        "new String(o)",
        "+({valueOf: Number.bind(null, o)})",
        "String({toString: String.bind(null, o)})",
    ] {
        let rt = Runtime::new();
        let task = spawn(
            &rt,
            &format!(
                r#"
            function convert() {{
                try {{ return __rt.need(5, 3); }}
                finally {{ __rt.effect(1, 1); }}
            }}
            let o = {{valueOf: convert, toString: convert}};
            try {{ {expression}; }}
            catch(e) {{ __rt.effect(1, 98); }}
            finally {{ __rt.effect(1, 2); }}
            __rt.effect(1, 97);
        "#
            ),
        );
        let mut log = Log::default();
        assert_eq!(waiting(run(&task, &mut log)), key(&rt, A));
        assert!(log.0.is_empty());
        rt.collect().unwrap();
        let resumed = cancel_parked(&task);
        let (outcome, log) = run_elsewhere(resumed, log);
        cancelled(outcome);
        assert_eq!(log.0, [1.0, 2.0], "{expression}");
        rt.collect().unwrap();
        assert_eq!(rt.heap_usage().objects.function, 0);
        assert!(
            matches!(rt.cell_status(key(&rt, A)), CellStatus::Pending { ref waiters, .. } if waiters.is_empty())
        );
    }
}
