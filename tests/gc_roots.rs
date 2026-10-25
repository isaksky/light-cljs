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

#[test]
fn array_buffer_reclaimed_when_unreachable() {
    let rt = Runtime::new();
    let code = clj_quickjs::frontend::compile_script("buffer", "new ArrayBuffer(1024)").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let RunOutcome::Complete(buffer) = task.run().unwrap() else {
        panic!("expected buffer")
    };
    let weak = buffer.downgrade();
    assert_eq!(rt.heap_usage().array_buffer_bytes, 1024);
    drop(task);
    rt.collect().unwrap();
    assert!(weak.upgrade().is_ok());
    drop(buffer);
    rt.collect().unwrap();
    assert!(matches!(weak.upgrade(), Err(HandleError::Reclaimed)));
    assert_eq!(rt.heap_usage().array_buffer_bytes, 0);
}

#[test]
fn array_buffer_properties_retained_and_reclaimed() {
    let rt = Runtime::new();
    // Synthetic fixtures, not light-cljs output. Test property, accessor,
    // and prototype edges, each as the child's only remaining strong path.
    for source in [
        "function f(o) { let b = new ArrayBuffer(4); b.p = o; return b; }",
        "function f(o) { let b = new ArrayBuffer(4); Object.defineProperty(b, 'p', {get: () => o}); return b; }",
        "function f(o) { let b = new ArrayBuffer(4); Object.setPrototypeOf(b, o); return b; }",
    ] {
        let (task, weak) = spawn_holding(&rt, source);
        let buffer = run_to_end(&task);
        drop(task);
        let held = Held::new(vec![weak, buffer.downgrade()], move || drop(buffer));
        assert_retained(&rt, &held);
        assert_reclaimed(&rt, held);
    }
}

/// Bound for every cross-thread wait.
const TIMEOUT: Duration = Duration::from_secs(30);

/// A loop long enough that a small budget always yields inside it.
const SPIN: &str = "let i = 0; while (i < 1000000) { i = i + 1; }";

/// Instructions a budgeted run gets before it yields: enough for every
/// fixture to reach its spin loop, far too few to leave it.
const PROLOGUE: u64 = 64;

/// Synthetic source, not light-cljs output. Symbols are immutable leaves
/// outside the traced heap: retain only a hash of identity across collection,
/// so neither a task nor a host symbol clone can keep the body alive here.
#[test]
fn symbol_registry_retains_unreferenced_symbols_across_collection() {
    use std::hash::{DefaultHasher, Hash, Hasher};

    struct Fingerprint(Option<u64>);
    impl Host for Fingerprint {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            let Primitive::Symbol(symbol) = call.arg else {
                return Err("expected symbol".into());
            };
            let mut hash = DefaultHasher::new();
            symbol.hash(&mut hash);
            self.0 = Some(hash.finish());
            Ok(Primitive::Undefined)
        }
    }

    let rt = Runtime::new();
    let code =
        compile_function("function register() { __rt.effect(128, Symbol.for('gc-registry')); }")
            .unwrap();
    let fingerprint = || {
        let mut host = Fingerprint(None);
        let task = rt.spawn(&code, &[]).unwrap();
        assert!(matches!(
            task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Complete(_)
        ));
        host.0.unwrap()
    };
    let before = fingerprint();
    rt.request_collection().unwrap();
    assert_eq!(before, fingerprint());
}

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

/// The method captures nothing: only its immutable home-object edge retains
/// the literal and the argument stored in it. The literal also points back
/// to the method, exercising collection of the unreachable cycle.
fn method_home_object(rt: &Runtime) -> Held {
    let (task, weak) = spawn_holding(rt, "function f(o) { return ({m() {}, value: o}).m; }");
    let method = run_to_end(&task);
    drop(task);
    Held::new(vec![weak, method.downgrade()], move || drop(method))
}

#[test]
fn method_home_object_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &method_home_object(&rt));
}

#[test]
fn method_home_object_reclaims() {
    let rt = Runtime::new();
    let held = method_home_object(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn accessor_slots_and_home_objects_retain_and_reclaim() {
    // Synthetic fixtures, not light-cljs output. The captured argument is
    // reachable only through the getter/setter or its defining home object.
    for src in [
        "function f(o) { return {get z() { return o; }}; }",
        "function f(o) { return {set z(v) { o; }}; }",
        "function f(o) { let a = []; Object.defineProperty(a, 3, {get: () => o}); return a; }",
        "function f(o) { let a = []; Object.defineProperty(a, 3, {set: v => o}); return a; }",
        "function f(o) { let a = {get z() {}, value: o}; return Object.getOwnPropertyDescriptor(a, 'z').get; }",
        "function f(o) { let a = {set z(v) {}, value: o}; return Object.getOwnPropertyDescriptor(a, 'z').set; }",
    ] {
        let rt = Runtime::new();
        let (task, weak) = spawn_holding(&rt, src);
        let object = run_to_end(&task);
        drop(task);
        let held = Held::new(vec![weak, object.downgrade()], move || drop(object));
        assert_retained(&rt, &held);
        assert_reclaimed(&rt, held);
    }
}

/// Each bound operand is the only path to an object: target closure,
/// bound receiver, bound argument, and own-property record. The immutable
/// metadata is traced too.
fn bound_function(rt: &Runtime) -> Held {
    let maker = compile(
        "function make(o) { const receiver = {}; const arg = {}; const property = {}; const target = function target(a) { return __rt.vector(o, this, a); }; const bound = target.bind(receiver, arg); bound.x = property; return __rt.vector(bound, target, receiver, arg, property); }",
    );
    let object = rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    });
    let weak_object = object.downgrade();
    let task = rt.spawn(&maker, &[&object]).unwrap();
    let result = run_to_end(&task);
    let (bound, mut weak) = rt.scope(|s| {
        let result = s.local(&result).unwrap();
        let bound = s.nth(result, 0).unwrap();
        let weak = (1..5)
            .map(|i| {
                let value = s.nth(result, i).unwrap();
                s.downgrade(value)
            })
            .collect::<Vec<_>>();
        (s.persist(bound), weak)
    });
    drop((task, result, object));
    weak.extend([weak_object, bound.downgrade()]);
    Held::new(weak, move || drop(bound))
}

