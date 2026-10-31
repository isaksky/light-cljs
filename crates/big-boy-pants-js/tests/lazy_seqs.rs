//! Lazy sequences as shareable once-realized cells (light-cljs-479,
//! `src/collections/lazy.rs`): one task realizes a shared lazy sequence by
//! running its thunk once, racing readers on other tasks park until it is
//! realized, a failure reaches every reader of the failed attempt, and
//! parked readers survive collection. Synthetic JavaScript, driven by hand.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier, Mutex};
use std::thread;

use big_boy_pants_js::frontend::compile_function;
use big_boy_pants_js::{
    Budget, CellStatus, Completion, DependencyId, HeapPolicy, Host, HostCall, LAZY_REALIZATION,
    Primitive, ResultKind, RunOutcome, Runtime, Task, ValueView,
};

/// Counts thunk runs (`__rt.effect(1, …)`), optionally holding the first
/// one at two barriers: "inside the thunk" and "may return".
#[derive(Clone, Default)]
struct Thunks {
    runs: Arc<AtomicUsize>,
    hold: Option<(Arc<Barrier>, Arc<Barrier>)>,
}

impl Host for Thunks {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        assert_eq!(call.op, 1);
        let run = self.runs.fetch_add(1, Ordering::SeqCst);
        if run == 0
            && let Some((inside, release)) = &self.hold
        {
            inside.wait();
            release.wait();
        }
        Ok(Primitive::Number(run as f64))
    }
}

/// A published lazy sequence whose thunk counts its runs and realizes
/// `(1 2 3)`, or throws `"boom"` when `throws`.
fn shared_lazy(rt: &Runtime, throws: bool) -> big_boy_pants_js::Rooted {
    let body = if throws {
        "throw 'boom';"
    } else {
        "return __rt.formList(1, 2, 3);"
    };
    let make = compile_function(&format!(
        "function make() {{ return __rt.lazySeq(() => {{ __rt.effect(1, 0); {body} }}); }}"
    ))
    .unwrap();
    let RunOutcome::Complete(lazy) = rt.spawn(&make, &[]).unwrap().run().unwrap() else {
        panic!("maker failed");
    };
    rt.publish(&lazy).unwrap();
    lazy
}

/// A reader: the sum of the sequence's elements, or the caught failure.
fn reader(rt: &Runtime, lazy: &big_boy_pants_js::Rooted) -> Task {
    let read = compile_function(
        "function read(xs) {
           try {
             let total = 0;
             for (let s = __rt.cljSeq(xs); s !== null; s = __rt.cljNext(s)) total += __rt.cljFirst(s);
             return total;
           } catch (e) { return 'caught ' + e; }
         }",
    )
    .unwrap();
    rt.spawn(&read, &[lazy]).unwrap()
}

fn complete(outcome: RunOutcome) -> ValueView {
    match outcome {
        RunOutcome::Complete(v) => v.view(),
        other => panic!("expected completion, got {other:?}"),
    }
}

/// Run `task` on its own thread until it parks on a lazy realization;
/// returns the wait's key.
fn park(task: Task, host: Thunks) -> big_boy_pants_js::DependencyKey {
    thread::spawn(
        move || match task.run_with(Budget::UNLIMITED, &mut host.clone()) {
            Ok(RunOutcome::Waiting(key)) => key,
            other => panic!("expected a lazy wait, got {other:?}"),
        },
    )
    .join()
    .unwrap()
}

