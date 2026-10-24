//! The collector's root inventory, one holder at a time (pellet
//! clj-quickjs-31, `docs/design.md` §11 "The collector must see"): for
//! every place a heap value can be held, a retention test (the value
//! survives forced collections while reachable only through that holder)
//! and a reclamation test (once the holder releases it, a collection
//! reclaims it and every handle to it is rejected as stale, also after its
//! storage is reused). A multi-worker variant keeps every holder alive while
//! executor workers allocate and collect.
//!
//! The inventory, and which test covers each `RootCategory` and object kind,
//! is `src/heap/inventory.rs`; its cross-check fails if a test named there is
//! missing from this file. Holders the public API cannot reach (object
//! constants, native payload edges, a dropped dependency cell) are tested
//! there. The inventory is documented in `docs/architecture.md`, "Root
//! inventory".
//!
//! Programs are JavaScript source (synthetic fixtures, not light-cljs
//! output). Collections are forced with `Runtime::request_collection`;
//! every cross-thread step waits on a latch, channel or executor event with
//! a bound, never on a sleep.

use std::any::Any;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, Weak};
use std::thread;
use std::time::{Duration, Instant};

use clj_quickjs::exec::{CollectionEvent, Event, EventKind, ExecutorConfig, OutcomeKind};
use clj_quickjs::frontend::compile_function;
use clj_quickjs::{
    Budget, CasOutcome, CellStatus, CollectionObserver, CollectionReport, Completion, DependencyId,
    DependencyKey, Executable, Executor, HandleError, Host, HostCall, MutatorState, Placement,
    Primitive, Provenance, ResultKind, Rooted, RunOutcome, Runtime, Task, TaskId, ValueView,
    WeakHandle, WorkerId,
};

/// Bound for every cross-thread wait.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A loop long enough that a small budget always yields inside it.
const SPIN: &str = "let i = 0; while (i < 1000000) { i = i + 1; }";

/// Instructions a budgeted run gets before it yields: enough for every
/// fixture to reach its spin loop, far too few to leave it.
const PROLOGUE: u64 = 64;

/// Host for programs without effects.
struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected effect {}", call.op))
    }
}

/// Objects reachable only through one holder, and how to release it.
struct Held {
    /// Non-rooting handles to the objects only the holder keeps alive.
    weak: Vec<WeakHandle>,
    /// Releases the holder (consumes the root, task or executor).
    release: Box<dyn FnOnce() + Send>,
    /// Other state the fixture needs alive (for example the atom whose old
    /// value a reader retains); dropped at the end of the test.
    keep: Box<dyn Any + Send>,
}

impl Held {
    fn new(weak: Vec<WeakHandle>, release: impl FnOnce() + Send + 'static) -> Held {
        Held {
            weak,
            release: Box::new(release),
            keep: Box::new(()),
        }
    }

    fn keeping(mut self, keep: impl Any + Send) -> Held {
        self.keep = Box::new(keep);
        self
    }
}

/// Force a collection; every object of `held` survives it with its identity.
fn assert_retained(rt: &Runtime, held: &Held) {
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert_live(&held.weak);
    }
}

fn assert_live(weak: &[WeakHandle]) {
    for (i, w) in weak.iter().enumerate() {
        assert!(w.is_live(), "object {i} was reclaimed while held");
        // An upgrade checks the slot generation: it succeeds only while the
        // very same object occupies the slot.
        w.upgrade().expect("a held object upgrades");
    }
}

/// Release the holder and collect: every object of `held` is reclaimed and
/// rejected as stale, also after new objects reuse its storage.
fn assert_reclaimed(rt: &Runtime, held: Held) {
    let Held {
        weak,
        release,
        keep,
    } = held;
    release();
    let before = rt.heap_stats().objects;
    let stats = rt.request_collection().unwrap();
    assert!(
        stats.reclaimed >= weak.len(),
        "{} reclaimed, {} expected at least",
        stats.reclaimed,
        weak.len()
    );
    assert_eq!(rt.heap_stats().objects, before - stats.reclaimed);
    assert_stale(rt, &weak);
    // New objects take over the freed slots; the stale handles must not
    // name them.
    let newcomers: Vec<Rooted> = (0..2 * weak.len() + 4).map(|_| object(rt).0).collect();
    assert_stale(rt, &weak);
    drop((newcomers, keep));
}

fn assert_stale(rt: &Runtime, weak: &[WeakHandle]) {
    for (i, w) in weak.iter().enumerate() {
        assert!(!w.is_live(), "object {i} survived its holder's release");
        assert_eq!(w.upgrade().unwrap_err(), HandleError::Reclaimed);
        rt.scope(|s| assert_eq!(s.upgrade(w).unwrap_err(), HandleError::Reclaimed));
    }
}

/// A new plain object rooted for the caller, and a weak handle to it.
fn object(rt: &Runtime) -> (Rooted, WeakHandle) {
    let rooted = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let weak = rooted.downgrade();
    (rooted, weak)
}