#[test]
fn bound_function_retains() {
    let rt = Runtime::new();
    assert_retained(&rt, &bound_function(&rt));
}

#[test]
fn bound_function_reclaims() {
    let rt = Runtime::new();
    let held = bound_function(&rt);
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
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
    // (the realm intrinsic the literals inherit from, clj-quickjs-117),
    // `Array.prototype` (clj-quickjs-118), `Number.prototype` and
    // `String.prototype` (clj-quickjs-73), `Symbol.prototype` (clj-quickjs-129), `Reflect` (clj-quickjs-102)
    // `Math` (clj-quickjs-152), `Error.prototype` (clj-quickjs-228),
    // the seven NativeError prototypes (clj-quickjs-229),
    // SuppressedError.prototype (clj-quickjs-234), AggregateError.prototype (clj-quickjs-317), JSON (clj-quickjs-159),
    // Map.prototype (clj-quickjs-169), WeakMap.prototype (clj-quickjs-173), WeakRef.prototype (clj-quickjs-176),
    // FinalizationRegistry.prototype (clj-quickjs-177),
    // ArrayBuffer.prototype (clj-quickjs-165),
    // Intl (clj-quickjs-209) and Intl.Locale.prototype (clj-quickjs-210),
    // Date.prototype (clj-quickjs-179), RegExp.prototype (clj-quickjs-188),
    // Intl.NumberFormat.prototype (clj-quickjs-213),
    // Intl.DateTimeFormat.prototype (clj-quickjs-215),
    // Intl.ListFormat.prototype (clj-quickjs-220),
    // Intl.RelativeTimeFormat.prototype (clj-quickjs-221),
    // Intl.Segmenter, Segments and Segment Iterator prototypes (clj-quickjs-222),
    // Intl.DisplayNames.prototype (clj-quickjs-223),
    // Intl.DurationFormat.prototype (clj-quickjs-225),
    // Intl.PluralRules.prototype (clj-quickjs-212), the three iterator
    // prototypes (clj-quickjs-237), Generator.prototype (clj-quickjs-243), and
    // the typed-array prototypes (clj-quickjs-166, clj-quickjs-167),
    // which the realm keeps for the
    // runtime's lifetime, plus Array's unscopables record (clj-quickjs-110).
    let Held {
        weak,
        release,
        keep,
    } = held;
    assert_eq!(rt.heap_usage().objects.plain, 3 + 65);
    assert_reclaimed(&rt, Held::new(weak, release).keeping(keep));
    rt.request_collection().unwrap();
    assert_eq!(rt.heap_usage().objects.plain, 65);
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
        "Object.assign",
        "Object.entries",
        "Object.values",
        "Object.fromEntries",
        "Object.defineProperties",
        "Object.prototype.toString",
        "Object.prototype.valueOf",
        "Array.prototype.push",
        "String.prototype.charAt",
        "String.prototype.charCodeAt",
        "String.prototype.codePointAt",
        "String.fromCharCode",
        "String.fromCodePoint",
        "String.raw",
        "String.prototype.indexOf",
        "String.prototype.lastIndexOf",
        "String.prototype.substring",
        "String.prototype.slice",
        "String.prototype.concat",
        "String.prototype.padStart",
        "String.prototype.padEnd",
        "String.prototype.split",
        "String.prototype.replace",
        "String.prototype.replaceAll",
        "String.prototype.match",
        "String.prototype.matchAll",
        "String.prototype.search",
        "RegExp.prototype[Symbol.match]",
        "RegExp.prototype[Symbol.matchAll]",
        "RegExp.prototype[Symbol.search]",
        "RegExp.prototype[Symbol.replace]",
        "RegExp.prototype[Symbol.split]",
        "Object.getOwnPropertyDescriptor(RegExp, Symbol.species).get",
        "Object.getPrototypeOf(''.matchAll(/x/g))",
        "Object.getPrototypeOf(''.matchAll(/x/g)).next",
        "Symbol",
        "Symbol.for",
        "Symbol.keyFor",
        "Symbol.prototype",
        "Symbol.prototype.toString",
        "Symbol.prototype.valueOf",
        "Object.getOwnPropertyDescriptor(Symbol.prototype, 'description').get",
        "Symbol.prototype[Symbol.toPrimitive]",
        "isNaN",
        "isFinite",
        "Number.isNaN",
        "Number.isFinite",
        "Number.isInteger",
        "Number.isSafeInteger",
        "parseInt",
        "parseFloat",
        "Math",
        "Math.floor",
        "Math.ceil",
        "Math.abs",
        "Math.imul",
        "Math.fround",
        "Math.hypot",
        "Math.pow",
        "Math.sqrt",
        "Math.log2",
        "Math.sumPrecise",
        "Array.prototype.join",
        "Uint8Array.prototype.set",
        "Uint8Array.prototype.fill",
        "Uint8Array.prototype.join",
        "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').get",
        "Object.getOwnPropertyDescriptor(Object.prototype, '__proto__').set",
        "TypeError",
        "TypeError.prototype",
        "RangeError",
        "RangeError.prototype",
        "ReferenceError",
        "ReferenceError.prototype",
        "SyntaxError",
        "SyntaxError.prototype",
        "EvalError",
        "EvalError.prototype",
        "URIError",
        "URIError.prototype",
        "InternalError",
        "InternalError.prototype",
        "JSON",
        "JSON.parse",
        "JSON.stringify",
        "Map",
        "Map.prototype",
        "Map.prototype.set",
        "Map.prototype.get",
        "Map.prototype.has",
        "Map.prototype.delete",
        "Map.prototype.clear",
        "Object.getOwnPropertyDescriptor(Map.prototype, 'size').get",
        "Intl",
        "Intl.Locale.prototype.maximize",
        "Intl.Locale.prototype.minimize",
        "RegExp",
        "RegExp.prototype",
        "RegExp.prototype.exec",
        "RegExp.prototype.test",
        "RegExp.prototype.toString",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'source').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'flags').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'hasIndices').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'global').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'ignoreCase').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'multiline').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'dotAll').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'unicode').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'unicodeSets').get",
        "Object.getOwnPropertyDescriptor(RegExp.prototype, 'sticky').get",
        "BigInt.prototype.toString",
        "BigInt.prototype.valueOf",
        "Intl.PluralRules",
        "Intl.PluralRules.prototype",
        "Intl.PluralRules.supportedLocalesOf",
        "Intl.PluralRules.prototype.select",
        "Intl.PluralRules.prototype.resolvedOptions",
        "Intl.NumberFormat",
        "Intl.NumberFormat.prototype",
        "Intl.NumberFormat.supportedLocalesOf",
        "Intl.NumberFormat.prototype.resolvedOptions",
        "Object.getOwnPropertyDescriptor(Intl.NumberFormat.prototype, 'format').get",
        "Date.parse",
        "Map.prototype.forEach",
        "Set",
        "WeakSet",
        "Set.prototype",
        "WeakSet.prototype",
        "Set.prototype.add",
        "Set.prototype.has",
        "Set.prototype.delete",
        "Set.prototype.clear",
        "Object.getOwnPropertyDescriptor(Set.prototype, 'size').get",
        "Set.prototype.forEach",
        "Set.prototype.values",
        "Set.prototype.keys",
        "Set.prototype.entries",
        "Set.prototype[Symbol.iterator]",
        "Object.getPrototypeOf(new Set().values())",
        "Object.getPrototypeOf(new Set().values()).next",
        "WeakSet.prototype.add",
        "WeakSet.prototype.has",
        "WeakSet.prototype.delete",
        "Array.from",
        "Array.prototype.sort",
        "Number.prototype.toExponential",
        "Number.prototype.toPrecision",
        "Intl.DateTimeFormat",
        "Intl.DateTimeFormat.prototype",
        "Intl.DateTimeFormat.supportedLocalesOf",
        "Intl.DateTimeFormat.prototype.resolvedOptions",
        "Object.getOwnPropertyDescriptor(Intl.DateTimeFormat.prototype, 'format').get",
        "eval",
        "Intl.ListFormat",
        "Intl.ListFormat.prototype",
        "Intl.ListFormat.supportedLocalesOf",
        "Intl.ListFormat.prototype.resolvedOptions",
        "Intl.ListFormat.prototype.format",
        "Intl.ListFormat.prototype.formatToParts",
        "Intl.RelativeTimeFormat",
        "Intl.RelativeTimeFormat.prototype",
        "Intl.RelativeTimeFormat.supportedLocalesOf",
        "Intl.RelativeTimeFormat.prototype.resolvedOptions",
        "Intl.RelativeTimeFormat.prototype.format",
        "Intl.RelativeTimeFormat.prototype.formatToParts",
        "Boolean",
        "Boolean.prototype",
        "Boolean.prototype.toString",
        "Boolean.prototype.valueOf",
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
    // Each completed task created its own global binding overlay (-104).
    // Reclaim those objects, their self-cycles, seventeen temporary accessor
    // descriptors, two temporary Set/cursor pairs, one species descriptor and
    // two temporary RegExp/matcher/iterator triples; intrinsics stay alive.
    assert_eq!(rt.request_collection().unwrap().reclaimed, exprs.len() + 28);
    assert_live(&weak);
    // No task, root or scope is left: only the realm holds them.
    // (`Array.prototype` and its `toString` and `constructor` too,
    // clj-quickjs-118, `defineProperty` and `getOwnPropertyDescriptor`, clj-quickjs-119,
    // `getPrototypeOf`, `setPrototypeOf` and `create`, clj-quickjs-121, and
    // `preventExtensions`, `isExtensible`, `freeze` and `isFrozen`, clj-quickjs-122,
    // and `keys` and `getOwnPropertyNames`, clj-quickjs-123, and `Number`,
    // `String`, their prototypes and their `toString` and `valueOf`,
    // clj-quickjs-73, and `Function.prototype` with `call` and `apply`,
    // `Function`, `Reflect` and `Reflect.apply`, clj-quickjs-102,
    // `Function.prototype.bind`, clj-quickjs-103, and
    // `Array.prototype.push`, clj-quickjs-155, and the five string code-unit
    // methods, clj-quickjs-143, and both substring-search methods,
    // clj-quickjs-144, five extraction/combination methods, clj-quickjs-145,
    // six number predicates, clj-quickjs-147, two parsing functions,
    // clj-quickjs-148, Math with six numeric methods, clj-quickjs-152,
    // the two __proto__ accessors, clj-quickjs-261, Array.prototype.join,
    // clj-quickjs-156, Error and its prototype/toString, clj-quickjs-228,
    // String.prototype.split, clj-quickjs-157, Array.isArray, clj-quickjs-153,
    // seven NativeError constructors and prototypes, clj-quickjs-229,
    // Math.pow, clj-quickjs-164, JSON with parse, clj-quickjs-159, and
    // Symbol, clj-quickjs-127. Two unary Math functions: clj-quickjs-205;
    // String.raw: clj-quickjs-58; Symbol.for/keyFor: clj-quickjs-128.
    // JSON.stringify: clj-quickjs-160.
    // Map adds its constructor, five methods and size getter (-169).
    // JSON.stringify: clj-quickjs-160; ArrayBuffer: clj-quickjs-165.
    // Integer typed arrays add 12 built-ins and 8 prototypes (-166).
    // Intl adds its ordinary namespace (-209).
    // Locale adds one prototype and fourteen built-ins (-210).
    // Locale likely-subtag methods add two built-in functions (-211).
    // Date adds one prototype and five built-ins (-179).
    // Floating-point typed arrays add three constructors and prototypes (-167).
    // Date ISO formatting adds one built-in method (-180).
    // RegExp adds one prototype and fourteen built-ins (-188).
    // BigInt adds one constructor function and one prototype (-201).
    // UTC field access adds fifteen Date methods (-181).
    // Typed-array set/fill/join append three built-ins (-168).
    // String replacement adds two built-in functions (-189).
    // BigInt conversion adds two built-in methods (-202).
    // Promise adds one prototype and five built-ins (-246).
    // Local Date fields and host inputs add seventeen built-ins (-182).
    // NumberFormat adds one prototype and five built-ins (-213).
    // Date non-locale formatting adds four built-ins (-183).
    // Iteration adds three prototypes and six built-ins (-237).
    // BigInt width wrapping adds two built-in methods (-204).
    // PluralRules adds one prototype and four built-ins (-212).
    // ISO date parsing adds one built-in function (-184).
    // Map forEach adds one built-in method (-170).
    // Array.from adds one built-in function (-207).
    // Generators add one prototype and its next built-in (-243).
    // Array.prototype.sort adds one built-in function (-208).
    // Collator adds one prototype and five built-in functions (-218).
    // Number.prototype.toFixed adds one built-in function (-150).
    // WeakMap adds one prototype and five built-in functions (-173).
    // Number.prototype.toExponential/toPrecision add two built-ins (-151).
    // DateTimeFormat adds one prototype and five built-ins (-215).
    // Indirect eval adds one built-in function (-106).
    // Math.sumPrecise adds one built-in function (-158).
    // WeakRef adds one prototype and two built-in functions (-176).
    // Symbol.prototype adds one prototype and four built-in functions (-129).
    // FinalizationRegistry adds one prototype and three built-in functions (-177).
    // ListFormat adds one prototype and five built-in functions (-220).
    // RelativeTimeFormat adds one prototype and five built-in functions (-221).
    // Segmenter adds three prototypes and seven built-in functions (-222).
    // DisplayNames adds one prototype and four built-in functions (-223).
    // DurationFormat adds one prototype and five built-in functions (-225).
    // SuppressedError adds one prototype and one constructor (-234).
    // DisposableStack adds one prototype and seven built-ins (-239).
    // AsyncDisposableStack adds one prototype and eight built-ins (-241).
    // Array unscopables adds one ordinary record (-110).
    // Seven array mutators and species getter add eight built-ins (-296).
    // Generator return/throw add two built-in methods (-293).
    // Eight array search methods add eight built-ins (-297).
    // Eight array callback methods add eight built-ins (-298).
    // Async iteration adds two prototypes and four built-ins (-313).
    // Array copies, iterator modes and Array.of add nine built-ins (-299).
    // Set and WeakSet add three prototypes and fourteen built-ins (-295).
    // Boolean adds one prototype and three built-ins (-302).
    // Fourteen String prototype methods (-300).
    // Promise methods, species and AggregateError add eight built-ins and one prototype (-317).
    // Five Object statics add five built-ins (-318).
    // RegExp symbol protocols add ten built-ins and one iterator prototype (-301).
    // The remaining Math methods add twenty-seven built-ins (-319).
    assert_eq!(rt.heap_usage().objects.builtin, 405);
    assert_eq!(rt.heap_usage().objects.plain, 65);
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