#[test]
fn racing_readers_park_and_the_thunk_runs_once() {
    let rt = Runtime::new();
    let lazy = shared_lazy(&rt, false);
    let (inside, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let host = Thunks {
        runs: Arc::default(),
        hold: Some((inside.clone(), release.clone())),
    };
    let (a, b) = (reader(&rt, &lazy), reader(&rt, &lazy));
    // A claims the cell and is held inside the thunk on its own thread.
    let realizer = {
        let (a, mut host) = (a.clone(), host.clone());
        thread::spawn(move || a.run_with(Budget::UNLIMITED, &mut host).unwrap())
    };
    inside.wait();
    // B, on another thread, finds the cell being realized and parks.
    let key = park(b.clone(), host.clone());
    assert_eq!(key.kind(), LAZY_REALIZATION);
    assert!(matches!(rt.cell_status(key), CellStatus::Pending { .. }));
    assert_eq!(rt.lazy_realizer(key), Some(a.id()));
    assert!(rt.take_lazy_wakeups().is_empty());
    // A's thunk returns: the settlement wakes B.
    release.wait();
    assert_eq!(complete(realizer.join().unwrap()), ValueView::Number(6.0));
    assert!(matches!(rt.cell_status(key), CellStatus::Absent));
    assert_eq!(rt.lazy_realizer(key), None);
    let woken = rt.take_lazy_wakeups();
    assert_eq!(woken.iter().map(Task::id).collect::<Vec<_>>(), [b.id()]);
    assert_eq!(complete(b.run().unwrap()), ValueView::Number(6.0));
    // Realized once: later readers read the value.
    assert_eq!(
        complete(reader(&rt, &lazy).run().unwrap()),
        ValueView::Number(6.0)
    );
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_throwing_thunk_is_observed_by_every_parked_reader() {
    let rt = Runtime::new();
    let lazy = shared_lazy(&rt, true);
    let (inside, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let host = Thunks {
        runs: Arc::default(),
        hold: Some((inside.clone(), release.clone())),
    };
    let (a, b) = (reader(&rt, &lazy), reader(&rt, &lazy));
    let realizer = {
        let (a, mut host) = (a.clone(), host.clone());
        thread::spawn(move || a.run_with(Budget::UNLIMITED, &mut host).unwrap())
    };
    inside.wait();
    park(b.clone(), host.clone());
    release.wait();
    let RunOutcome::Complete(a_value) = realizer.join().unwrap() else {
        panic!("realizer did not complete");
    };
    assert_eq!(a_value.string().unwrap().to_string_lossy(), "caught boom");
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    let RunOutcome::Complete(b_value) = b.run().unwrap() else {
        panic!("parked reader did not complete");
    };
    assert_eq!(b_value.string().unwrap().to_string_lossy(), "caught boom");
    // One run for the failed attempt. As in ClojureScript, a later
    // realization runs the thunk again.
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
    let RunOutcome::Complete(c_value) = reader(&rt, &lazy)
        .run_with(Budget::UNLIMITED, &mut host.clone())
        .unwrap()
    else {
        panic!("later reader did not complete");
    };
    assert_eq!(c_value.string().unwrap().to_string_lossy(), "caught boom");
    assert_eq!(host.runs.load(Ordering::SeqCst), 2);
}

#[test]
fn collection_while_a_reader_is_parked_keeps_the_sequence_and_both_tasks() {
    let rt = Runtime::new();
    let host = Thunks::default();
    let lazy = {
        let make = compile_function(
            "function make() {
               return __rt.lazySeq(() => {
                 __rt.effect(1, 0);
                 let n = 0;
                 for (let i = 0; i < 200; i++) n += 1;
                 return __rt.formList(n, __rt.formVector('kept'));
               });
             }",
        )
        .unwrap();
        let RunOutcome::Complete(lazy) = rt.spawn(&make, &[]).unwrap().run().unwrap() else {
            panic!("maker failed");
        };
        rt.publish(&lazy).unwrap();
        lazy
    };
    let read = compile_function(
        "function read(xs) { return __rt.cljFirst(xs) + __rt.cljCount(__rt.cljFirst(__rt.cljRest(xs))); }",
    )
    .unwrap();
    let (a, b) = (
        rt.spawn(&read, &[&lazy]).unwrap(),
        rt.spawn(&read, &[&lazy]).unwrap(),
    );
    drop(lazy);
    // A enters the thunk and yields inside it, holding the claim.
    let mut steps = 0;
    while host.runs.load(Ordering::SeqCst) == 0 {
        assert!(matches!(
            a.run_with(Budget::instructions(1), &mut host.clone())
                .unwrap(),
            RunOutcome::Yielded
        ));
        steps += 1;
        assert!(steps < 1000);
    }
    let RunOutcome::Waiting(key) = b.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap() else {
        panic!("B did not park");
    };
    assert_eq!(key.kind(), LAZY_REALIZATION);
    // Only the two tasks hold the sequence now.
    rt.collect().unwrap();
    rt.collect().unwrap();
    assert_eq!(
        complete(a.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap()),
        ValueView::Number(201.0)
    );
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    rt.collect().unwrap();
    assert_eq!(complete(b.run().unwrap()), ValueView::Number(201.0));
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
}

/// One task's result for `body`, a function body over `__rt`.
fn eval(rt: &Runtime, body: &str) -> big_boy_pants_js::Rooted {
    let f = compile_function(&format!("function test() {{ {body} }}")).unwrap();
    match rt.spawn(&f, &[]).unwrap().run().unwrap() {
        RunOutcome::Complete(v) => v,
        other => panic!("{body}: {other:?}"),
    }
}

#[test]
fn a_thunk_capturing_task_owned_state_does_not_publish_and_names_the_path() {
    let rt = Runtime::new();
    let lazy = eval(
        &rt,
        "let n = 0; const xs = __rt.lazySeq(() => { n++; return null; }); n = 1; return xs;",
    );
    let error = rt.publish(&lazy).unwrap_err().to_string();
    assert!(error.contains("value.thunk.capture(0)"), "{error}");
    // Realized to a task-owned value: the path names what it realized.
    let realized = eval(
        &rt,
        "const xs = __rt.lazySeq(() => __rt.formList({})); __rt.cljCount(xs); return xs;",
    );
    let error = rt.publish(&realized).unwrap_err().to_string();
    assert!(error.contains("value.realized[0]"), "{error}");
}

#[test]
fn a_shared_sequence_cannot_realize_a_task_owned_value() {
    let rt = Runtime::new();
    let lazy = eval(&rt, "return __rt.lazySeq(() => __rt.formList({}));");
    rt.publish(&lazy).unwrap();
    let read = compile_function(
        "function read(xs) { try { return __rt.cljCount(xs); } catch (e) { return String(e); } }",
    )
    .unwrap();
    let RunOutcome::Complete(v) = rt.spawn(&read, &[&lazy]).unwrap().run().unwrap() else {
        panic!("reader failed");
    };
    let message = v.string().unwrap().to_string_lossy();
    assert!(
        message.starts_with("TypeError: a shared lazy sequence realized a value it cannot share"),
        "{message}"
    );
}

#[test]
fn lazy_and_cons_sequences_compare_hash_and_realize_like_lists() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "const step = (self, n) => __rt.lazySeq(() => n === 0 ? null : __rt.cljCons(n, self(self, n - 1)));
         const xs = step(step, 3), list = __rt.formList(3, 2, 1);
         const ok = [
           __rt.formEquals(xs, list), __rt.formEquals(list, xs), __rt.formHash(xs) === __rt.formHash(list),
           __rt.cljCount(xs) === 3, __rt.cljNth(xs, 2) === 1, __rt.cljNthOr(xs, 7, 'd') === 'd',
           __rt.formKind(xs) === 'lazy-seq', __rt.formKind(__rt.cljRest(xs)) === 'lazy-seq',
           __rt.formKind(__rt.cljSeq(xs)) === 'cons', __rt.cljRealized(xs),
           __rt.formEquals(__rt.cljConcat(__rt.formList(4), xs), __rt.formList(4, 3, 2, 1)),
           __rt.formKind(__rt.cljConcat(__rt.formList(4), step(step, 1))) === 'lazy-seq',
           __rt.cljSeq(step(step, 0)) === null, __rt.formEquals(step(step, 0), __rt.formList()),
           !__rt.formEquals(step(step, 0), null),
           __rt.cljGet(__rt.formMap(__rt.formList(3, 2, 1), 'found'), xs, null) === 'found', __rt.cljGet(xs, 0, 'none') === 'none',
           __rt.formGet(__rt.formMap(__rt.formList(3, 2, 1), 'found'), step(step, 3), null) === 'found',
         ];
         const deep = __rt.cljRealizeAll(__rt.formVector(1, xs, __rt.formMap('k', step(step, 1))));
         ok.push(__rt.formKind(__rt.formNth(deep, 1)) === 'list');
         ok.push(__rt.formKind(__rt.formGet(__rt.formNth(deep, 2), 'k', null)) === 'list');
         const plain = __rt.formVector(1, 2);
         ok.push(__rt.cljRealizeAll(plain) === plain);
         return ok.indexOf(false);",
    );
    assert_eq!(v.view(), ValueView::Number(-1.0));
}