/// A shareable value: a vector holding a vector. Weak handles to both.
fn shareable(rt: &Runtime) -> (Rooted, Vec<WeakHandle>) {
    rt.scope(|s| {
        let one = s.primitive(Primitive::Number(1.0));
        let inner = s.new_vector(&[one]).unwrap();
        let outer = s.new_vector(&[inner, one]).unwrap();
        let weak = vec![s.downgrade(outer), s.downgrade(inner)];
        (s.persist(outer), weak)
    })
}

fn compile(source: &str) -> Executable {
    compile_function(source).unwrap_or_else(|e| panic!("{e:?}"))
}

/// Spawn `source` with a fresh object as its only argument; the caller's
/// root for the argument is dropped, so only the task holds it.
fn spawn_holding(rt: &Runtime, source: &str) -> (Task, WeakHandle) {
    let (arg, weak) = object(rt);
    let task = rt.spawn(&compile(source), &[&arg]).unwrap();
    drop(arg);
    (task, weak)
}

/// Run `task` for [`PROLOGUE`] instructions; it must yield.
fn run_prologue(task: &Task) {
    let outcome = task
        .run_with(Budget::instructions(PROLOGUE), &mut NoEffects)
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Yielded), "{outcome:?}");
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

fn run_to_end(task: &Task) -> Rooted {
    complete(task.run_with(Budget::UNLIMITED, &mut NoEffects).unwrap())
}

fn key(rt: &Runtime, def: u64) -> DependencyKey {
    DependencyKey::new(rt.world(), DependencyId::new(def), ResultKind::new(1))
}

// ---------------------------------------------------------------------------
// Fixtures: one per holder. Each returns objects reachable only through it.
// ---------------------------------------------------------------------------

/// Host root: a `Rooted` handle (`RootCategory::PersistentHandle`).
fn persistent_handle(rt: &Runtime) -> Held {
    let (root, weak) = object(rt);
    Held::new(vec![weak], move || drop(root))
}

/// Edges of the persistent kinds: a rooted map holding a vector holding an
/// occurrence of a plain object.
fn persistent_collection(rt: &Runtime) -> Held {
    let (root, weak) = rt.scope(|s| {
        let o = s.new_object();
        let at = Provenance {
            source: 1,
            line: 2,
            column: 3,
        };
        let occ = s.new_occurrence(o, at).unwrap();
        let vec = s.new_vector(&[occ]).unwrap();
        let k = s.primitive(Primitive::Number(0.0));
        let map = s.new_map(&[(k, vec)]).unwrap();
        let weak = [map, vec, occ, o].map(|l| s.downgrade(l)).to_vec();
        (s.persist(map), weak)
    });
    Held::new(weak, move || drop(root))
}

/// A task's published result: the `Rooted` of `RunOutcome::Complete`,
/// owned by the embedder after the task is gone.
fn task_result(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(rt, "function f(o) { return o; }");
    let result = run_to_end(&task);
    drop(task);
    Held::new(vec![weak], move || drop(result))
}

/// A suspended task frame: a yielded task's local (`TaskFrame`, through the
/// task's root source).
fn suspended_frame(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(rt, &format!("function f(o) {{ {SPIN} return 0; }}"));
    run_prologue(&task);
    Held::new(vec![weak], move || drop(task))
}

/// A task suspended at a pending `__rt.need`: its frames are saved with the
/// pending need; the dependency cell holds only a weak waiter.
fn pending_need(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(rt, "function f(o) { const r = __rt.need(7, 1); return o; }");
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(rt, 7)));
    Held::new(vec![weak], move || drop(task))
}

/// A completed dependency cell's `Ready` value (`DependencyOutcome`). A cell
/// is never removed while its runtime exists, so its release is tested in
/// `src/heap/inventory.rs` (a dropped cell table).
fn dependency_ready(rt: &Runtime, def: u64) -> Vec<WeakHandle> {
    let request = rt.claim(key(rt, def)).unwrap();
    let (value, weak) = shareable(rt);
    let wakeups = rt
        .complete(key(rt, def), request.producer(), Completion::Ready(&value))
        .unwrap();
    assert!(wakeups.deliveries().is_empty());
    weak
}

/// A completed dependency cell's `Failed` error (`DependencyOutcome`).
fn dependency_failed(rt: &Runtime, def: u64) -> Vec<WeakHandle> {
    let request = rt.claim(key(rt, def)).unwrap();
    let (error, weak) = shareable(rt);
    let wakeups = rt
        .complete(key(rt, def), request.producer(), Completion::Failed(&error))
        .unwrap();
    assert!(wakeups.deliveries().is_empty());
    weak
}

/// An atom's current value: the atom object's edge. Released by replacing
/// the value; the atom itself stays rooted.
fn atom_current(rt: &Runtime) -> Held {
    let (value, weak) = shareable(rt);
    let atom = rt.new_atom(&value).unwrap();
    drop(value);
    let (rt2, atom2) = (rt.clone(), atom.clone());
    Held::new(weak, move || {
        let current = rt2.atom_load(&atom2).unwrap();
        let replacement = rt2.root(Primitive::Number(2.0));
        let swapped = rt2
            .atom_compare_and_set(&atom2, &current, &replacement)
            .unwrap();
        assert!(matches!(swapped, CasOutcome::Swapped));
    })
    .keeping(atom)
}