/// Synthetic fixture, not light-cljs output: constructor elements are
/// traced while the result survives a wait, collection and migration.
#[test]
fn constructed_array_survives_wait_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { const a = new Array(o, 2); o = 0; \
         __rt.need(9, 1); return __rt.vector(a[0], \
         a.length === 2 && a[1] === 2 && Array.isArray(a)); }",
    );
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 9)));
    rt.request_collection().unwrap();
    assert_live(std::slice::from_ref(&weak));
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
        assert!(s.same_object(s.nth(v, 0).unwrap(), s.local(&original).unwrap()));
        assert_eq!(s.view(s.nth(v, 1).unwrap()), ValueView::Bool(true));
    });
    drop((result, original));
    rt.collect().unwrap();
    assert!(weak.upgrade().is_err());
}

/// A property update (clj-quickjs-76) waiting in its computed key: the
/// base, an object literal holding the argument, is then reachable only
/// from the suspended frame's operand stack (the argument's local was
/// cleared). It survives collections, resumes on another thread, and the
/// update finishes with the key evaluated once and the old value given.
#[test]
fn property_update_base_survives_wait_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { let n = 0; \
         const r = {v: o, k: 5}[(n = n + 1, o = 0, __rt.need(9, 1), 'k')]++; \
         return __rt.vector(r, n); }",
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
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        assert_eq!(s.view(s.nth(v, 0).unwrap()), ValueView::Number(5.0));
        assert_eq!(s.view(s.nth(v, 1).unwrap()), ValueView::Number(1.0));
    });
}

