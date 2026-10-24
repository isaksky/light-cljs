//! Cooperative safepoints and mutator registration (`docs/design.md` §11):
//! collections requested while other OS threads run tasks or hold scopes.
//!
//! Every cross-thread step is sequenced by channels, `std::sync::Barrier`
//! or an observed state (`Runtime::collection_requested`, the collection
//! counter), never by sleeping; every wait is bounded by `HANDSHAKE` and
//! fails instead of hanging. Early VM tests: programs are hand-assembled
//! bytecode, not JavaScript source support.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, TryRecvError};
use std::sync::{Arc, Barrier};
use std::thread;
use std::time::{Duration, Instant};

use clj_quickjs::{
    Assembler, Budget, CollectError, Executable, Host, HostCall, Opcode, Primitive, Rooted,
    RunOutcome, Runtime, ValueView,
};

/// Bound for every cross-thread handshake.
const HANDSHAKE: Duration = Duration::from_secs(10);

fn new_rooted_object(rt: &Runtime) -> Rooted {
    rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    })
}

/// Block until `cond` holds, failing (not hanging) after `HANDSHAKE`. Used
/// only to observe a state another thread has already committed to (a
/// pending request), not to wait out a race.
fn wait_until(what: &str, cond: impl Fn() -> bool) {
    let deadline = Instant::now() + HANDSHAKE;
    while !cond() {
        assert!(Instant::now() < deadline, "timed out waiting for {what}");
        thread::yield_now();
    }
}

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// `function(kept, doomed) { doomed = undefined;
///   do { new Object(); } while (host_effect(0, null)); return kept; }`:
/// allocates garbage on every iteration and loops through a backward branch
/// (a poll point) for as long as the host says so.
fn loop_until_host_stops() -> Executable {
    let mut asm = Assembler::new();
    let top = asm.label();
    asm.op(Opcode::Undefined).set_local(1);
    asm.bind(top);
    asm.op(Opcode::NewObject).op(Opcode::Pop);
    asm.constant(Primitive::Null).host_effect(0);
    let done = asm.label();
    asm.jump_if_false(done).jump(top);
    asm.bind(done);
    asm.get_local(0).op(Opcode::Return);
    asm.finish("loop-until-host-stops", 2, 2).unwrap()
}

/// Host for [`loop_until_host_stops`]: reports the first iteration, then
/// continues until `stop` is set.
struct LoopHost {
    entered: Option<mpsc::Sender<()>>,
    stop: Arc<AtomicBool>,
    deadline: Instant,
}

impl LoopHost {
    fn new(entered: mpsc::Sender<()>, stop: Arc<AtomicBool>) -> Self {
        LoopHost {
            entered: Some(entered),
            stop,
            deadline: Instant::now() + HANDSHAKE,
        }
    }
}

impl Host for LoopHost {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        if let Some(tx) = self.entered.take() {
            tx.send(()).unwrap();
        }
        if Instant::now() > self.deadline {
            return Err("test loop ran past its deadline".to_string());
        }
        // Acquire: pairs with the test's Release store.
        Ok(Primitive::Bool(!self.stop.load(Ordering::Acquire)))
    }
}

#[test]
fn a_scope_delays_a_requested_collection_until_released_and_its_locals_are_roots() {
    let rt = Runtime::new();
    let doomed = new_rooted_object(&rt).downgrade();
    let (done_tx, done_rx) = mpsc::channel();
    thread::scope(|t| {
        rt.scope(|s| {
            let kept = s.new_object(); // a local only: never persisted
            let weak = s.downgrade(kept);
            t.spawn(|| done_tx.send(rt.request_collection()).unwrap());
            wait_until("the collection request", || rt.collection_requested());
            // This thread is inside a no-GC scope: the request cannot finish.
            assert_eq!(done_rx.try_recv().unwrap_err(), TryRecvError::Empty);
            assert_eq!(rt.heap_stats().collections, 0);
            assert!(doomed.is_live());

            // Releasing is a safepoint: the collection runs during it and
            // sees this scope's locals as roots.
            let stats = s.released(move || done_rx.recv_timeout(HANDSHAKE).unwrap().unwrap());
            assert_eq!((stats.live, stats.reclaimed), (1, 1));
            assert!(!doomed.is_live());
            assert!(weak.is_live());
            // The local is still valid after reentering.
            assert_eq!(s.view(kept), ValueView::Object);
            assert!(s.persist(kept).same_object(&weak.upgrade().unwrap()));
        });
    });
    assert_eq!(rt.heap_stats().collections, 1);
}