#[test]
fn a_vector_seq_reads_in_chunks_of_thirty_two() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "let v = __rt.formVector(); for (let i = 0; i < 40; i++) v = __rt.formConj(v, i);
         const s = __rt.cljSeq(v), first = __rt.cljChunkFirst(s), rest = __rt.cljChunkRest(s);
         const b = __rt.vectorBuilder(); __rt.builderConj(b, 'a'); __rt.builderConj(b, 'b');
         const tail = __rt.lazySeq(() => __rt.formList('c'));
         const c = __rt.cljChunkCons(__rt.freeze(b), tail);
         const ok = [
           __rt.cljChunkedSeq(s), __rt.cljCount(first) === 32, __rt.cljCount(rest) === 8,
           __rt.cljChunkNext(__rt.cljChunkRest(s)) === null, !__rt.cljChunkedSeq(tail),
           __rt.formEquals(s, v), __rt.cljNth(rest, 7) === 39, __rt.cljFirst(__rt.cljRest(s)) === 1,
           !__rt.cljChunkedSeq(__rt.formList(1)), __rt.formKind(s) === 'cons',
           __rt.cljChunkedSeq(c), __rt.formEquals(c, __rt.formList('a', 'b', 'c')),
           __rt.formEquals(__rt.cljChunkFirst(c), __rt.formVector('a', 'b')),
           __rt.cljChunkRest(c) === tail, __rt.cljChunkCons(__rt.formVector(), tail) === tail,
         ];
         return ok.indexOf(false);",
    );
    assert_eq!(v.view(), ValueView::Number(-1.0));
}

#[test]
fn a_thunk_that_needs_its_own_sequence_fails_instead_of_recursing() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "let xs; xs = __rt.lazySeq(() => __rt.cljFirst(xs));
         let first;
         try { __rt.cljFirst(xs); } catch (e) { first = e instanceof RangeError; }
         // Not realized: a later attempt runs the thunk again.
         try { __rt.cljFirst(xs); return false; } catch (e) { return first && !__rt.cljRealized(xs); }",
    );
    assert_eq!(v.view(), ValueView::Bool(true));
}