/// A compound assignment on a property (clj-quickjs-260) waiting in its
/// right operand: the base, an object literal holding the argument, is
/// then reachable only from the suspended frame's operand stack (the
/// argument's local was cleared). It survives collections, resumes on
/// another thread, and finishes with the key evaluated once and the value
/// read before the wait.
#[test]
fn property_compound_assignment_base_survives_wait_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { let n = 0; \
         const r = {v: o, k: 5}[(n = n + 1, o = 0, 'k')] *= __rt.need(9, 1) + 2; \
         return __rt.vector(r, n); }",
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
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        assert_eq!(s.view(s.nth(v, 0).unwrap()), ValueView::Number(10.0));
        assert_eq!(s.view(s.nth(v, 1).unwrap()), ValueView::Number(1.0));
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
    // go with the function; the realm's 65 ordinary objects stay (-301).
    let Held {
        weak,
        release,
        keep,
    } = held;
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.function), (4 + 65, 1));
    assert_reclaimed(&rt, Held::new(weak, release).keeping(keep));
    rt.request_collection().unwrap();
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.function), (65, 0));
}

#[test]
fn array_element_reclaims() {
    let rt = Runtime::new();
    let held = array_element(&rt);
    assert_retained(&rt, &held);
    // The argument and the three arrays go together; the realm's
    // prototypes (`Object`, `Array`, `Number`, `String`, `Symbol`, `Error`, seven
    // NativeErrors, `Map`, `WeakMap`, `WeakRef`, `FinalizationRegistry`, `DisposableStack`, `ArrayBuffer`, `Intl.Locale`, `Intl.NumberFormat`,
    // `Date`, `RegExp`, `BigInt`, `Generator`, the three iterator prototypes and the eleven
    // typed-array prototypes), `Reflect`, `Math`, `JSON` and `Intl` stay.
    let Held {
        weak,
        release,
        keep,
    } = held;
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.array), (1 + 65, 3));
    assert_reclaimed(&rt, Held::new(weak, release).keeping(keep));
    rt.request_collection().unwrap();
    let usage = rt.heap_usage().objects;
    assert_eq!((usage.plain, usage.array), (65, 0));
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
        method_home_object(&rt),
        bound_function(&rt),
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

