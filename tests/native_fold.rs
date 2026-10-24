//! Resumable native operations (pellet clj-quickjs-12, `docs/design.md`
//! §§5, 10–11, 13): `__rt.reduce` folds a persistent vector through a
//! JavaScript callback, and `__rt.needAll` needs one dependency per element,
//! each as a task-owned native continuation the interpreter loop drives step
//! by step. Programs are JavaScript source: the synthetic fixtures
//! `fixtures/js/fold_consumer.js`, `fold_needs.js` and `need_producer.js`
//! (not light-cljs output) and inline snippets.
//!
//! The tests play a deterministic, manually driven host, as in
//! `tests/need.rs`: they take the producer requests a suspended task
//! claimed, publish results, and run the tasks the wakeups return. Every
//! cross-thread step is sequenced by an observed state or a join, never by
//! sleeping.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clj_quickjs::frontend::compile_function;
use clj_quickjs::{
    Budget, Cancel, Completion, DependencyId, DependencyKey, Executable, Host, HostCall, Primitive,
    ProducerRequest, ResultKind, Rooted, RunOutcome, Runtime, Task, TaskStatus, ValueView,
    VmErrorKind, Wakeups, WeakHandle,
};

/// Bound for every cross-thread handshake.
const HANDSHAKE: Duration = Duration::from_secs(10);

/// Records every host effect: operation 1 returns its argument plus 100,
/// the others return 0.
#[derive(Default)]
struct Recorder {
    effects: Vec<(u16, f64)>,
}

impl Recorder {
    fn op(&self, op: u16) -> Vec<f64> {
        self.effects
            .iter()
            .filter(|(o, _)| *o == op)
            .map(|&(_, n)| n)
            .collect()
    }
}

impl Host for Recorder {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            return Err(format!("unexpected argument {:?}", call.arg));
        };
        self.effects.push((call.op, n));
        Ok(Primitive::Number(if call.op == 1 {
            n + 100.0
        } else {
            0.0
        }))
    }
}

fn key(rt: &Runtime, def: u64, kind: u32) -> DependencyKey {
    DependencyKey::new(rt.world(), DependencyId::new(def), ResultKind::new(kind))
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

/// A persistent vector of numbers, rooted.
fn number_vector(rt: &Runtime, items: &[f64]) -> Rooted {
    rt.scope(|s| {
        let items: Vec<_> = items
            .iter()
            .map(|&n| s.primitive(Primitive::Number(n)))
            .collect();
        let v = s.new_vector(&items).unwrap();
        s.persist(v)
    })
}

/// A rooted vector whose elements are vectors of `1..=n` for each `n` in
/// `counts`, and weak handles to the outer vector and to each element.
fn nested_vector(rt: &Runtime, counts: &[usize]) -> (Rooted, Vec<WeakHandle>) {
    rt.scope(|s| {
        let mut weak = Vec::new();
        let elements: Vec<_> = counts
            .iter()
            .map(|&n| {
                let items: Vec<_> = (1..=n)
                    .map(|i| s.primitive(Primitive::Number(i as f64)))
                    .collect();
                let e = s.new_vector(&items).unwrap();
                weak.push(s.downgrade(e));
                e
            })
            .collect();
        let v = s.new_vector(&elements).unwrap();
        weak.insert(0, s.downgrade(v));
        (s.persist(v), weak)
    })
}

fn one_request(task: &Task, expected: DependencyKey) -> ProducerRequest {
    let mut requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "{requests:?}");
    let request = requests.pop().unwrap();
    assert_eq!(request.key(), expected);
    request
}

fn publish_number(rt: &Runtime, request: &ProducerRequest, n: f64) -> Wakeups {
    let value = rt.root(Primitive::Number(n));
    rt.complete(request.key(), request.producer(), Completion::Ready(&value))
        .unwrap()
}

fn only_runnable(wakeups: Wakeups) -> Task {
    let mut runnable = wakeups.into_runnable();
    assert_eq!(runnable.len(), 1);
    runnable.pop().unwrap()
}

/// The source line and column of the suspended task's next instruction.
fn suspended_at(task: &Task) -> (u32, u32) {
    let at = task.source_location().expect("a source location");
    (at.line, at.column)
}