/// An atom's replaced (old) value, retained by a reader that loaded it
/// before the replacement (load-to-root).
fn atom_retained_old(rt: &Runtime) -> Held {
    let (old, weak) = shareable(rt);
    let atom = rt.new_atom(&old).unwrap();
    let reader = rt.atom_load(&atom).unwrap();
    let (new, _) = shareable(rt);
    let swapped = rt.atom_compare_and_set(&atom, &old, &new).unwrap();
    assert!(matches!(swapped, CasOutcome::Swapped));
    drop((old, new));
    Held::new(weak, move || drop(reader)).keeping(atom)
}

/// A closure environment: a captured immutable value.
fn closure_environment(rt: &Runtime) -> Held {
    let (task, captured) = spawn_holding(rt, "function f(o) { return function () { return o; }; }");
    let closure = run_to_end(&task);
    drop(task);
    let weak = vec![captured, closure.downgrade()];
    Held::new(weak, move || drop(closure))
}

/// A binding cell: a captured mutable binding, holding the value.
fn binding_cell(rt: &Runtime) -> Held {
    let (task, captured) = spawn_holding(
        rt,
        "function f(o) { let x = 0; const g = function () { return x; }; x = o; return g; }",
    );
    let closure = run_to_end(&task);
    drop(task);
    let weak = vec![captured, closure.downgrade()];
    Held::new(weak, move || drop(closure))
}

/// An open transient builder's draft, reachable from a suspended frame.
fn transient_builder(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(
        rt,
        &format!(
            "function f(o) {{ const b = __rt.vectorBuilder(); __rt.builderConj(b, o); o = 0; \
             {SPIN} return 0; }}"
        ),
    );
    run_prologue(&task);
    Held::new(vec![weak], move || drop(task))
}

/// An ordinary object's property values (clj-quickjs-114): the argument
/// is reachable only through a property of an object held by another
/// object's property, and the outer object is a local of a suspended frame.
/// Once the task is gone, all three are garbage.
fn object_property(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(
        rt,
        &format!("function f(o) {{ const outer = {{p: {{q: o}}}}; o = 0; {SPIN} return outer; }}"),
    );
    run_prologue(&task);
    Held::new(vec![weak], move || drop(task))
}

/// An array's elements (clj-quickjs-118): the argument is reachable only
/// as an element of arrays held by another array's dense and sparse
/// elements, and the outer array is a local of a suspended frame. Once the
/// task is gone, all of them are garbage.
fn array_element(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(
        rt,
        &format!(
            "function f(o) {{ const outer = [1, , [2, o]]; outer[5000] = [o]; o = 0; \
             {SPIN} return outer; }}"
        ),
    );
    run_prologue(&task);
    Held::new(vec![weak], move || drop(task))
}

/// A function's own properties (clj-quickjs-125): the argument is
/// reachable only through the property record of a returned function (an
/// object in its `x`, and its lazy `prototype` object), which captures
/// nothing. Once the result is released, all of them are garbage.
fn function_properties(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(
        rt,
        "function f(o) { const g = function () {}; g.x = {p: o}; g.prototype.q = o; return g; }",
    );
    let function = run_to_end(&task);
    drop(task);
    let weak = vec![weak, function.downgrade()];
    Held::new(weak, move || drop(function))
}

/// A pending `return` completion of a running `finally` block (handler
/// stack). The argument is overwritten, so only the completion holds it.
fn finally_return(rt: &Runtime) -> (Task, WeakHandle) {
    let (task, weak) = spawn_holding(
        rt,
        &format!("function f(o) {{ try {{ return o; }} finally {{ o = 0; {SPIN} }} }}"),
    );
    run_prologue(&task);
    (task, weak)
}

/// A pending `return` completion of a `finally` block suspended at a
/// pending `__rt.need` (the handler stack is saved across the wait).
fn finally_return_across_wait(rt: &Runtime) -> (Task, WeakHandle) {
    let (task, weak) = spawn_holding(
        rt,
        "function f(o) { try { return o; } finally { o = 0; const r = __rt.need(9, 1); } }",
    );
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(rt, 9)));
    (task, weak)
}

/// Settle every dependency `task` claimed with a primitive, then run it to
/// the end.
fn resume_to_end(rt: &Runtime, task: &Task) -> Rooted {
    for request in task.take_producer_requests() {
        let value = rt.root(Primitive::Number(0.0));
        let wakeups = rt
            .complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap();
        assert_eq!(wakeups.task_ids(), vec![task.id()]);
    }
    run_to_end(task)
}

/// A pending `throw` completion of a running `finally` block, caught by an
/// enclosing handler once the block ends.
fn finally_throw(rt: &Runtime) -> (Task, WeakHandle) {
    let (task, weak) = spawn_holding(
        rt,
        &format!(
            "function f(o) {{ try {{ try {{ throw o; }} finally {{ o = 0; {SPIN} }} }} \
             catch (e) {{ return e; }} }}"
        ),
    );
    run_prologue(&task);
    (task, weak)
}