/// Complete every dependency request `task` made with a ready `0`.
fn complete_requests(rt: &Runtime, task: &Task) {
    for request in task.take_producer_requests() {
        let value = rt.root(Primitive::Number(0.0));
        let wakeups = rt
            .complete(request.key(), request.producer(), Completion::Ready(&value))
            .unwrap();
        assert_eq!(wakeups.task_ids(), vec![task.id()]);
    }
}

/// Prototype links (clj-quickjs-121) are traced edges: the argument (an
/// embedder object, which every task may read) is reachable only as the
/// last ancestor of a chain built with `Object.create`, a literal's
/// `__proto__:`, the `__proto__` setter and `Object.setPrototypeOf`, whose
/// first object is a local of a task waiting on a dependency. It survives
/// collections, the task resumes on another thread, and the inherited read
/// and the chain's identity are intact.
#[test]
fn prototype_links_survive_wait_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { const a = Object.create(o); a.k = 7; const b = {__proto__: a}; \
         const c = {}; c.__proto__ = b; const d = Object.create(null); \
         Object.setPrototypeOf(d, c); o = 0; const r = __rt.need(9, 1); \
         const p = Object.getPrototypeOf; \
         return __rt.vector(p(p(p(p(d)))), d.k === 7 && p(d) === c && p(c) === b && p(b) === a); }",
    );
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 9)));
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert_live(std::slice::from_ref(&weak));
    }
    complete_requests(&rt, &task);
    let result = thread::spawn(move || run_to_end(&task)).join().unwrap();
    let original = weak.upgrade().unwrap();
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        let first = s.nth(v, 0).unwrap();
        assert!(s.same_object(first, s.local(&original).unwrap()));
        assert_eq!(s.view(s.nth(v, 1).unwrap()), ValueView::Bool(true));
    });
}

/// Replacing a prototype link (clj-quickjs-121) releases the old
/// prototype: once every link to the argument (made by `Object.create`, a
/// literal's `__proto__:` and the `__proto__` setter) is replaced with
/// `Object.setPrototypeOf`, a collection reclaims it while the task, which still holds
/// the objects, waits; the objects themselves stay.
#[test]
fn replaced_prototypes_are_reclaimed() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { const a = Object.create(o); const c = {__proto__: o}; const e = {}; \
         e.__proto__ = o; o = 0; const r1 = __rt.need(9, 1); \
         Object.setPrototypeOf(a, null); Object.setPrototypeOf(c, {}); Object.setPrototypeOf(e, a); \
         c.__proto__ = 1; \
         const r2 = __rt.need(10, 1); \
         return Object.getPrototypeOf(a) === null && Object.getPrototypeOf(e) === a && \
           c.__proto__ !== 1 && Object.getPrototypeOf(c).__proto__ === Object.prototype; }",
    );
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 9)));
    rt.request_collection().unwrap();
    assert_live(std::slice::from_ref(&weak));
    complete_requests(&rt, &task);
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 10)));
    let before = rt.heap_usage().objects.plain;
    rt.request_collection().unwrap();
    assert_stale(&rt, std::slice::from_ref(&weak));
    // Only the argument went: `a`, `c`, `e` and `c`'s new prototype stay.
    assert_eq!(rt.heap_usage().objects.plain, before - 1);
    complete_requests(&rt, &task);
    let result = run_to_end(&task);
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        assert_eq!(s.view(v), ValueView::Bool(true));
    });
}