#[test]
fn a_collection_requested_after_the_scope_closes_does_not_wait() {
    let rt = Runtime::new();
    let weak = rt.scope(|s| {
        let o = s.new_object();
        s.downgrade(o)
    });
    let other = rt.clone();
    let stats = thread::spawn(move || other.request_collection())
        .join()
        .unwrap()
        .unwrap();
    assert_eq!((stats.live, stats.reclaimed), (0, 1));
    assert!(!weak.is_live());
    assert!(!rt.collection_requested());
}

#[test]
fn a_thread_that_never_enters_heap_access_does_not_block_collection() {
    let rt = Runtime::new();
    let root = new_rooted_object(&rt);
    let weak = root.downgrade();
    let (go_tx, go_rx) = mpsc::channel::<()>();
    let (ready_tx, ready_rx) = mpsc::channel();
    let idle = thread::spawn(move || {
        // Holds a persistent root but no heap access, and blocks.
        ready_tx.send(()).unwrap();
        go_rx.recv_timeout(HANDSHAKE).unwrap();
        root
    });
    ready_rx.recv_timeout(HANDSHAKE).unwrap();
    // A thread inside a released scope does not block it either.
    let (released_tx, released_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel::<()>();
    let releaser = {
        let rt = rt.clone();
        thread::spawn(move || {
            rt.scope(|s| {
                let local = s.new_object();
                s.released(move || {
                    released_tx.send(()).unwrap();
                    finish_rx.recv_timeout(HANDSHAKE).unwrap();
                });
                s.view(local)
            })
        })
    };
    released_rx.recv_timeout(HANDSHAKE).unwrap();

    let stats = rt.request_collection().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (2, 0), "root and local kept");
    assert_eq!(rt.collect().unwrap().reclaimed, 0, "no-wait collection too");

    finish_tx.send(()).unwrap();
    assert_eq!(releaser.join().unwrap(), ValueView::Object);
    go_tx.send(()).unwrap();
    let root = idle.join().unwrap();
    assert!(weak.upgrade().unwrap().same_object(&root));
    assert_eq!(
        rt.collect().unwrap().reclaimed,
        1,
        "the local died with its scope"
    );
}

#[test]
fn a_running_task_parks_at_a_safepoint_and_its_frames_are_roots() {
    let rt = Runtime::new();
    let kept = new_rooted_object(&rt);
    let doomed = new_rooted_object(&rt);
    let (weak_kept, weak_doomed) = (kept.downgrade(), doomed.downgrade());
    let task = rt
        .spawn(&loop_until_host_stops(), &[&kept, &doomed])
        .unwrap();
    drop((kept, doomed)); // reachable only from the task's frames now

    let stop = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered_rx) = mpsc::channel();
    let worker = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut host = LoopHost::new(entered_tx, stop);
            task.run_with(Budget::UNLIMITED, &mut host).unwrap()
        })
    };
    entered_rx.recv_timeout(HANDSHAKE).unwrap();
    // The worker is inside its run and cannot finish until `stop` is set,
    // so the collection happens while it is parked at a poll point.
    assert_eq!(rt.collect(), Err(CollectError::MutatorActive));
    let stats = rt.request_collection().unwrap();
    assert_eq!(stats.live, 1, "only the object in the running frame");
    assert!(
        stats.reclaimed >= 2,
        "the dropped argument and loop garbage"
    );
    assert!(weak_kept.is_live());
    assert!(!weak_doomed.is_live());

    stop.store(true, Ordering::Release);
    let result = complete(worker.join().unwrap());
    assert!(result.same_object(&weak_kept.upgrade().unwrap()));
}