fn handler_completion(rt: &Runtime, fixture: fn(&Runtime) -> (Task, WeakHandle)) -> Held {
    let (task, weak) = fixture(rt);
    Held::new(vec![weak], move || drop(task))
}

/// A resumable native frame (`__rt.reduce`): its accumulator, while its
/// callback runs. The callback clears every other reference first.
fn native_frame(rt: &Runtime) -> (Task, WeakHandle) {
    let (task, weak) = spawn_holding(
        rt,
        &format!(
            "function f(o) {{ return __rt.reduce(function (acc, x) {{ o = 0; acc = 0; {SPIN} \
             return 1; }}, o, __rt.vector(1)); }}"
        ),
    );
    run_prologue(&task);
    (task, weak)
}

/// A task in an executor's ready queue, never run (its frame holds its
/// argument). The executor is stopping, so nothing dispatches it.
fn ready_queue(rt: &Runtime) -> Held {
    let exec = ExecutorConfig::new().workers(1).start(rt).unwrap();
    exec.begin_shutdown();
    let (task, weak) = spawn_holding(rt, "function f(o) { return o; }");
    exec.submit(task).unwrap();
    Held::new(vec![weak], move || {
        let shutdown = exec.shutdown(TIMEOUT).unwrap();
        assert_eq!(shutdown.pending.len(), 1);
        drop(shutdown);
    })
}

/// A task parked by an executor on a pending dependency: the executor holds
/// the only task handle.
fn executor_parked(rt: &Runtime) -> Held {
    let exec = ExecutorConfig::new().workers(1).start(rt).unwrap();
    let (task, weak) = spawn_holding(rt, "function f(o) { const r = __rt.need(8, 1); return o; }");
    let id = task.id();
    exec.submit(task).unwrap();
    assert!(exec.wait_for(TIMEOUT, |events| {
        events.iter().any(|e| {
            e.task == Some(id) && matches!(e.kind, EventKind::Released(OutcomeKind::Waiting(_)))
        })
    }));
    Held::new(vec![weak], move || {
        let shutdown = exec.shutdown(TIMEOUT).unwrap();
        assert_eq!(shutdown.parked.len(), 1);
        drop(shutdown);
    })
}

// ---------------------------------------------------------------------------
// Retention and reclamation, one pair per holder.
// ---------------------------------------------------------------------------

#[test]
fn persistent_handle_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &persistent_handle(&rt));
}