/// A `cljRealizeAll` that parks on a shared sequence another task is
/// realizing keeps its walk (light-cljs-622): the lists it already built
/// survive collections while it waits, and it continues where it stopped,
/// running no thunk again.
#[test]
fn a_realize_all_parked_mid_walk_keeps_its_walk_across_collection() {
    let rt = Runtime::new();
    let host = Thunks::default();
    let lazy = eval(
        &rt,
        "return __rt.lazySeq(() => {
           __rt.effect(1, 0);
           let n = 0;
           for (let i = 0; i < 200; i++) n += 1;
           return __rt.formList(n, 1);
         });",
    );
    rt.publish(&lazy).unwrap();
    let walk = compile_function(
        "function walk(shared) {
           let own = 0;
           const inner = __rt.lazySeq(() => { own++; return __rt.formList('x'); });
           const mine = __rt.lazySeq(() => { own++; return __rt.formList(inner, 2); });
           const r = __rt.cljRealizeAll(__rt.formVector(mine, shared, mine));
           const m = __rt.formList(__rt.formList('x'), 2);
           return own === 2
             && __rt.formEquals(r, __rt.formVector(m, __rt.formList(200, 1), m))
             && __rt.formNth(r, 0) === __rt.formNth(r, 2);
         }",
    )
    .unwrap();
    let (a, b) = (reader(&rt, &lazy), rt.spawn(&walk, &[&lazy]).unwrap());
    drop(lazy);
    // A enters the shared thunk and yields inside it, holding the claim.
    let mut steps = 0;
    while host.runs.load(Ordering::SeqCst) == 0 {
        assert!(matches!(
            a.run_with(Budget::instructions(1), &mut host.clone())
                .unwrap(),
            RunOutcome::Yielded
        ));
        steps += 1;
        assert!(steps < 1000);
    }
    // B realizes `mine` and parks at the shared sequence: its walk holds
    // the only reference to the list it built for `mine`.
    let RunOutcome::Waiting(key) = b.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap() else {
        panic!("B did not park");
    };
    assert_eq!(key.kind(), LAZY_REALIZATION);
    rt.collect().unwrap();
    rt.collect().unwrap();
    assert_eq!(
        complete(a.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap()),
        ValueView::Number(201.0)
    );
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    rt.collect().unwrap();
    assert_eq!(complete(b.run().unwrap()), ValueView::Bool(true));
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
}

/// A byte-limited runtime, which charges a kept `cljRealizeAll` walk.
fn byte_limited() -> Runtime {
    Runtime::with_heap_policy(
        Default::default(),
        HeapPolicy {
            hard_limit_bytes: Some(256 << 20),
            ..Default::default()
        },
    )
}

/// A task that realizes all of a vector of 2,000 task-owned lazy sequences
/// and then `shared`, inside a `try` with a `finally` block when
/// `finally`: its walk builds 2,000 lists before it meets `shared`.
fn realize_all_task(rt: &Runtime, shared: &big_boy_pants_js::Rooted, finally: bool) -> Task {
    let (open, close) = if finally {
        ("try {", "} finally { v = null; }")
    } else {
        ("{", "}")
    };
    let walk = compile_function(&format!(
        "function walk(shared) {{
           let v = __rt.formVector();
           for (let i = 0; i < 2000; i++) v = __rt.formConj(v, __rt.lazySeq(() => __rt.formList(i, i)));
           v = __rt.formConj(v, shared);
           {open} return __rt.formCount(__rt.cljRealizeAll(v)); {close}
         }}"
    ))
    .unwrap();
    rt.spawn(&walk, &[shared]).unwrap()
}

/// The managed bytes and objects of `rt` after a collection.
fn settled(rt: &Runtime) -> (usize, usize) {
    rt.collect().unwrap();
    let stats = rt.heap_stats();
    (stats.managed_bytes, stats.objects)
}