/// The backward branch alone is a poll point: a loop that never allocates
/// or calls still lets a requested collection run, and its frame's object
/// survives.
#[test]
fn an_allocation_free_loop_parks_at_its_backward_branch() {
    // function(kept) { do {} while (host_effect(0, null)); return kept; }
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.bind(top);
    asm.constant(Primitive::Null).host_effect(0);
    asm.jump_if_false(done).jump(top);
    asm.bind(done);
    asm.get_local(0).op(Opcode::Return);
    let exe = asm.finish("allocation-free-loop", 1, 1).unwrap();

    let rt = Runtime::new();
    let kept = new_rooted_object(&rt);
    let weak = kept.downgrade();
    let garbage = new_rooted_object(&rt).downgrade();
    let task = rt.spawn(&exe, &[&kept]).unwrap();
    drop(kept);
    let stop = Arc::new(AtomicBool::new(false));
    let (entered_tx, entered_rx) = mpsc::channel();
    let worker = {
        let stop = Arc::clone(&stop);
        thread::spawn(move || {
            let mut host = LoopHost::new(entered_tx, stop);
            task.run_with(Budget::UNLIMITED, &mut host).unwrap()
        })
    };
    entered_rx.recv_timeout(HANDSHAKE).unwrap();
    // The loop cannot exit before `stop`, so the collection completes only
    // because the run parks at its backward branch.
    let stats = rt.request_collection().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (1, 1));
    assert!(weak.is_live() && !garbage.is_live());
    stop.store(true, Ordering::Release);
    let result = complete(worker.join().unwrap());
    assert!(result.same_object(&weak.upgrade().unwrap()));
}

#[test]
fn threads_entering_heap_access_wait_for_a_pending_request() {
    let rt = Runtime::new();
    let (done_tx, done_rx) = mpsc::channel();
    let (seen_tx, seen_rx) = mpsc::channel();
    thread::scope(|t| {
        rt.scope(|s| {
            t.spawn(|| done_tx.send(rt.request_collection()).unwrap());
            wait_until("the collection request", || rt.collection_requested());
            // A thread entering heap access now waits for the collection.
            t.spawn(|| {
                let seen = rt.scope(|_| rt.heap_stats().collections);
                seen_tx.send(seen).unwrap();
            });
            // A nested registration on a thread already in access does not
            // wait (it would deadlock): a nested scope, an allocation and an
            // atom operation all proceed while the request is pending.
            let atom_value = rt.scope(|inner| {
                let n = inner.primitive(Primitive::Number(1.0));
                let v = inner.new_vector(&[n]).unwrap();
                inner.persist(v)
            });
            let atom = rt.new_atom(&atom_value).unwrap();
            assert!(rt.atom_load(&atom).unwrap().same_object(&atom_value));
            assert_eq!(rt.heap_stats().collections, 0);
            assert_eq!(done_rx.try_recv().unwrap_err(), TryRecvError::Empty);
            s.released(move || {
                done_rx.recv_timeout(HANDSHAKE).unwrap().unwrap();
            });
        });
    });
    assert_eq!(seen_rx.recv_timeout(HANDSHAKE).unwrap(), 1);
}

#[test]
fn scope_safepoint_lets_a_requested_collection_run() {
    let rt = Runtime::new();
    let (done_tx, done_rx) = mpsc::channel();
    thread::scope(|t| {
        rt.scope(|s| {
            let local = s.new_object();
            s.safepoint(); // nothing requested: returns at once
            assert_eq!(rt.heap_stats().collections, 0);
            t.spawn(|| done_tx.send(rt.request_collection()).unwrap());
            wait_until("the collection request", || rt.collection_requested());
            s.safepoint(); // releases until the collection has run
            assert_eq!(rt.heap_stats().collections, 1);
            assert!(!rt.collection_requested());
            assert_eq!(s.view(local), ValueView::Object);
        });
    });
    let stats = done_rx.recv_timeout(HANDSHAKE).unwrap().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (1, 0));
}