#[test]
fn persistent_handle_reclaims() {
    let rt = Runtime::new();
    let held = persistent_handle(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn scope_local_retains() {
    let rt = Runtime::new();
    rt.scope(|s| {
        let local = s.new_object();
        let weak = s.downgrade(local);
        for _ in 0..2 {
            rt.collect()
                .expect("the collecting thread's own scope is fine");
            let again = s.upgrade(&weak).unwrap();
            assert!(s.same_object(local, again));
        }
    });
}

#[test]
fn scope_local_reclaims() {
    let rt = Runtime::new();
    let weak = rt.scope(|s| {
        let local = s.new_object();
        rt.collect().unwrap();
        s.downgrade(local)
    });
    assert_reclaimed(&rt, Held::new(vec![weak], || {}));
}

#[test]
fn persistent_collection_edges_retain() {
    let rt = Runtime::new();
    assert_retained(&rt, &persistent_collection(&rt));
}

#[test]
fn persistent_collection_edges_reclaim() {
    let rt = Runtime::new();
    let held = persistent_collection(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn task_result_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &task_result(&rt));
}

#[test]
fn task_result_reclaims() {
    let rt = Runtime::new();
    let held = task_result(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn suspended_frame_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &suspended_frame(&rt));
}

#[test]
fn suspended_frame_reclaims() {
    let rt = Runtime::new();
    let held = suspended_frame(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

/// A suspended task's frames are also released when it finishes, not only
/// when its handle is dropped.
#[test]
fn finished_task_frame_reclaims() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(&rt, &format!("function f(o) {{ {SPIN} return 0; }}"));
    run_prologue(&task);
    rt.request_collection().unwrap();
    assert!(weak.is_live());
    assert_eq!(run_to_end(&task).view(), ValueView::Number(0.0));
    assert_reclaimed(&rt, Held::new(vec![weak], || {}).keeping(task));
}

#[test]
fn pending_need_frame_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &pending_need(&rt));
}

#[test]
fn pending_need_frame_reclaims() {
    let rt = Runtime::new();
    let held = pending_need(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn dependency_outcome_retains() {
    let rt = Runtime::new();
    let ready = dependency_ready(&rt, 1);
    let failed = dependency_failed(&rt, 2);
    let held = Held::new(ready.iter().chain(&failed).cloned().collect(), || {});
    assert_retained(&rt, &held);
    // Readers still get the very objects that were published.
    let (CellStatus::Ready(value), CellStatus::Failed(error)) =
        (rt.cell_status(key(&rt, 1)), rt.cell_status(key(&rt, 2)))
    else {
        panic!("both cells are complete");
    };
    assert!(value.same_object(&ready[0].upgrade().unwrap()));
    assert!(error.same_object(&failed[0].upgrade().unwrap()));
}

#[test]
fn atom_current_value_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &atom_current(&rt));
}

#[test]
fn atom_current_value_reclaims() {
    let rt = Runtime::new();
    let held = atom_current(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

/// Dropping the atom's last root reclaims the atom and its value together.
#[test]
fn atom_object_reclaims_with_its_value() {
    let rt = Runtime::new();
    let (value, mut weak) = shareable(&rt);
    let atom = rt.new_atom(&value).unwrap();
    drop(value);
    weak.push(atom.downgrade());
    let held = Held::new(weak, move || drop(atom));
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn atom_retained_old_value_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &atom_retained_old(&rt));
}

#[test]
fn atom_retained_old_value_reclaims() {
    let rt = Runtime::new();
    let held = atom_retained_old(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn closure_environment_retains() {
    let rt = Runtime::new();
    let held = closure_environment(&rt);
    assert_retained(&rt, &held);
    // Calling the closure after the collections returns the captured object.
    let [captured, closure] = [&held.weak[0], &held.weak[1]].map(|w| w.upgrade().unwrap());
    let caller = rt
        .spawn(&compile("function g(h) { return h(); }"), &[&closure])
        .unwrap();
    assert!(run_to_end(&caller).same_object(&captured));
}

#[test]
fn closure_environment_reclaims() {
    let rt = Runtime::new();
    let held = closure_environment(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn binding_cell_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &binding_cell(&rt));
}

#[test]
fn binding_cell_reclaims() {
    let rt = Runtime::new();
    let held = binding_cell(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn transient_builder_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &transient_builder(&rt));
}

#[test]
fn transient_builder_reclaims() {
    let rt = Runtime::new();
    let held = transient_builder(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn object_property_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &object_property(&rt));
}

#[test]
fn object_property_reclaims() {
    let rt = Runtime::new();
    let held = object_property(&rt);
    assert_retained(&rt, &held);
    // The argument, the inner object and the outer object go together: no
    // ordinary object is left once the task is gone but `Object.prototype`
    // (the realm intrinsic the literals inherit from, clj-quickjs-117) and
    // `Array.prototype` (clj-quickjs-118), which the realm keeps for the
    // runtime's lifetime.
    let Held {
        weak,
        release,
        keep,
    } = held;
    assert_eq!(rt.heap_usage().objects.plain, 3 + 2);
    assert_reclaimed(&rt, Held::new(weak, release).keeping(keep));
    rt.request_collection().unwrap();
    assert_eq!(rt.heap_usage().objects.plain, 2);
}

/// The realm intrinsics (clj-quickjs-117, `RootCategory::Intrinsic`):
/// once a task has used them they survive collections with no task
/// running, keep their identities, and serve later tasks. The realm lives
/// as long as the runtime; the reclamation side is the crate-internal
/// `realm_intrinsics_reclaim_with_their_heap`.
#[test]
fn realm_intrinsics_retain() {
    let rt = Runtime::new();
    let exprs = [
        "Object",
        "Object.prototype",
        "Object.is",
        "Object.prototype.toString",
        "Object.prototype.valueOf",
    ];
    let weak: Vec<WeakHandle> = exprs
        .iter()
        .map(|e| {
            let task = rt
                .spawn(&compile(&format!("function f() {{ return {e}; }}")), &[])
                .unwrap();
            run_to_end(&task).downgrade()
        })
        .collect();
    // No task, root or scope is left: only the realm holds them.
    // (`Array.prototype` and its `toString` and `constructor` too,
    // clj-quickjs-118, and `defineProperty` and `getOwnPropertyDescriptor`, clj-quickjs-119.)
    assert_eq!(rt.heap_usage().objects.builtin, 8);
    assert_eq!(rt.heap_usage().objects.plain, 2);
    for _ in 0..2 {
        assert_eq!(rt.request_collection().unwrap().reclaimed, 0);
        assert_live(&weak);
    }
    // A later task sees the very same objects and can use them.
    let object = weak[0].upgrade().unwrap();
    let prototype = weak[1].upgrade().unwrap();
    let check = compile(
        "function f(o, p) { return o === Object && p === Object.prototype \
         && ({}).toString() === '[object Object]' && Object.is(o, o); }",
    );
    let task = rt.spawn(&check, &[&object, &prototype]).unwrap();
    assert_eq!(run_to_end(&task).view(), ValueView::Bool(true));
    drop((object, prototype, task));
    rt.request_collection().unwrap();
    assert_live(&weak);
}

/// An array's named (non-index) properties are traced too
/// (clj-quickjs-118): the argument is reachable only through `a.child` of
/// an array held by a suspended frame.
fn array_property(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(
        rt,
        &format!("function f(o) {{ const a = [1]; a.child = {{p: o}}; o = 0; {SPIN} return a; }}"),
    );
    run_prologue(&task);
    Held::new(vec![weak], move || drop(task))
}

#[test]
fn array_property_retains_and_reclaims() {
    let rt = Runtime::new();
    let held = array_property(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

/// An array literal under construction (clj-quickjs-118) lives on the
/// operand stack while its elements run: suspended at a pending
/// `__rt.need` inside an element, with its first element reachable only
/// from that partial array, it survives collections, resumes on another
/// thread, and finishes with each element evaluated once.
#[test]
fn array_literal_survives_wait_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { let n = 0; \
         const a = [o, , (n = n + 1, o = 0, __rt.need(9, 1)), 3]; \
         return __rt.vector(a[0], a.length === 4 && a[1] === undefined && a[2] === 0 \
           && a[3] === 3 && n === 1); }",
    );
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 9)));
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert_live(std::slice::from_ref(&weak));
    }
    for request in task.take_producer_requests() {
        let value = rt.root(Primitive::Number(0.0));
        let wakeups = rt
            .complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap();
        assert_eq!(wakeups.task_ids(), vec![task.id()]);
    }
    let result = thread::spawn(move || run_to_end(&task)).join().unwrap();
    let original = weak.upgrade().unwrap();
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        let first = s.nth(v, 0).unwrap();
        assert!(s.same_object(first, s.local(&original).unwrap()));
        assert_eq!(s.view(s.nth(v, 1).unwrap()), ValueView::Bool(true));
    });
}