/// A `for`-`in` enumerator (clj-quickjs-85) is task-owned frame state: the
/// enumerated object, an object literal holding the argument, is reachable
/// only from the enumerator in the loop's scratch local (the argument's
/// local is cleared in the body). Run in small budget slices with a
/// collection after each, the loop keeps the object (and through it the
/// argument) alive and visits the remaining keys in order.
#[test]
fn for_in_enumerator_survives_budget_yields_and_collections() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { let s = ''; \
         for (const k in {b: o, a: 1, 2: 1, c: 1}) { o = 0; \
           let j = 0; while (j < 100) { j = j + 1; } s = s + k; } \
         return s === '2bac'; }",
    );
    let mut yields = 0;
    let result = loop {
        match task
            .run_with(Budget::instructions(50), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                yields += 1;
                assert!(yields < 1000, "the loop did not finish");
                rt.request_collection().unwrap();
                // Each key's spin alone takes more than 600 instructions,
                // so these slices end inside the loop.
                if yields < 12 {
                    assert_live(std::slice::from_ref(&weak));
                }
            }
            other => break complete(other),
        }
    };
    assert!(yields > 20, "{yields} yields");
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        assert_eq!(s.view(v), ValueView::Bool(true));
    });
    // The loop is over: nothing holds the argument any more.
    drop(task);
    rt.request_collection().unwrap();
    assert_stale(&rt, std::slice::from_ref(&weak));
}

/// Synthetic fixtures, not light-cljs output. An abandoned enumerator must
/// release its object even while its frame remains alive at a dependency wait.
#[test]
fn for_in_abrupt_exit_releases_enumerator() {
    for (case, source) in [
        (
            "caught exception",
            "function f(o) { try { for (let k in {a: o}) { o = 0; __rt.need(9, 1); throw 1; } } \
             catch (e) { if (e !== 1) throw e; } __rt.need(10, 1); return true; }",
        ),
        (
            "outer labelled break",
            "function f(o) { out: { for (const k in {a: o}) { o = 0; __rt.need(9, 1); break out; } } \
             __rt.need(10, 1); return true; }",
        ),
        (
            "outer labelled continue",
            "function f(o) { let n = 0; out: while (n < 1) { n++; \
             for (let k in {a: o}) { o = 0; __rt.need(9, 1); continue out; } } \
             __rt.need(10, 1); return n === 1; }",
        ),
        (
            "own break",
            "function f(o) { for (let k in {a: o}) { o = 0; __rt.need(9, 1); break; } \
             __rt.need(10, 1); return true; }",
        ),
        (
            "own continue then exhaustion",
            "function f(o) { let s = ''; loop: for (let k in {a: o, b: 0}) { o = 0; \
             if (k === 'a') { __rt.need(9, 1); continue loop; } s += k; } \
             __rt.need(10, 1); return s === 'b'; }",
        ),
        (
            "return through outer finally",
            "function f(o) { try { for (let k in {a: o}) { o = 0; __rt.need(9, 1); return true; } } \
             finally { __rt.need(10, 1); } }",
        ),
        (
            "throwing member target",
            "function f(o) { const target = { set k(v) { o = 0; __rt.need(9, 1); throw 1; } }; \
             try { for (target.k in {a: o}) {} } catch (e) { if (e !== 1) throw e; } \
             __rt.need(10, 1); return true; }",
        ),
    ] {
        let rt = Runtime::new();
        let (task, weak) = spawn_holding(&rt, source);
        assert!(
            matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 9)),
            "{case}: did not suspend inside the loop"
        );
        rt.request_collection().unwrap();
        assert_live(std::slice::from_ref(&weak));
        complete_requests(&rt, &task);
        assert!(
            matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 10)),
            "{case}: did not suspend after leaving the loop"
        );
        rt.request_collection().unwrap();
        assert!(
            !weak.is_live(),
            "{case}: abandoned enumerator kept its object"
        );
        assert_stale(&rt, std::slice::from_ref(&weak));
        complete_requests(&rt, &task);
        assert_eq!(run_to_end(&task).view(), ValueView::Bool(true), "{case}");
    }
}

/// A `for`-`in` body (clj-quickjs-85) waiting on `__rt.need` with the
/// enumerated object reachable only from the enumerator, and closures over
/// two iterations' bindings live: the task survives collections, resumes
/// on another thread, visits the remaining keys in order, and each closure
/// returns its own key.
#[test]
fn for_in_wait_survives_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { let s = ''; const b = __rt.vectorBuilder(); \
         for (let k in {x: o, y: 1, z: 1}) { o = 0; __rt.builderConj(b, () => k); \
           if (k === 'y') __rt.need(9, 1); s = s + k; } \
         const v = __rt.freeze(b); \
         return s === 'xyz' && __rt.nth(v, 0)() + __rt.nth(v, 1)() + __rt.nth(v, 2)() === 'xyz'; }",
    );
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 9)));
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert_live(std::slice::from_ref(&weak));
    }
    complete_requests(&rt, &task);
    let result = thread::spawn(move || run_to_end(&task)).join().unwrap();
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        assert_eq!(s.view(v), ValueView::Bool(true));
    });
}