/// A cancelled `cljRealizeAll` leaves nothing of its kept walk behind
/// (light-cljs-628): the lists it built and its byte charge are released
/// by the cancellation's cleanup, while the task's handle is still held,
/// and the task it shares a sequence with goes on. Once as the realizer of
/// the shared sequence, held inside its thunk (the cancellation runs a
/// `finally` block first, unwinding through the realization), and once as
/// a reader parked on another task's realization of it.
#[test]
fn retained_realize_all_walk_is_released_on_cancellation() {
    let rt = byte_limited();
    // Grow the object table to the walk's size first, so a settled heap
    // holds as many bytes before the walk as after it.
    let warm = eval(&rt, "return null;");
    let task = realize_all_task(&rt, &warm, false);
    assert_eq!(complete(task.run().unwrap()), ValueView::Number(2001.0));
    drop(task);
    // Kept by the measurement: the walk's charge and lists, over 100 KiB.
    let kept = |parked: (usize, usize), base: (usize, usize)| {
        assert!(parked.0 > base.0 + (100 << 10), "{parked:?} over {base:?}");
        assert!(parked.1 > base.1 + 2000, "{parked:?} over {base:?}");
    };
    // Released: the heap settles where it was before the walk.
    let released = |after: (usize, usize), base: (usize, usize)| assert_eq!(after, base);

    // The walk's task realizes the shared sequence and is cancelled inside
    // its thunk; a reader parked on it then realizes it itself.
    let host = Thunks::default();
    let shared = shared_lazy(&rt, false);
    let (walk, parked) = (realize_all_task(&rt, &shared, true), reader(&rt, &shared));
    let base = settled(&rt);
    let mut steps = 0;
    while host.runs.load(Ordering::SeqCst) == 0 {
        let outcome = walk
            .run_with(Budget::instructions(1), &mut host.clone())
            .unwrap();
        assert!(matches!(outcome, RunOutcome::Yielded), "{outcome:?}");
        steps += 1;
        assert!(steps < 100_000);
    }
    let RunOutcome::Waiting(_) = parked
        .run_with(Budget::UNLIMITED, &mut host.clone())
        .unwrap()
    else {
        panic!("the reader did not park");
    };
    kept(settled(&rt), base);
    let _ = walk.cancel();
    assert!(matches!(walk.run().unwrap(), RunOutcome::Cancelled));
    released(settled(&rt), base);
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    assert_eq!(
        complete(
            parked
                .run_with(Budget::UNLIMITED, &mut host.clone())
                .unwrap()
        ),
        ValueView::Number(6.0)
    );
    assert_eq!(host.runs.load(Ordering::SeqCst), 2);
    drop((walk, parked, shared));

    // The walk's task parks on another task's realization of the shared
    // sequence and is cancelled there; the realizer finishes.
    let host = Thunks::default();
    let shared = shared_lazy(&rt, false);
    let (realizer, walk) = (reader(&rt, &shared), realize_all_task(&rt, &shared, false));
    let base = settled(&rt);
    while host.runs.load(Ordering::SeqCst) == 0 {
        let outcome = realizer
            .run_with(Budget::instructions(1), &mut host.clone())
            .unwrap();
        assert!(matches!(outcome, RunOutcome::Yielded), "{outcome:?}");
    }
    let RunOutcome::Waiting(key) = walk.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap()
    else {
        panic!("the walk did not park");
    };
    assert_eq!(key.kind(), LAZY_REALIZATION);
    kept(settled(&rt), base);
    let _ = walk.cancel();
    assert!(matches!(walk.run().unwrap(), RunOutcome::Cancelled));
    released(settled(&rt), base);
    assert_eq!(
        complete(
            realizer
                .run_with(Budget::UNLIMITED, &mut host.clone())
                .unwrap()
        ),
        ValueView::Number(6.0)
    );
    assert!(rt.take_lazy_wakeups().is_empty());
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
}

#[test]
fn a_cancelled_realizer_gives_its_claim_to_a_parked_reader() {
    let rt = Runtime::new();
    let host = Thunks::default();
    let lazy = eval(
        &rt,
        "return __rt.lazySeq(() => { __rt.effect(1, 0); let n = 0; for (let i = 0; i < 100; i++) n++; return __rt.formList(n); });",
    );
    rt.publish(&lazy).unwrap();
    let (a, b) = (reader(&rt, &lazy), reader(&rt, &lazy));
    while host.runs.load(Ordering::SeqCst) == 0 {
        assert!(matches!(
            a.run_with(Budget::instructions(1), &mut host.clone())
                .unwrap(),
            RunOutcome::Yielded
        ));
    }
    let RunOutcome::Waiting(key) = b.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap() else {
        panic!("B did not park");
    };
    assert_eq!(rt.lazy_realizer(key), Some(a.id()));
    let _ = a.cancel();
    assert!(matches!(a.run().unwrap(), RunOutcome::Cancelled));
    assert_eq!(rt.lazy_realizer(key), None);
    // The dropped claim woke B, which realizes the sequence itself.
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    assert_eq!(
        complete(b.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap()),
        ValueView::Number(100.0)
    );
    assert_eq!(host.runs.load(Ordering::SeqCst), 2);
}