/// Acceptance 1: native → JavaScript callback → dependency wait → callback
/// resume → native continuation. The fold's first callback captures the
/// live local `seen`, performs its effect, and needs a helper nobody has
/// produced: the whole task suspends inside the callback inside the fold.
/// Once a VM producer task's helper is published, a run on another thread
/// resumes the callback where it stopped, returns into the fold, which calls
/// the remaining elements (the helper is ready now) and completes.
#[test]
fn a_callback_waits_inside_the_fold_and_the_fold_completes_after_resume() {
    let rt = Runtime::new();
    let items = number_vector(&rt, &[1.0, 2.0, 3.0]);
    let scale = rt.root(Primitive::Number(10.0));
    let exe = compile_function(include_str!("fixtures/js/fold_consumer.js")).unwrap();
    let task = rt.spawn(&exe, &[&items, &scale]).unwrap();
    drop(items);
    let mut host = Recorder::default();

    // Run 1: the pre-fold effect, the first callback's effect, then the
    // helper is pending.
    let helper_key = key(&rt, 7, 2);
    let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert_eq!(waiting(outcome), helper_key);
    assert_eq!(host.effects, [(1, 10.0), (2, 1.0)]);
    assert_eq!(suspended_at(&task), (15, 20), "at the need in `step`");
    let request = one_request(&task, helper_key);

    // Collected while parked inside the fold: nothing it needs is lost.
    rt.collect().unwrap();

    // The helper is a VM function produced by another task.
    let producer_exe = compile_function(include_str!("fixtures/js/need_producer.js")).unwrap();
    let offset = rt.root(Primitive::Number(1000.0));
    let producer = rt.spawn(&producer_exe, &[&offset]).unwrap();
    let helper = complete(producer.run().unwrap());
    let wakeups = rt
        .complete(
            request.key(),
            request.producer(),
            Completion::Ready(&helper),
        )
        .unwrap();
    let resumed = only_runnable(wakeups);

    // Run 2, on another thread: the callback continues at its need, the
    // fold continues with elements 2 and 3, and the task completes.
    let (outcome, host) = thread::spawn(move || {
        let outcome = resumed.run_with(Budget::UNLIMITED, &mut host).unwrap();
        (outcome, host)
    })
    .join()
    .unwrap();
    // pre + sum of helper(x * scale) = 110 + (20 + 1000) + (40 + 1000) + (60 + 1000).
    assert_eq!(number(&complete(outcome)), 3230.0);
    assert_eq!(host.op(1), [10.0], "the pre-fold effect ran once");
    assert_eq!(host.op(2), [1.0, 2.0, 3.0], "one callback per element");
    assert_eq!(host.op(3), [3.0], "`seen` counted each callback once");
    assert_eq!(task.status(), TaskStatus::Finished);
    assert_eq!(task.dependency_log().edges(), &[helper_key]);
}

/// Acceptance 2: every element's callback needs its own pending dependency,
/// so the fold pauses once per element, and every run is also cut to one
/// instruction or native step, so it yields between all of them. No
/// callback, effect or completed element is repeated.
#[test]
fn a_fold_pauses_at_every_element_and_every_step_without_repeating_any() {
    let rt = Runtime::new();
    let defs = [21.0, 22.0, 23.0, 24.0];
    let input = number_vector(&rt, &defs);
    let exe = compile_function(include_str!("fixtures/js/fold_needs.js")).unwrap();
    let mut task = rt.spawn(&exe, &[&input]).unwrap();
    let mut host = Recorder::default();
    let (mut waits, mut yields) = (Vec::new(), 0);
    let result = loop {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => yields += 1,
            RunOutcome::Waiting(key) => {
                // Suspended inside the callback: its effect ran once, and the
                // elements before it are not visited again.
                let reported = host.op(2);
                assert_eq!(
                    reported,
                    defs[..=waits.len()],
                    "after {} waits",
                    waits.len()
                );
                waits.push(key.dependency());
                let request = one_request(&task, key);
                rt.collect().unwrap();
                let def = defs[waits.len() - 1];
                task = only_runnable(publish_number(&rt, &request, def * 100.0));
            }
            RunOutcome::Complete(v) => break v,
            other => panic!("unexpected {other:?}"),
        }
    };
    let expected: Vec<DependencyId> = defs.iter().map(|&d| DependencyId::new(d as u64)).collect();
    assert_eq!(waits, expected, "one pause per element, in order");
    assert!(yields > 4 * 4, "budget pauses between steps: {yields}");
    assert_eq!(host.op(2), defs, "each callback ran exactly once");
    let expected: Vec<f64> = defs.iter().map(|d| d * 100.0 + d).collect();
    assert_eq!(numbers(&rt, &result), expected);
}