#[test]
fn a_run_guard_holder_cannot_request_a_collection() {
    struct Requester {
        rt: Runtime,
        result: Option<Result<(), CollectError>>,
    }
    impl Host for Requester {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            self.result = Some(self.rt.request_collection().map(|_| ()));
            Ok(Primitive::Null)
        }
    }
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.constant(Primitive::Null)
        .host_effect(0)
        .op(Opcode::Return);
    let task = rt.spawn(&asm.finish("effect", 0, 0).unwrap(), &[]).unwrap();
    let mut host = Requester {
        rt: rt.clone(),
        result: None,
    };
    complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(host.result, Some(Err(CollectError::MutatorActive)));
    let guard = task.try_begin_run();
    assert!(guard.is_err(), "finished");
    let other = rt.spawn(&loop_until_host_stops(), &[]).unwrap();
    let guard = other.try_begin_run().unwrap();
    assert_eq!(rt.request_collection(), Err(CollectError::MutatorActive));
    assert!(
        !rt.collection_requested(),
        "a refused request is not left pending"
    );
    drop(guard);
    assert!(rt.request_collection().is_ok());
}

/// `function() { let v = []; let n = 0;
///   while (host_effect(0, n)) { new Object(); v = conj(v, new Object()); n = n + 1; }
///   return v; }`
fn grow_vector_while_host_says() -> Executable {
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.new_vector(0).set_local(0);
    asm.constant(Primitive::Number(0.0)).set_local(1);
    asm.bind(top);
    asm.get_local(1).host_effect(0).jump_if_false(done);
    asm.op(Opcode::NewObject).op(Opcode::Pop);
    asm.get_local(0)
        .op(Opcode::NewObject)
        .op(Opcode::Conj)
        .set_local(0);
    asm.get_local(1)
        .constant(Primitive::Number(1.0))
        .op(Opcode::Add)
        .set_local(1);
    asm.jump(top);
    asm.bind(done);
    asm.get_local(0).op(Opcode::Return);
    asm.finish("grow-vector", 0, 2).unwrap()
}
/// Two worker threads build vectors while a third requests collections
/// back to back; each worker keeps going until it has seen three
/// collections complete during its run. Every object reachable from a
/// running frame survives every collection, garbage is reclaimed, and
/// nothing deadlocks or starves. The interleaving is uncontrolled on
/// purpose: every interleaving must satisfy the assertions.
#[test]
fn collections_requested_while_two_tasks_run_keep_every_reachable_object() {
    struct UntilCollections {
        rt: Runtime,
        seen_at_start: Option<u64>,
        deadline: Instant,
    }
    impl Host for UntilCollections {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            if Instant::now() > self.deadline {
                return Err("worker ran past its deadline".to_string());
            }
            let now = self.rt.heap_stats().collections;
            let start = *self.seen_at_start.get_or_insert(now);
            Ok(Primitive::Bool(now < start + 3))
        }
    }
    let rt = Runtime::new();
    let exe = grow_vector_while_host_says();
    let finished = Arc::new(AtomicUsize::new(0));
    let start = Arc::new(Barrier::new(3));
    let workers: Vec<_> = (0..2)
        .map(|_| {
            let (rt, exe) = (rt.clone(), exe.clone());
            let (finished, start) = (Arc::clone(&finished), Arc::clone(&start));
            thread::spawn(move || {
                let task = rt.spawn(&exe, &[]).unwrap();
                let mut host = UntilCollections {
                    rt: rt.clone(),
                    seen_at_start: None,
                    deadline: Instant::now() + HANDSHAKE,
                };
                start.wait();
                let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
                finished.fetch_add(1, Ordering::Release);
                complete(outcome)
            })
        })
        .collect();
    start.wait();
    let deadline = Instant::now() + HANDSHAKE;
    let mut reclaimed = 0;
    while finished.load(Ordering::Acquire) < 2 {
        assert!(Instant::now() < deadline, "workers did not finish");
        reclaimed += rt.request_collection().unwrap().reclaimed;
    }
    let results: Vec<Rooted> = workers.into_iter().map(|w| w.join().unwrap()).collect();
    assert!(reclaimed > 0, "loop garbage was reclaimed");
    let elements = rt.scope(|s| {
        let mut elements = 0;
        for result in &results {
            let items = s.items(s.local(result).unwrap()).unwrap();
            assert!(!items.is_empty());
            for (i, &item) in items.iter().enumerate() {
                assert_eq!(s.view(item), ValueView::Object, "element {i}");
                assert!(s.downgrade(item).is_live());
            }
            elements += items.len();
        }
        elements
    });
    let stats = rt.collect().unwrap();
    assert_eq!(stats.live, results.len() + elements, "results and elements");
}