/// A published lazy sequence of `body` (a thunk body that may read the run
/// index `r`) and two readers of it: the first held inside the thunk on its
/// own thread, the second parked. Returns the sequence, the held reader's
/// thread, the parked reader, the host and the barrier that releases it.
fn race(
    rt: &Runtime,
    body: &str,
) -> (
    big_boy_pants_js::Rooted,
    thread::JoinHandle<RunOutcome>,
    Task,
    Thunks,
    Arc<Barrier>,
) {
    let make = compile_function(&format!(
        "function make() {{ return __rt.lazySeq(() => {{ const r = __rt.effect(1, 0); {body} }}); }}"
    ))
    .unwrap();
    let RunOutcome::Complete(lazy) = rt.spawn(&make, &[]).unwrap().run().unwrap() else {
        panic!("maker failed");
    };
    rt.publish(&lazy).unwrap();
    let (inside, release) = (Arc::new(Barrier::new(2)), Arc::new(Barrier::new(2)));
    let host = Thunks {
        runs: Arc::default(),
        hold: Some((inside.clone(), release.clone())),
    };
    let (a, b) = (reader(rt, &lazy), reader(rt, &lazy));
    let realizer = {
        let mut host = host.clone();
        thread::spawn(move || a.run_with(Budget::UNLIMITED, &mut host).unwrap())
    };
    inside.wait();
    park(b.clone(), host.clone());
    (lazy, realizer, b, host, release)
}

fn caught(outcome: RunOutcome) -> String {
    match outcome {
        RunOutcome::Complete(v) => v.string().unwrap().to_string_lossy(),
        other => panic!("expected a caught failure, got {other:?}"),
    }
}

#[test]
fn a_parked_reader_observes_its_attempt_failure_after_a_later_attempt_succeeds() {
    let rt = Runtime::new();
    let (lazy, realizer, b, host, release) =
        race(&rt, "if (r === 0) throw 'boom'; return __rt.formList(r);");
    release.wait();
    assert_eq!(caught(realizer.join().unwrap()), "caught boom");
    // Before B runs again, a third reader retries and succeeds.
    let c = reader(&rt, &lazy);
    assert_eq!(
        complete(c.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap()),
        ValueView::Number(1.0)
    );
    assert_eq!(rt.take_lazy_wakeups().len(), 1);
    assert_eq!(caught(b.run().unwrap()), "caught boom");
    assert_eq!(host.runs.load(Ordering::SeqCst), 2);
}

#[test]
fn a_parked_reader_observes_a_shared_sequence_s_unshareable_result_once() {
    let rt = Runtime::new();
    let (_lazy, realizer, b, host, release) = race(&rt, "return __rt.formList({});");
    release.wait();
    let message = caught(realizer.join().unwrap());
    assert!(
        message.starts_with("caught TypeError: a shared lazy sequence"),
        "{message}"
    );
    assert_eq!(caught(b.run().unwrap()), message);
    assert_eq!(host.runs.load(Ordering::SeqCst), 1);
}

#[test]
fn equality_concat_and_cycles_read_only_what_they_need() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "const boom = () => __rt.lazySeq(() => { throw 'tail'; });
         const ok = [
           // An unequal first element ends the comparison before the tail.
           !__rt.formEquals(__rt.cljCons(0, boom()), __rt.formList(1)),
           // concat realizes its collections one at a time.
           __rt.cljFirst(__rt.cljConcat(__rt.cljCons(1, boom()), __rt.formVector())) === 1,
           __rt.formKind(__rt.cljConcat(__rt.formList(1), __rt.formVector(2))) === 'lazy-seq',
         ];
         let xs; xs = __rt.lazySeq(() => __rt.formList(xs));
         __rt.cljFirst(xs);
         try { __rt.cljRealizeAll(xs); ok.push(false); } catch (e) { ok.push(e instanceof RangeError); }
         try { __rt.formHash(xs); ok.push(false); } catch (e) { ok.push(e instanceof RangeError); }
         ok.push(__rt.formEquals(xs, xs));
         return ok.indexOf(false);",
    );
    assert_eq!(v.view(), ValueView::Number(-1.0));
}

#[test]
fn embedder_structural_operations_refuse_unrealized_lazy_keys() {
    let rt = Runtime::new();
    let lazy = eval(&rt, "return __rt.lazySeq(() => __rt.formList(1));");
    rt.scope(|s| {
        let key = s.local(&lazy).unwrap();
        let one = s.primitive(Primitive::Number(1.0));
        assert!(s.new_form_map(&[(key, one)]).is_err());
        assert!(s.new_form_set(&[key]).is_err());
        let empty = s.new_form_map(&[]).unwrap();
        assert!(s.form_get(empty, key).is_err());
    });
}

#[test]
fn a_concat_that_contains_itself_fails_instead_of_looping() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "let xs; xs = __rt.lazySeq(() => __rt.cljConcat(xs, __rt.formList(1)));
         try { __rt.cljFirst(xs); return false; } catch (e) { return e instanceof RangeError; }",
    );
    assert_eq!(v.view(), ValueView::Bool(true));
}

#[test]
fn equality_backtracking_does_not_reuse_sequence_positions() {
    let rt = Runtime::new();
    let v = eval(
        &rt,
        "const p = __rt.formList(1, 2), q = __rt.formList(1, 3);
         const [a, b, c, d] = [p, p, q, q].map(x => __rt.lazySeq(() => x));
         for (const s of [a, b, c, d]) __rt.cljFirst(s);
         return __rt.formEquals(__rt.formMap(0, a, 2147483647, b), __rt.formMap(0, c, 2147483647, d));",
    );
    assert_eq!(v.view(), ValueView::Bool(false));
}