/// A `for`-`in` loop walking the prototype chain (clj-quickjs-86) waits on
/// `__rt.need` while it is on a prototype that the body has unlinked from
/// the enumerated object (`Object.setPrototypeOf(x, null)`): the prototype,
/// which holds the argument, is then reachable only from the enumerator.
/// It survives collections, the task resumes on another thread and visits
/// the prototype's remaining keys, and once the loop is over the
/// prototype and the argument are reclaimed.
#[test]
fn for_in_prototype_walk_survives_collection_and_migration() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        "function f(o) { let s = ''; \
         const x = {a: 1, q: 1, __proto__: {p: o, q: 2, r: 1, s: 1}}; \
         for (const k in x) { \
           if (k === 'p') { Object.setPrototypeOf(x, null); o = 0; __rt.need(9, 1); } \
           s = s + k; } \
         const r = __rt.need(10, 1); \
         return s === 'aqprs'; }",
    );
    let outcome = task.run().unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 9)));
    for _ in 0..2 {
        rt.request_collection().unwrap();
        assert_live(std::slice::from_ref(&weak));
    }
    complete_requests(&rt, &task);
    let (task, outcome) = thread::spawn(move || {
        let outcome = task.run().unwrap();
        (task, outcome)
    })
    .join()
    .unwrap();
    assert!(matches!(outcome, RunOutcome::Waiting(k) if k == key(&rt, 10)));
    // The loop is over: its enumerator, the unlinked prototype and the
    // argument are garbage although the task lives on.
    rt.request_collection().unwrap();
    assert_stale(&rt, std::slice::from_ref(&weak));
    complete_requests(&rt, &task);
    let result = run_to_end(&task);
    rt.scope(|s| {
        let v = s.local(&result).unwrap();
        assert_eq!(s.view(v), ValueView::Bool(true));
    });
}

/// Truncation clears both dense and sparse element edges while keeping the
/// array alive in a suspended frame (clj-quickjs-154). Synthetic fixtures,
/// not light-cljs output.
#[test]
fn array_length_truncation_reclaims_elements() {
    for index in [2, 4294967294_u32] {
        let rt = Runtime::new();
        let (task, weak) = spawn_holding(
            &rt,
            &format!(
                "function f(o) {{ const a = [1]; a[{index}] = o; o = 0; \
                 const r1 = __rt.need(9, 1); a.length = 1; \
                 const r2 = __rt.need(10, 1); return a.length === 1 && a[0] === 1 && a[{index}] === undefined; }}"
            ),
        );
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 9)));
        rt.request_collection().unwrap();
        assert_live(std::slice::from_ref(&weak));
        complete_requests(&rt, &task);
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 10)));
        rt.request_collection().unwrap();
        assert_stale(&rt, std::slice::from_ref(&weak));
        complete_requests(&rt, &task);
        assert_eq!(run_to_end(&task).view(), ValueView::Bool(true));
    }
}

/// The global object and its cycle are reachable only from VmState, after
/// the assignment operands and argument have been discarded.
#[test]
fn task_global_object_retains_and_reclaims() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        &format!("function f(o) {{ globalThis.saved = o; o = 0; {SPIN} return 0; }}"),
    );
    run_prologue(&task);
    let held = Held::new(vec![weak], move || drop(task));
    assert_retained(&rt, &held);
    assert_reclaimed(&rt, held);
}

#[test]
fn finished_task_releases_global_object() {
    let rt = Runtime::new();
    let (task, weak) = spawn_holding(
        &rt,
        &format!("function f(o) {{ globalThis.saved = o; o = 0; {SPIN} return 0; }}"),
    );
    run_prologue(&task);
    rt.collect().unwrap();
    assert!(weak.is_live());
    assert_eq!(run_to_end(&task).view(), ValueView::Number(0.0));
    assert_reclaimed(&rt, Held::new(vec![weak], || {}).keeping(task));
}