/// `function only_native(items)`: the vector is handed to the fold and the
/// local cleared in the same expression, so while the fold runs, the vector
/// and its unvisited elements are reachable only from the native frame.
/// Each callback needs definition 9 (kind 3) and host operation 4 reports
/// the accumulator.
const ONLY_NATIVE: &str = "
function only_native(items) {
  function first(a, b) { return a; }
  function step(acc, x) {
    __rt.effect(4, acc);
    return acc + __rt.count(x) + __rt.need(9, 3);
  }
  return __rt.reduce(step, 0, first(items, items = null));
}";

fn spawn_only_native(rt: &Runtime, counts: &[usize]) -> (Task, Vec<WeakHandle>) {
    let exe: Executable = compile_function(ONLY_NATIVE).unwrap();
    let (input, weak) = nested_vector(rt, counts);
    let task = rt.spawn(&exe, &[&input]).unwrap();
    drop(input);
    (task, weak)
}

/// Acceptance 3: collections while the vector and its elements are
/// reachable only through the pending native continuation (suspended in a
/// callback, and yielded between native steps) keep them valid; once the
/// task finishes they are reclaimed.
#[test]
fn values_held_only_by_a_pending_fold_survive_collection_until_it_finishes() {
    let rt = Runtime::new();
    let (task, weak) = spawn_only_native(&rt, &[1, 2, 3]);
    let mut host = Recorder::default();
    let need = key(&rt, 9, 3);
    assert_eq!(
        waiting(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        need
    );
    let request = one_request(&task, need);
    for _ in 0..2 {
        rt.collect().unwrap();
        assert!(weak.iter().all(WeakHandle::is_live), "kept by the fold");
    }

    // Resume one instruction or step per run, collecting at every pause:
    // the fold's own frame keeps the vector valid for the remaining steps,
    // and only the fold's last step (popping its frame) releases it: the one
    // pause after that, before the entry frame's `return`, finds it gone.
    let task = only_runnable(publish_number(&rt, &request, 5.0));
    let mut kept = Vec::new();
    let result = loop {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
                kept.push(weak[0].is_live());
            }
            RunOutcome::Complete(v) => break v,
            other => panic!("unexpected {other:?}"),
        }
    };
    // (1 + 5) + (2 + 5) + (3 + 5)
    assert_eq!(number(&result), 21.0);
    assert_eq!(host.op(4), [0.0, 6.0, 13.0]);
    let (last, before) = kept.split_last().unwrap();
    assert!(before.len() > 10 && before.iter().all(|&k| k), "{kept:?}");
    assert!(!last, "released when the fold returned");

    // Nothing else holds the vector or its elements.
    rt.collect().unwrap();
    assert!(
        weak.iter().all(|w| !w.is_live()),
        "reclaimed after the task"
    );
}