#[test]
fn undrained_lazy_wakeups_do_not_keep_dropped_readers() {
    let rt = Runtime::new();
    let (_lazy, realizer, b, _host, release) = race(&rt, "return __rt.formList(1);");
    release.wait();
    assert_eq!(complete(realizer.join().unwrap()), ValueView::Number(1.0));
    // The woken reader is dropped and the wakeups are never taken.
    drop(b);
    assert!(rt.take_lazy_wakeups().is_empty());
}

/// Attributes the realizations its task is running at each effect, as an
/// embedder that counts the effect for them does.
struct Attributing(Runtime, Arc<std::sync::Mutex<Vec<u64>>>);

impl Host for Attributing {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let serials = self.0.attribute_realizing(call.task);
        self.1.lock().unwrap().extend(serials);
        Ok(Primitive::Null)
    }
}

#[test]
fn consumption_logs_are_isolated_between_runtimes() {
    let a = Runtime::new();
    let b = Runtime::new();
    let attributed = shared_lazy(&a, false);
    let plain = shared_lazy(&b, false);
    let serials = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut host = Attributing(a.clone(), serials.clone());
    complete(
        reader(&a, &attributed)
            .run_with(Budget::UNLIMITED, &mut host)
            .unwrap(),
    );
    complete(
        reader(&b, &plain)
            .run_with(Budget::UNLIMITED, &mut Thunks::default())
            .unwrap(),
    );
    let serials = serials.lock().unwrap().clone();
    assert_eq!(serials.len(), 1);
    for take_before_run in [true, false] {
        let a_reader = reader(&a, &attributed);
        let b_reader = reader(&b, &plain);
        assert_eq!(a_reader.id(), b_reader.id());
        complete(a_reader.run_with(Budget::UNLIMITED, &mut host).unwrap());
        if take_before_run {
            // A foreign runtime cannot take this task's entries even when
            // its task has the same numeric identity, or clear them.
            assert!(b.take_lazy_consumed(b_reader.id()).is_empty());
            assert_eq!(a.take_lazy_consumed(a_reader.id()), serials);
        }
        complete(
            b_reader
                .run_with(Budget::UNLIMITED, &mut Thunks::default())
                .unwrap(),
        );
        // Starting the foreign task drops A's untaken entries instead of
        // keeping them as if this were another run of the same task.
        assert!(b.take_lazy_consumed(b_reader.id()).is_empty());
        assert!(a.take_lazy_consumed(a_reader.id()).is_empty());
    }
}

#[test]
fn readers_of_a_settled_attributed_realization_are_noted() {
    let rt = Runtime::new();
    let attributed = shared_lazy(&rt, false);
    let plain = shared_lazy(&rt, false);
    let serials = Arc::new(std::sync::Mutex::new(Vec::new()));
    let mut host = Attributing(rt.clone(), serials.clone());
    // The realizer's effect inside the thunk attributes the realization.
    let a = reader(&rt, &attributed);
    let outcome = a.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert_eq!(complete(outcome), ValueView::Number(6.0));
    let serials = serials.lock().unwrap().clone();
    assert_eq!(serials.len(), 1);
    // A later reader reads the settled value without waiting: noted once,
    // and taken once.
    let b = reader(&rt, &attributed);
    let outcome = b.run_with(Budget::UNLIMITED, &mut host).unwrap();
    assert_eq!(complete(outcome), ValueView::Number(6.0));
    assert_eq!(rt.take_lazy_consumed(b.id()), serials);
    assert!(rt.take_lazy_consumed(b.id()).is_empty());
    // Another task's run drops what an earlier one left untaken, and a
    // realization nobody attributed is not noted.
    let c = reader(&rt, &attributed);
    complete(c.run_with(Budget::UNLIMITED, &mut host).unwrap());
    let d = reader(&rt, &plain);
    complete(
        d.run_with(Budget::UNLIMITED, &mut Thunks::default())
            .unwrap(),
    );
    let e = reader(&rt, &plain);
    complete(
        e.run_with(Budget::UNLIMITED, &mut Thunks::default())
            .unwrap(),
    );
    assert!(rt.take_lazy_consumed(c.id()).is_empty());
    assert!(rt.take_lazy_consumed(e.id()).is_empty());
}

/// Answers the realization gate (`__rt.effect(2, 0)`) from `answers` in
/// order, then with 0 (admitted), and records which thunk ran
/// (`__rt.effect(1, tag)`).
#[derive(Clone, Default)]
struct ScriptedGate {
    answers: Arc<Mutex<VecDeque<f64>>>,
    asked: Arc<AtomicUsize>,
    ran: Arc<Mutex<Vec<f64>>>,
}