/// Synthetic fixture, not light-cljs output. The formatter-to-cache edge keeps
/// the bound callable alive, and the reverse edge retains a detached receiver.
#[test]
fn intl_number_format_bound_cache_retains_and_reclaims() {
    for result in ["n", "n.format"] {
        let rt = Runtime::new();
        let source = format!("let n=new Intl.NumberFormat('de'); n.format; {result}");
        let code = clj_quickjs::frontend::compile_script("formatter", &source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(value) = task.run().unwrap() else {
            panic!("expected formatter")
        };
        let weak = value.downgrade();
        drop(task);
        rt.collect().unwrap();
        assert!(weak.upgrade().is_ok());
        assert_eq!(rt.heap_usage().objects.bound_function, 1);
        drop(value);
        rt.collect().unwrap();
        assert!(matches!(weak.upgrade(), Err(HandleError::Reclaimed)));
        assert_eq!(rt.heap_usage().objects.bound_function, 0);
        assert_eq!(rt.heap_usage().objects.plain, 65);
    }
}

/// Synthetic fixture, not light-cljs output. The cached compare function and
/// receiver retain one another while rooted, then their cycle is collected.
#[test]
fn intl_collator_bound_cache_retains_and_reclaims() {
    for result in ["c", "c.compare"] {
        let rt = Runtime::new();
        let source = format!("let c=new Intl.Collator('en'); c.compare; {result}");
        let code = clj_quickjs::frontend::compile_script("collator", &source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(value) = task.run().unwrap() else {
            panic!("expected Collator")
        };
        let weak = value.downgrade();
        drop(task);
        rt.collect().unwrap();
        assert!(weak.upgrade().is_ok());
        assert_eq!(rt.heap_usage().objects.bound_function, 1);
        drop(value);
        rt.collect().unwrap();
        assert!(matches!(weak.upgrade(), Err(HandleError::Reclaimed)));
        assert_eq!(rt.heap_usage().objects.bound_function, 0);
        assert_eq!(rt.heap_usage().objects.plain, 65);
    }
}

/// A running task's body is skipped by its RootSource. Only the runner's
/// safepoint report can retain this symbol key and activate its value.
#[test]
fn weak_map_symbol_keys_survive_running_safepoints() {
    #[derive(Default)]
    struct Reports(Mutex<Vec<CollectionReport>>);
    impl CollectionObserver for Reports {
        fn collected(&self, report: &CollectionReport) {
            self.0.lock().unwrap().push(report.clone());
        }
    }
    let rt = Runtime::new();
    let reports = Arc::new(Reports::default());
    rt.observe_collections(Arc::downgrade(&reports) as Weak<dyn CollectionObserver>);
    let value = rt.scope(|s| {
        let v = s.new_object();
        s.persist(v)
    });
    let weak = value.downgrade();
    let code = compile(
        "function f(v) { let key = Symbol('live'); const m = new WeakMap(); m.set(key, v); v = null; while (__rt.effect(0, 0)) {} if (m.get(key) === undefined) throw 'lost symbol at safepoint'; key = null; return m; }",
    );
    let task = rt.spawn(&code, &[&value]).unwrap();
    drop(value);
    let spin = Spin::default();
    let runner = {
        let spin = spin.clone();
        thread::spawn(move || {
            let mut host = SpinHost::new(&spin);
            complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap())
        })
    };
    let worker = runner.thread().id();
    assert!(spin.entered.wait(), "the symbol-key task never started");
    for _ in 0..3 {
        rt.request_collection().unwrap();
        assert!(weak.is_live(), "the symbol local must activate its entry");
        let reports = reports.0.lock().unwrap();
        assert!(
            reports
                .last()
                .unwrap()
                .participants
                .contains(&(worker, MutatorState::Safepoint))
        );
    }
    spin.stop.store(true, Ordering::Release);
    let map = runner.join().unwrap();
    assert_eq!(map.weak_map_live_entry_count(), Some(1));
    rt.collect().unwrap();
    assert_eq!(map.weak_map_live_entry_count(), Some(0));
    assert!(!weak.is_live(), "dropping the local key releases the value");
}

/// Synthetic fixture, not light-cljs output. The formatter-to-cache edge keeps
/// the bound callable alive, and the reverse edge retains a detached receiver.
#[test]
fn intl_date_time_format_bound_cache_retains_and_reclaims() {
    for result in ["n", "n.format"] {
        let rt = Runtime::new();
        let source = format!("let n=new Intl.DateTimeFormat('de'); n.format; {result}");
        let code = clj_quickjs::frontend::compile_script("formatter", &source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let RunOutcome::Complete(value) = task.run().unwrap() else {
            panic!("expected formatter")
        };
        let weak = value.downgrade();
        drop(task);
        rt.collect().unwrap();
        assert!(weak.upgrade().is_ok());
        assert_eq!(rt.heap_usage().objects.bound_function, 1);
        drop(value);
        rt.collect().unwrap();
        assert!(matches!(weak.upgrade(), Err(HandleError::Reclaimed)));
        assert_eq!(rt.heap_usage().objects.bound_function, 0);
        assert_eq!(rt.heap_usage().objects.plain, 65);
    }
}

/// Synthetic fixtures, not light-cljs output. Protocol records must release
/// their source while the async activation remains alive in an outer scope.
#[test]
fn for_await_abrupt_exit_releases_iterator() {
    for protocol in ["asyncIterator", "iterator"] {
        for (case, body) in [
            (
                "break",
                "for await (const x of iterable) { o = 0; __rt.need(9, 1); break; }",
            ),
            (
                "throw",
                "try { for await (const x of iterable) { o = 0; __rt.need(9, 1); throw 1; } } catch(e) { if (e !== 1) throw e; }",
            ),
            (
                "outer continue",
                "out: for (let n = 0; n < 1; n++) { for await (const x of iterable) { o = 0; __rt.need(9, 1); continue out; } }",
            ),
            (
                "return",
                "try { for await (const x of iterable) { o = 0; __rt.need(9, 1); return; } } finally { __rt.need(10, 1); }",
            ),
            (
                "exhaustion",
                "for await (const x of iterable) { o = 0; __rt.need(9, 1); continue; }",
            ),
            (
                "close failure",
                "try { for await (const x of iterable) { o = 0; __rt.need(9, 1); break; } } catch(e) { if (e !== 2) throw e; }",
            ),
        ] {
            let source = format!(
                "function f(o) {{ (async () => {{
                    function iterable() {{ return {{
                        keep: o, n: 0,
                        [Symbol.{protocol}]() {{ return this; }},
                        next() {{ return {{value: 0, done: this.n++ > 0}}; }},
                        return() {{ {} return {{}}; }}
                    }}; }}
                    // Only the protocol record holds the source after acquisition.
                    {body}
                    __rt.need(10, 1);
                }})(); return true; }}",
                if case == "close failure" {
                    "throw 2;"
                } else {
                    ""
                }
            )
            .replace("of iterable)", "of iterable())");
            let rt = Runtime::new();
            let (task, weak) = spawn_holding(&rt, &source);
            assert!(
                matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 9)),
                "{protocol}/{case}"
            );
            rt.collect().unwrap();
            assert_live(std::slice::from_ref(&weak));
            complete_requests(&rt, &task);
            assert!(
                matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt, 10)),
                "{protocol}/{case}"
            );
            rt.collect().unwrap();
            assert!(
                !weak.is_live(),
                "{protocol}/{case}: abandoned iterator retained its source"
            );
            assert_stale(&rt, std::slice::from_ref(&weak));
            complete_requests(&rt, &task);
            assert_eq!(run_to_end(&task).view(), ValueView::Bool(true));
            assert!(task.unhandled_rejections().entries.is_empty());
        }
    }
}