/// Cancelling a task suspended inside a fold is terminal cleanup too: the
/// native frame's values become garbage.
#[test]
fn cancelling_a_task_inside_a_fold_releases_its_native_state() {
    let rt = Runtime::new();
    let (task, weak) = spawn_only_native(&rt, &[2, 2]);
    let need = key(&rt, 9, 3);
    let mut host = Recorder::default();
    assert_eq!(
        waiting(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        need
    );
    rt.collect().unwrap();
    assert!(weak.iter().all(WeakHandle::is_live));
    let Cancel::Runnable(cancelled) = task.cancel() else {
        panic!("a parked task is made runnable to observe its cancellation");
    };
    assert!(matches!(cancelled.run().unwrap(), RunOutcome::Cancelled));
    assert_eq!(task.status(), TaskStatus::Cancelled);
    rt.collect().unwrap();
    assert!(weak.iter().all(|w| !w.is_live()));
}

/// Host for the running-collection test: the first effect requests a
/// collection from another thread and returns once the request is raised,
/// so the run parks at its next VM poll; later effects record whether the
/// fold's values survived it.
struct CollectingHost {
    rt: Runtime,
    weak: Vec<WeakHandle>,
    collector: Option<thread::JoinHandle<()>>,
    observed: Vec<(u64, bool)>,
}

impl Host for CollectingHost {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        if self.collector.is_none() {
            let rt = self.rt.clone();
            let (tx, rx) = mpsc::channel();
            self.collector = Some(thread::spawn(move || {
                tx.send(()).unwrap();
                rt.request_collection().unwrap();
            }));
            rx.recv_timeout(HANDSHAKE).unwrap();
            let deadline = Instant::now() + HANDSHAKE;
            while !self.rt.collection_requested() {
                if Instant::now() > deadline {
                    return Err("the collection request never arrived".to_string());
                }
                thread::yield_now();
            }
        } else {
            let collections = self.rt.heap_stats().collections;
            self.observed
                .push((collections, self.weak.iter().all(WeakHandle::is_live)));
        }
        Ok(Primitive::Number(0.0))
    }
}

/// A collection requested while the fold runs on this thread: the run parks
/// at its next poll with the native frame among the frames it reports, and
/// the values only that frame holds survive.
#[test]
fn a_running_fold_reports_its_native_frame_at_a_safepoint() {
    let rt = Runtime::new();
    let need = key(&rt, 9, 3);
    // Published up front: no suspension, one run.
    let request = rt.claim(need).unwrap();
    let _ = publish_number(&rt, &request, 0.0);
    let (task, weak) = spawn_only_native(&rt, &[1, 2, 3]);
    let mut host = CollectingHost {
        rt: rt.clone(),
        weak,
        collector: None,
        observed: Vec::new(),
    };
    let result = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(number(&result), 6.0);
    host.collector.take().unwrap().join().unwrap();
    assert_eq!(host.observed, [(1, true), (1, true)]);
    rt.collect().unwrap();
    assert!(host.weak.iter().all(|w| !w.is_live()));
}

/// `__rt.needAll` demands dependencies from the native continuation itself
/// (no callback): it suspends at the call for each pending definition, a
/// ready one continues in the same run, and the result vector keeps the
/// published values in order.
#[test]
fn need_all_suspends_in_the_native_for_each_pending_dependency() {
    let rt = Runtime::new();
    let exe = compile_function(
        "function all(defs) {
  __rt.effect(2, 1);
  const results = __rt.needAll(defs, 3);
  __rt.effect(2, 2);
  return results;
}",
    )
    .unwrap();
    // Definition 31 is ready before the task runs.
    let ready = rt.claim(key(&rt, 31, 3)).unwrap();
    let _ = publish_number(&rt, &ready, 310.0);
    let input = number_vector(&rt, &[30.0, 31.0, 32.0]);
    let task = rt.spawn(&exe, &[&input]).unwrap();
    let mut host = Recorder::default();

    let mut outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
    for (def, value) in [(30, 300.0), (32, 320.0)] {
        let pending = key(&rt, def, 3);
        assert_eq!(waiting(outcome), pending);
        assert_eq!(suspended_at(&task), (3, 19), "at the needAll call");
        assert_eq!(host.op(2), [1.0], "nothing after the call ran");
        let request = one_request(&task, pending);
        rt.collect().unwrap();
        let resumed = only_runnable(publish_number(&rt, &request, value));
        outcome = resumed.run_with(Budget::UNLIMITED, &mut host).unwrap();
    }
    let results = complete(outcome);
    assert_eq!(numbers(&rt, &results), [300.0, 310.0, 320.0]);
    assert_eq!(host.op(2), [1.0, 2.0]);
    let edges = [30, 31, 32].map(|d| key(&rt, d, 3));
    assert_eq!(
        task.dependency_log().edges(),
        edges,
        "ready reads are recorded too"
    );
}