impl ScriptedGate {
    fn new(answers: &[f64]) -> ScriptedGate {
        ScriptedGate {
            answers: Arc::new(Mutex::new(answers.iter().copied().collect())),
            ..Default::default()
        }
    }

    fn ran(&self) -> Vec<f64> {
        self.ran.lock().unwrap().clone()
    }
}

impl Host for ScriptedGate {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        match (call.op, call.arg) {
            (1, Primitive::Number(tag)) => {
                self.ran.lock().unwrap().push(tag);
                Ok(Primitive::Undefined)
            }
            (2, _) => {
                self.asked.fetch_add(1, Ordering::SeqCst);
                let answer = self.answers.lock().unwrap().pop_front().unwrap_or(0.0);
                Ok(Primitive::Number(answer))
            }
            (op, arg) => Err(format!("unexpected effect {op} {arg:?}")),
        }
    }
}

/// The published value `body` (a function body over `__rt`) returns.
fn published(rt: &Runtime, body: &str) -> big_boy_pants_js::Rooted {
    let value = eval(rt, body);
    rt.publish(&value).unwrap();
    value
}

/// Completes with 0 the gate dependency `task` waits on, then runs it.
fn open_gate(rt: &Runtime, task: &Task, host: &ScriptedGate) -> RunOutcome {
    let request = task.take_producer_requests().pop().unwrap();
    let answer = rt.root(Primitive::Number(0.0));
    let task = rt
        .complete(
            request.key(),
            request.producer(),
            Completion::Ready(&answer),
        )
        .unwrap()
        .into_runnable()
        .pop()
        .unwrap();
    task.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap()
}

const GATE_KIND: ResultKind = ResultKind::new(9);

/// A runtime whose realization gate is `__rt.effect(2, 0)`.
fn gated_runtime() -> Runtime {
    let rt = Runtime::new();
    rt.set_realization_gate(2, GATE_KIND);
    rt
}

/// `outcome` is a wait at the gate, on the dependency 5 the host named.
fn waits_at_gate(outcome: RunOutcome) {
    match outcome {
        RunOutcome::Waiting(key) => {
            assert_eq!(key.kind(), GATE_KIND);
            assert_eq!(key.dependency(), DependencyId::new(5));
        }
        other => panic!("expected a gate wait, got {other:?}"),
    }
}

/// The embedder's realization gate is asked before each shared cell a
/// realization claims, not once per realization frame (light-cljs-685): a
/// frame that realizes the chain of a whole-sequence read goes on to the
/// next cell of the chain, and its admission to the first cell does not
/// admit it to the next. Here the gate admits the outer cell and closes
/// for its tail: the task waits on the gate's dependency without running
/// the tail's thunk, and finishes once that dependency completes.
#[test]
fn realization_gate_is_asked_for_each_shared_cell_in_a_chain() {
    let rt = gated_runtime();
    let outer = published(
        &rt,
        "const tail = __rt.lazySeq(() => { __rt.effect(1, 2); return __rt.formList(2, 3); });
         return __rt.lazySeq(() => { __rt.effect(1, 1); return __rt.cljCons(1, tail); });",
    );
    let count = compile_function("function count(xs) { return __rt.cljCount(xs); }").unwrap();
    let host = ScriptedGate::new(&[0.0, 5.0]);
    let task = rt.spawn(&count, &[&outer]).unwrap();
    waits_at_gate(task.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap());
    assert_eq!(host.asked.load(Ordering::SeqCst), 2);
    assert_eq!(host.ran(), [1.0], "the tail ran without the gate");
    assert_eq!(
        complete(open_gate(&rt, &task, &host)),
        ValueView::Number(3.0)
    );
    assert_eq!(host.ran(), [1.0, 2.0]);
}

/// A `concat` realizes its first collection, another shared cell, in the
/// frame that claimed the `concat` (light-cljs-685): the gate is asked
/// again before that frame claims the collection.
#[test]
fn realization_gate_is_asked_for_the_collection_a_concat_realizes() {
    let rt = gated_runtime();
    let joined = published(
        &rt,
        "const first = __rt.lazySeq(() => { __rt.effect(1, 2); return __rt.formList(2, 3); });
         return __rt.cljConcat(first, __rt.formList(4));",
    );
    let head = compile_function("function head(xs) { return __rt.cljFirst(xs); }").unwrap();
    let host = ScriptedGate::new(&[0.0, 5.0]);
    let task = rt.spawn(&head, &[&joined]).unwrap();
    waits_at_gate(task.run_with(Budget::UNLIMITED, &mut host.clone()).unwrap());
    assert_eq!(host.asked.load(Ordering::SeqCst), 2);
    assert!(host.ran().is_empty(), "the collection ran without the gate");
    assert_eq!(
        complete(open_gate(&rt, &task, &host)),
        ValueView::Number(2.0)
    );
    assert_eq!(host.ran(), [2.0]);
}