#[test]
fn array_element_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &array_element(&rt));
}

#[test]
fn function_properties_retain() {
    let rt = Runtime::new();
    assert_retained(&rt, &function_properties(&rt));
}

#[test]
fn function_properties_reclaim() {
    let rt = Runtime::new();
    let held = function_properties(&rt);
    assert_retained(&rt, &held);
    // The argument, the record, the object in `x` and the prototype object
    // go with the function; the realm's two prototypes stay.
    let Held {
        weak,
        release,
        keep,
    } = held;
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.function), (4 + 2, 1));
    assert_reclaimed(&rt, Held::new(weak, release).keeping(keep));
    rt.request_collection().unwrap();
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.function), (2, 0));
}

#[test]
fn array_element_reclaims() {
    let rt = Runtime::new();
    let held = array_element(&rt);
    assert_retained(&rt, &held);
    // The argument and the three arrays go together; the realm's two
    // prototypes (`Object.prototype`, `Array.prototype`) stay.
    let Held {
        weak,
        release,
        keep,
    } = held;
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.array), (1 + 2, 3));
    assert_reclaimed(&rt, Held::new(weak, release).keeping(keep));
    rt.request_collection().unwrap();
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.array), (2, 0));
}

#[test]
fn handler_completion_retains() {
    for fixture in [finally_return, finally_throw, finally_return_across_wait] {
        let rt = Runtime::new();
        let (task, weak) = fixture(&rt);
        assert_retained(&rt, &Held::new(vec![weak.clone()], || {}));
        // The completion delivers the very object once the block ends.
        let result = resume_to_end(&rt, &task);
        assert!(result.same_object(&weak.upgrade().unwrap()));
    }
}

#[test]
fn handler_completion_reclaims() {
    for fixture in [finally_return, finally_throw, finally_return_across_wait] {
        let rt = Runtime::new();
        let held = handler_completion(&rt, fixture);
        assert_retained(&rt, &held);
        assert_reclaimed(&rt, held);
    }
}

#[test]
fn native_frame_retains() {
    let rt = Runtime::new();
    let (task, weak) = native_frame(&rt);
    assert_retained(&rt, &Held::new(vec![weak], || {}));
    assert_eq!(run_to_end(&task).view(), ValueView::Number(1.0));
}