/// Failures inside a native report the starting call; a failure inside a
/// callback reports the callback, with the fold's call site in its chain.
#[test]
fn native_failures_name_the_call_site() {
    let rt = Runtime::new();
    let run = |src: &str, args: &[&Rooted]| {
        let exe = compile_function(src).unwrap();
        let task = rt.spawn(&exe, args).unwrap();
        match task
            .run_with(Budget::UNLIMITED, &mut Recorder::default())
            .unwrap()
        {
            RunOutcome::Failed(e) => e,
            other => panic!("expected Failed, got {other:?}"),
        }
    };
    let three = number_vector(&rt, &[1.0, 2.0, 3.0]);

    // Not a function (checked up front, even before any element).
    let e = run(
        "function f(v) {\n  return __rt.reduce(1, 0, v);\n}",
        &[&three],
    );
    assert_eq!(e.kind(), VmErrorKind::TypeError);
    assert!(e.to_string().contains("number is not a function"), "{e}");
    let at = e.diagnostic().source.unwrap();
    assert_eq!((at.line, at.column), (2, 10));

    // Not a vector.
    let e = run(
        "function f(v) {\n  function g(a, x) { return a; }\n  return __rt.reduce(g, 0, 5);\n}",
        &[],
    );
    assert!(matches!(e.kind(), VmErrorKind::Collection(_)), "{e}");

    // A callback that fails: the failure names its own frame, and its
    // caller chain includes the fold's call site.
    let e = run(
        "function f(v) {\n  function g(a, x) { return a + x + undefinedName; }\n  return __rt.reduce(g, 0, v);\n}",
        &[&three],
    );
    assert_eq!(e.kind(), VmErrorKind::ReferenceError);
    let chain = &e.diagnostic().call_chain;
    assert_eq!(chain.len(), 1, "{chain:?}");
    let at = chain[0].source.unwrap();
    assert_eq!((at.line, at.column), (3, 10));

    // needAll: a bad definition id fails at the call, after the valid ones.
    let bad = number_vector(&rt, &[1.5]);
    let e = run("function f(v) {\n  return __rt.needAll(v, 3);\n}", &[&bad]);
    assert_eq!(e.kind(), VmErrorKind::UnsupportedOperand);
    assert!(e.to_string().contains("got 1.5 at index 0"), "{e}");
}

/// Nested folds: a callback starts its own fold, so native frames stack
/// above call frames and each resumes its own caller.
#[test]
fn folds_nest_through_callbacks() {
    let rt = Runtime::new();
    let exe = compile_function(
        "function f(rows) {
  function add(a, x) { return a + x; }
  function row(acc, r) { return acc + __rt.reduce(add, 0, r) * __rt.need(40, 1); }
  return __rt.reduce(row, 0, rows);
}",
    )
    .unwrap();
    let (rows, _) = nested_vector(&rt, &[1, 2, 3]);
    let task = rt.spawn(&exe, &[&rows]).unwrap();
    let need = key(&rt, 40, 1);
    assert_eq!(waiting(task.run().unwrap()), need);
    let request = one_request(&task, need);
    let resumed = only_runnable(publish_number(&rt, &request, 10.0));
    // (1 + (1 + 2) + (1 + 2 + 3)) * 10
    assert_eq!(number(&complete(resumed.run().unwrap())), 100.0);
}

/// Run `task` one instruction or native step per run to completion,
/// collecting at every pause and recording whether all of `weak` survived.
fn step_and_collect(rt: &Runtime, task: &Task, weak: &[WeakHandle]) -> (Rooted, Vec<bool>) {
    let mut kept = Vec::new();
    loop {
        match task
            .run_with(Budget::instructions(1), &mut Recorder::default())
            .unwrap()
        {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
                kept.push(weak.iter().all(WeakHandle::is_live));
            }
            RunOutcome::Complete(v) => return (v, kept),
            other => panic!("unexpected {other:?}"),
        }
    }
}