#[test]
fn native_frame_reclaims() {
    let rt = Runtime::new();
    let (task, weak) = native_frame(&rt);
    let held = Held::new(vec![weak], move || drop(task));
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn ready_queue_retains() {
    let rt = Runtime::new();
    let held = ready_queue(&rt);
    assert_retained(&rt, &held);
    // Release through shutdown so the executor is not leaked.
    (held.release)();
}

#[test]
fn ready_queue_reclaims() {
    let rt = Runtime::new();
    let held = ready_queue(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn executor_parked_task_retains() {
    let rt = Runtime::new();
    let held = executor_parked(&rt);
    assert_retained(&rt, &held);
    (held.release)();
}

#[test]
fn executor_parked_task_reclaims() {
    let rt = Runtime::new();
    let held = executor_parked(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

// ---------------------------------------------------------------------------
// Running frames: reported by a run parked at a VM safepoint.
// ---------------------------------------------------------------------------

/// Opens once; waiting for it is bounded by [`TIMEOUT`].
#[derive(Default)]
struct Latch {
    open: Mutex<bool>,
    cv: Condvar,
}

impl Latch {
    fn open(&self) {
        *self.open.lock().unwrap() = true;
        self.cv.notify_all();
    }

    fn wait(&self) -> bool {
        let open = self.open.lock().unwrap();
        *self
            .cv
            .wait_timeout_while(open, TIMEOUT, |o| !*o)
            .unwrap()
            .0
    }
}

/// Host of spinning tasks: effect 0 answers "continue" until `stop` is set
/// (failing after a bound) and opens `entered` the first time.
#[derive(Clone, Default)]
struct Spin {
    entered: Arc<Latch>,
    stop: Arc<AtomicBool>,
}

struct SpinHost {
    spin: Spin,
    deadline: Instant,
}

impl SpinHost {
    fn new(spin: &Spin) -> SpinHost {
        SpinHost {
            spin: spin.clone(),
            deadline: Instant::now() + TIMEOUT,
        }
    }
}

impl Host for SpinHost {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        self.spin.entered.open();
        if Instant::now() > self.deadline {
            return Err("spin ran past its deadline".to_string());
        }
        Ok(Primitive::Bool(!self.spin.stop.load(Ordering::Acquire)))
    }
}

/// Allocates garbage while spinning; its argument is only in its frame.
const SPINNER: &str =
    "function f(o) { while (__rt.effect(0, 0)) { const g = __rt.vector(1, 2); } return o; }";

/// Run a spinner on its own thread and collect while it runs: the run parks
/// at a VM safepoint and reports its frame. Returns the result and the weak
/// handle once the spinner finished.
fn collect_around_running_frame(rt: &Runtime) -> (Rooted, WeakHandle) {
    let spin = Spin::default();
    let (task, weak) = spawn_holding(rt, SPINNER);
    let runner = {
        let spin = spin.clone();
        thread::spawn(move || {
            let mut host = SpinHost::new(&spin);
            complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap())
        })
    };
    assert!(spin.entered.wait(), "the spinner never started");
    for _ in 0..3 {
        let stats = rt.request_collection().unwrap();
        assert!(stats.roots >= 1, "the parked run reported its frame");
        assert!(weak.is_live(), "a running frame's value was reclaimed");
    }
    spin.stop.store(true, Ordering::Release);
    let result = runner.join().unwrap();
    (result, weak)
}

#[test]
fn running_frame_retains() {
    let rt = Runtime::new();
    let (result, weak) = collect_around_running_frame(&rt);
    assert!(result.same_object(&weak.upgrade().unwrap()));
}

#[test]
fn running_frame_reclaims() {
    let rt = Runtime::new();
    let (result, weak) = collect_around_running_frame(&rt);
    // The run ended, so its frames are gone; the result is the last holder.
    assert_reclaimed(&rt, Held::new(vec![weak], move || drop(result)));
}

// ---------------------------------------------------------------------------
// Multi-worker variant.
// ---------------------------------------------------------------------------

/// Checks, after every collection, that none of `weak` was reclaimed.
struct Watch {
    weak: Mutex<Vec<WeakHandle>>,
    collections: AtomicUsize,
    lost: AtomicUsize,
}

impl CollectionObserver for Watch {
    fn collected(&self, _: &CollectionReport) {
        self.collections.fetch_add(1, Ordering::AcqRel);
        let weak = self.weak.lock().unwrap();
        let lost = weak.iter().filter(|w| !w.is_live()).count();
        self.lost.fetch_add(lost, Ordering::AcqRel);
    }
}

/// Allocates `n` small vectors, dropping most of them.
const CHURN: &str = "function f(n) { let i = 0; let v = __rt.vector(); \
     while (i < n) { v = __rt.conj(v, __rt.vector(i)); \
     if (__rt.count(v) > 16) { v = __rt.vector(); } i = i + 1; } return i; }";

/// Every public holder at once, while two executor workers allocate and
/// collect: worker 0 runs an allocating spinner without a budget (its frame
/// is a running-frame holder), worker 1 runs allocating tasks, a budgeted
/// spinner and every collection request, so each collection stops worker 0
/// at a VM safepoint mid-run. Every held object survives every collection,
/// then is reclaimed once released. The crate-internal holders get the same
/// treatment in `src/heap/inventory.rs`.
#[test]
fn every_holder_retains_while_other_workers_allocate_and_collect() {
    const COLLECTIONS: usize = 12;
    let rt = Runtime::new();

    let mut helds = vec![
        persistent_handle(&rt),
        persistent_collection(&rt),
        task_result(&rt),
        suspended_frame(&rt),
        pending_need(&rt),
        atom_current(&rt),
        atom_retained_old(&rt),
        closure_environment(&rt),
        binding_cell(&rt),
        transient_builder(&rt),
        object_property(&rt),
        array_element(&rt),
        function_properties(&rt),
        handler_completion(&rt, finally_return),
        handler_completion(&rt, finally_throw),
        handler_completion(&rt, finally_return_across_wait),
        ready_queue(&rt),
        executor_parked(&rt),
    ];
    let (native_task, native_weak) = native_frame(&rt);
    helds.push(Held::new(vec![native_weak], move || drop(native_task)));
    let mut published = dependency_ready(&rt, 1);
    published.extend(dependency_failed(&rt, 2));

    // Worker 0 runs the allocating spinner without a budget for the whole
    // collection phase; worker 1 runs everything else, including every
    // collection request, so each collection must rendezvous with worker 0
    // stopped at a VM safepoint in the middle of its allocating run.
    let (allocator, collector) = (WorkerId::new(0), WorkerId::new(1));
    let spin = Spin::default();
    let unbudgeted: Arc<Mutex<Vec<TaskId>>> = Arc::default();
    let exec: Executor = {
        let spin = spin.clone();
        let unbudgeted = Arc::clone(&unbudgeted);
        ExecutorConfig::new()
            .workers(2)
            .budget(Budget::instructions(500))
            .host(move |_| Box::new(SpinHost::new(&spin)))
            .driver(move |guard, cx| {
                if unbudgeted.lock().unwrap().contains(&guard.task_id()) {
                    guard.run_with(Budget::UNLIMITED, &mut *cx.host)
                } else {
                    guard.run_with(cx.budget, &mut *cx.host)
                }
            })
            .start(&rt)
            .unwrap()
    };
    let mut spinners: Vec<(TaskId, WeakHandle)> = Vec::new();
    let (task, weak) = spawn_holding(&rt, SPINNER);
    spinners.push((task.id(), weak));
    unbudgeted.lock().unwrap().push(task.id());
    exec.submit_with(task, Placement::Worker(allocator))
        .unwrap();
    // The spinner has entered its run on worker 0 (only it runs there).
    assert!(spin.entered.wait(), "the allocating spinner never started");
    // A second, budgeted spinner yields and is requeued between collections.
    let (task, weak) = spawn_holding(&rt, SPINNER);
    spinners.push((task.id(), weak));
    exec.submit_with(task, Placement::Worker(collector))
        .unwrap();

    let mut all_weak: Vec<WeakHandle> = helds.iter().flat_map(|h| h.weak.clone()).collect();
    all_weak.extend(published.iter().cloned());
    all_weak.extend(spinners.iter().map(|(_, w)| w.clone()));
    let watch = Arc::new(Watch {
        weak: Mutex::new(all_weak.clone()),
        collections: AtomicUsize::new(0),
        lost: AtomicUsize::new(0),
    });
    let observer: Weak<dyn CollectionObserver> = Arc::downgrade(&watch) as _;
    rt.observe_collections(observer);

    let churn = compile(CHURN);
    rt.scope(|s| {
        // A host scope local, kept across the whole run: the scope is
        // released while this thread waits, so it never delays a collection.
        let local = s.new_object();
        let local_weak = s.downgrade(local);
        watch.weak.lock().unwrap().push(local_weak.clone());
        s.released(|| {
            let mut churners = Vec::new();
            for _ in 0..COLLECTIONS {
                let n = rt.root(Primitive::Number(2000.0));
                let task = rt.spawn(&churn, &[&n]).unwrap();
                churners.push(task.id());
                exec.submit_with(task, Placement::Worker(collector))
                    .unwrap();
                exec.schedule_collection(Placement::Worker(collector));
            }
            for id in churners {
                let outcome = exec.take_outcome(id, TIMEOUT).expect("churn finished");
                assert_eq!(complete(outcome).view(), ValueView::Number(2000.0));
            }
            let by_collector = |events: &[Event]| -> Vec<CollectionEvent> {
                events
                    .iter()
                    .filter_map(|e| match &e.kind {
                        EventKind::Collected(c) if c.requester == Some(collector) => {
                            Some(c.clone())
                        }
                        _ => None,
                    })
                    .collect()
            };
            let mut collections = Vec::new();
            assert!(
                exec.wait_for(TIMEOUT, |events| {
                    collections = by_collector(events);
                    collections.len() >= COLLECTIONS
                }),
                "worker 1's collections did not all run"
            );
            // The first request had to wait for the running worker; a later
            // one may find it still parked from the previous collection.
            assert!(collections[0].waited);
            for c in &collections {
                assert_eq!(
                    c.state_of(allocator),
                    Some(MutatorState::Safepoint),
                    "collection {}: the allocating worker was not stopped mid-run",
                    c.number
                );
            }
        });
        let again = s.upgrade(&local_weak).unwrap();
        assert!(s.same_object(local, again));
    });

    spin.stop.store(true, Ordering::Release);
    for (id, weak) in &spinners {
        let result = complete(exec.take_outcome(*id, TIMEOUT).expect("spinner finished"));
        assert!(result.same_object(&weak.upgrade().unwrap()));
    }
    assert!(watch.collections.load(Ordering::Acquire) >= COLLECTIONS);
    assert_eq!(
        watch.lost.load(Ordering::Acquire),
        0,
        "a held object was reclaimed"
    );
    for held in &helds {
        assert_retained(&rt, held);
    }
    assert_live(&published);

    // Then every holder lets go, and everything it held is reclaimed.
    drop(exec.shutdown(TIMEOUT).unwrap());
    watch.weak.lock().unwrap().clear();
    let weak: Vec<WeakHandle> = helds.iter().flat_map(|h| h.weak.clone()).collect();
    let mut keep = Vec::new();
    for held in helds {
        (held.release)();
        keep.push(held.keep);
    }
    assert_reclaimed(&rt, Held::new(weak, || {}).keeping(keep));
}