/// The callback and the heap-valued initial accumulator, like the vector,
/// are reachable only from the fold's frame once `reduce` has started (its
/// first pause is before the first callback is entered); they stay valid
/// until the fold returns. The callback returns the accumulator unchanged, so
/// the initializer is the accumulator throughout and the result.
#[test]
fn a_fold_alone_keeps_its_callback_and_initial_accumulator() {
    let rt = Runtime::new();
    let maker = compile_function(
        "function maker() {\n  return function (acc, x) { return __rt.count(acc) + x > 0 ? acc : null; };\n}",
    )
    .unwrap();
    let f = complete(rt.spawn(&maker, &[]).unwrap().run().unwrap());
    let init = number_vector(&rt, &[0.0]);
    let items = number_vector(&rt, &[1.0, 2.0]);
    let weak = [f.downgrade(), init.downgrade(), items.downgrade()];
    let exe = compile_function(
        "function main(f, init, items) {
  function first(a, b) { return a; }
  return __rt.reduce(first(f, f = null), first(init, init = null), first(items, items = null));
}",
    )
    .unwrap();
    let task = rt.spawn(&exe, &[&f, &init, &items]).unwrap();
    drop((f, init, items));
    let (result, kept) = step_and_collect(&rt, &task, &weak);
    assert!(
        result.same_object(&weak[1].upgrade().unwrap()),
        "init itself"
    );
    let (last, before) = kept.split_last().unwrap();
    assert!(before.len() > 10 && before.iter().all(|&k| k), "{kept:?}");
    assert!(!last, "released when the fold returned");
}

/// `needAll`'s definitions vector and its results so far are reachable only
/// from its native frame while it waits; both survive collections at every
/// wait, and the definitions are reclaimed once the task finishes.
#[test]
fn need_all_alone_keeps_its_definitions_and_results_while_waiting() {
    let rt = Runtime::new();
    let exe = compile_function(
        "function main(defs) {
  function first(a, b) { return a; }
  return __rt.needAll(first(defs, defs = null), 3);
}",
    )
    .unwrap();
    let defs = number_vector(&rt, &[50.0, 51.0, 52.0]);
    let weak = defs.downgrade();
    let task = rt.spawn(&exe, &[&defs]).unwrap();
    drop(defs);
    let mut outcome = task.run().unwrap();
    for def in [50, 51, 52] {
        let pending = key(&rt, def, 3);
        assert_eq!(waiting(outcome), pending);
        let request = one_request(&task, pending);
        rt.collect().unwrap();
        assert!(weak.is_live(), "kept by the waiting needAll");
        // A heap-valued result each time, so the results so far hold objects
        // only the native frame reaches.
        let value = number_vector(&rt, &[def as f64]);
        let wakeups = rt
            .complete(pending, request.producer(), Completion::Ready(&value))
            .unwrap();
        drop(value);
        outcome = only_runnable(wakeups).run().unwrap();
    }
    let results = complete(outcome);
    let firsts: Vec<f64> = rt.scope(|s| {
        let v = s.local(&results).unwrap();
        s.items(v)
            .unwrap()
            .into_iter()
            .map(|r| match s.view(s.nth(r, 0).unwrap()) {
                ValueView::Number(n) => n,
                other => panic!("{other:?}"),
            })
            .collect()
    });
    assert_eq!(firsts, [50.0, 51.0, 52.0]);
    rt.collect().unwrap();
    assert!(!weak.is_live(), "reclaimed after the task");
}

/// Empty inputs complete from the first step: `reduce` returns its initial
/// accumulator itself without calling the callback, `needAll` an empty
/// vector without needing anything.
#[test]
fn empty_inputs_complete_without_callbacks_or_needs() {
    let rt = Runtime::new();
    let exe = compile_function(
        "function main(init, empty) {
  function f(acc, x) { __rt.effect(2, x); return x; }
  return __rt.vector(__rt.reduce(f, init, empty), __rt.needAll(empty, 3));
}",
    )
    .unwrap();
    let init = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let empty = number_vector(&rt, &[]);
    let task = rt.spawn(&exe, &[&init, &empty]).unwrap();
    let mut host = Recorder::default();
    let result = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert!(host.effects.is_empty(), "no callback ran");
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        let items = s.items(v).unwrap();
        assert_eq!(items.len(), 2);
        assert!(
            s.persist(items[0]).same_object(&init),
            "the initializer itself"
        );
        assert_eq!(s.count(items[1]).unwrap(), 0);
    });
    assert!(task.dependency_log().edges().is_empty());
    assert!(task.take_producer_requests().is_empty());
}
