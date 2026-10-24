//! Garbage collection through the embedding API: persistent roots, scoped
//! access, weak handles, task frames, and the "collect now" entry point.
//!
//! Early VM tests: programs are hand-assembled bytecode, not JavaScript
//! source support.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use clj_quickjs::{
    Assembler, Budget, CollectError, Executable, HandleError, Host, HostCall, Opcode, Primitive,
    Rooted, RunOutcome, Runtime, ValueView,
};

/// Bound for every cross-thread handshake: a broken handshake fails the test
/// instead of hanging it.
const HANDSHAKE: Duration = Duration::from_secs(10);

fn complete(outcome: RunOutcome) -> Rooted {
    match outcome {
        RunOutcome::Complete(v) => v,
        other => panic!("expected Complete, got {other:?}"),
    }
}

#[test]
fn suspended_generator_operands_and_completions_are_traced() {
    // Synthetic fixtures, not light-cljs output. Only a saved operand or
    // pending completion retains the object while the task waits.
    use clj_quickjs::frontend::compile_script;
    for body in [
        "function read(a,b) { return a; } return read({n:17}, yield 0);",
        "try { return {n:17}; } finally { yield 0; }",
        "try { throw {n:17}; } finally { yield 0; }",
        "return yield* (function() { let held={n:17}; let count=0; let it={next() { return count++ === 0 ? {value:0,done:false} : {value:held,done:true}; }}; it[Symbol.iterator]=function() { return this; }; return it; })();",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "function* f() {{ {body} }} let g=f(); g.next(); __rt.need(90,1); function a() {{ return b(); }} function b() {{ try {{ return g.next().value; }} catch(e) {{ return e; }} }} a().n"
        );
        let code = compile_script("generator-saved-values", &source).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        rt.collect().unwrap();
        assert_eq!(rt.heap_usage().objects.generator, 1);
        let request = &task.take_producer_requests()[0];
        let ready = rt.root(Primitive::Number(0.0));
        let resumed = rt
            .complete(
                request.key(),
                request.producer(),
                clj_quickjs::Completion::Ready(&ready),
            )
            .unwrap()
            .into_runnable();
        assert_eq!(
            complete(resumed[0].run().unwrap()).view(),
            ValueView::Number(17.0),
            "{body}"
        );
        drop(resumed);
        drop(task);
        let collected = rt.collect().unwrap();
        assert!(collected.reclaimed >= 2, "{body}");
        assert_eq!(rt.heap_usage().objects.generator, 0);
    }
}

#[test]
fn suspended_generator_is_traced_and_reclaimed() {
    // Synthetic source fixture, not light-cljs output. Only the saved
    // activation retains `held` after the first next returns.
    use clj_quickjs::frontend::compile_script;
    let rt = Runtime::new();
    let exe = compile_script(
        "generator-roots",
        r#"
        let g = (function*() { let held = {n: 17}; yield 0; return held; })();
        g.next();
        __rt.need(90, 1);
        let value = g.next().value;
        if (value.n !== 17) throw 'lost local';
        g = null; value = null;
        __rt.need(91, 1);
        1
    "#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    let before = rt.heap_stats().objects;
    let counts = rt.heap_usage().objects;
    assert_eq!(counts.generator, 1);
    let requests = task.take_producer_requests();
    let request = &requests[0];
    let ready = rt.root(Primitive::Number(0.0));
    let resumed = rt
        .complete(
            request.key(),
            request.producer(),
            clj_quickjs::Completion::Ready(&ready),
        )
        .unwrap()
        .into_runnable();
    assert!(matches!(resumed[0].run().unwrap(), RunOutcome::Waiting(_)));
    let collected = rt.collect().unwrap();
    assert!(collected.reclaimed >= 2);
    assert!(rt.heap_stats().objects < before);
    assert_eq!(rt.heap_usage().objects.generator, 0);
}

fn new_rooted_object(rt: &Runtime) -> Rooted {
    rt.scope(|s| {
        let o = s.new_object();
        s.persist(o)
    })
}

#[test]
fn a_weak_handle_alone_does_not_keep_an_object_alive() {
    let rt = Runtime::new();
    let weak = rt.scope(|s| {
        let o = s.new_object();
        s.downgrade(o)
    });
    assert!(weak.is_live(), "nothing has collected yet");
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (0, 1));
    assert!(!weak.is_live());
    assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    rt.scope(|s| assert_eq!(s.upgrade(&weak).unwrap_err(), HandleError::Reclaimed));
}

#[test]
fn stale_handles_are_rejected_even_after_their_storage_is_reused() {
    let rt = Runtime::new();
    let doomed = new_rooted_object(&rt);
    let weak = doomed.downgrade();
    drop(doomed);
    rt.collect().unwrap();
    // New objects reuse the freed storage; the stale handle must not see them.
    let newcomers: Vec<Rooted> = (0..8).map(|_| new_rooted_object(&rt)).collect();
    assert_eq!(rt.heap_stats().objects, 8);
    assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    for n in &newcomers {
        assert!(n.downgrade().upgrade().unwrap().same_object(n));
    }
}

#[test]
fn host_roots_retain_objects_until_every_root_is_released() {
    let rt = Runtime::new();
    let root = new_rooted_object(&rt);
    let weak = root.downgrade();
    let second = root.clone();
    for _ in 0..3 {
        rt.collect().unwrap();
        assert!(weak.is_live());
    }
    drop(root);
    rt.collect().unwrap();
    let again = weak.upgrade().expect("still rooted by the clone");
    assert!(
        again.same_object(&second),
        "identity is stable across collections"
    );
    drop((second, again));
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert!(!weak.is_live());
    assert_eq!(rt.heap_stats().persistent_roots, 0);
}

#[test]
fn scope_locals_are_roots_while_the_scope_is_open() {
    let rt = Runtime::new();
    let weak = rt.scope(|s| {
        let kept = s.new_object();
        let weak = s.downgrade(kept);
        let stats = rt
            .collect()
            .expect("the collecting thread's own scope is fine");
        assert_eq!((stats.live, stats.reclaimed), (1, 0));
        // The local still names the same object.
        let again = s.upgrade(&weak).unwrap();
        assert!(s.same_object(kept, again));
        weak
    });
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert!(!weak.is_live());
}

/// `function f(o) { let mine = {}; <yield point>; return mine; }`.
/// Locals: 0 = o (argument), 1 = mine.
fn hold_then_return_own() -> Executable {
    let mut asm = Assembler::new();
    asm.op(Opcode::NewObject).set_local(1); // 0, 1
    asm.get_local(1).op(Opcode::Return); // 2, 3
    asm.finish("hold-then-return-own", 1, 2).unwrap()
}

struct NoEffects;

impl Host for NoEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        Err(format!("unexpected effect {}", call.op))
    }
}

#[test]
fn task_frames_retain_values_between_runs_and_results_outlive_the_task() {
    let rt = Runtime::new();
    let arg = new_rooted_object(&rt);
    let arg_weak = arg.downgrade();
    let task = rt.spawn(&hold_then_return_own(), &[&arg]).unwrap();
    drop(arg); // From here the argument is reachable only from the task frame.

    let outcome = task
        .run_with(Budget::instructions(2), &mut NoEffects)
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Yielded));
    for _ in 0..2 {
        let stats = rt.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (2, 0), "both held by frames");
        assert!(arg_weak.is_live());
    }

    let result = complete(task.run().unwrap());
    let result_weak = result.downgrade();
    // Finishing released the frames (the argument is garbage) but not every
    // allocation of the task: the returned object survives through its root.
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (1, 1));
    assert!(!arg_weak.is_live());
    assert_eq!(result.view(), ValueView::Object);
    drop(task);
    rt.collect().unwrap();
    assert!(result_weak.upgrade().unwrap().same_object(&result));

    drop(result);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert_eq!(result_weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    assert_eq!(rt.heap_stats().objects, 0);
}

#[test]
fn task_operands_retain_values_between_runs() {
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.op(Opcode::NewObject).op(Opcode::Return);
    let task = rt
        .spawn(&asm.finish("pending-return", 0, 0).unwrap(), &[])
        .unwrap();
    let outcome = task
        .run_with(Budget::instructions(1), &mut NoEffects)
        .unwrap();
    assert!(matches!(outcome, RunOutcome::Yielded));
    // The object is reachable only from the saved operand stack.
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (1, 0));
    let result = complete(task.run().unwrap());
    let weak = result.downgrade();
    rt.collect().unwrap();
    assert!(weak.upgrade().unwrap().same_object(&result));
    drop(result);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
}

#[test]
fn weak_upgrade_roots_an_otherwise_unrooted_live_object() {
    let rt = Runtime::new();
    // The scope has closed: nothing roots the object, but no collection ran.
    let weak = rt.scope(|s| {
        let o = s.new_object();
        s.downgrade(o)
    });
    let revived = weak.upgrade().expect("not collected yet");
    for _ in 0..2 {
        let stats = rt.collect().unwrap();
        assert_eq!((stats.live, stats.reclaimed), (1, 0));
    }
    assert!(weak.upgrade().unwrap().same_object(&revived));
    drop(revived);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
}

#[test]
fn collection_recovers_after_a_host_panic() {
    struct Panic;
    impl Host for Panic {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            panic!("host bug")
        }
    }
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.constant(Primitive::Null)
        .host_effect(0)
        .op(Opcode::Return);
    let exe = asm.finish("panicking-effect", 1, 1).unwrap();
    let arg = new_rooted_object(&rt);
    let weak = arg.downgrade();
    let task = rt.spawn(&exe, &[&arg]).unwrap();
    drop(arg); // Now reachable only from the task's locals.
    let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        task.run_with(Budget::UNLIMITED, &mut Panic)
    }));
    assert!(run.is_err());
    // The unwound guard deregistered its mutator, and the poisoned task's
    // saved state is still traced.
    let stats = rt.collect().expect("no mutator leaked by the panic");
    assert_eq!((stats.live, stats.reclaimed), (1, 0));
    assert!(weak.is_live());
    drop(task);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert!(!weak.is_live());
}

#[test]
fn collection_refuses_while_a_run_guard_exists() {
    let rt = Runtime::new();
    let arg = new_rooted_object(&rt);
    let task = rt.spawn(&hold_then_return_own(), &[&arg]).unwrap();
    let guard = task.try_begin_run().unwrap();
    assert_eq!(rt.collect(), Err(CollectError::MutatorActive));
    drop(guard);
    assert!(rt.collect().is_ok());
    assert_eq!(rt.heap_stats().collections, 1);
}

#[test]
fn collection_refuses_inside_a_host_effect() {
    struct Collector {
        rt: Runtime,
        result: Option<Result<(), CollectError>>,
    }
    impl Host for Collector {
        fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
            self.result = Some(self.rt.collect().map(|_| ()));
            Ok(Primitive::Null)
        }
    }
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.op(Opcode::NewObject).set_local(0);
    asm.constant(Primitive::Null).host_effect(1).op(Opcode::Pop);
    asm.get_local(0).op(Opcode::Return);
    let exe = asm.finish("effect-holding-object", 0, 1).unwrap();
    let mut host = Collector {
        rt: rt.clone(),
        result: None,
    };
    let task = rt.spawn(&exe, &[]).unwrap();
    let result = complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap());
    assert_eq!(host.result, Some(Err(CollectError::MutatorActive)));
    assert_eq!(result.view(), ValueView::Object);
    assert_eq!(rt.heap_stats().collections, 0);
}

#[test]
fn collection_refuses_while_another_thread_has_a_scope_open() {
    let rt = Runtime::new();
    let (opened_tx, opened_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel::<()>();
    let worker = {
        let rt = rt.clone();
        thread::spawn(move || {
            rt.scope(|s| {
                let o = s.new_object();
                opened_tx.send(s.downgrade(o)).unwrap();
                // Hold the scope (and its unpersisted local) until released.
                release_rx.recv_timeout(HANDSHAKE).unwrap();
                s.view(o)
            })
        })
    };
    let weak = opened_rx.recv_timeout(HANDSHAKE).unwrap();
    assert_eq!(rt.collect(), Err(CollectError::MutatorActive));
    assert!(weak.is_live());
    release_tx.send(()).unwrap();
    assert_eq!(worker.join().unwrap(), ValueView::Object);
    assert_eq!(rt.collect().unwrap().reclaimed, 1);
    assert!(!weak.is_live());
}

#[test]
fn roots_from_other_threads_are_traced() {
    let rt = Runtime::new();
    let (tx, rx) = mpsc::channel();
    let worker = {
        let rt = rt.clone();
        thread::spawn(move || {
            let root = new_rooted_object(&rt);
            tx.send(root.downgrade()).unwrap();
            root
        })
    };
    let weak = rx.recv_timeout(HANDSHAKE).unwrap();
    let root = worker.join().unwrap();
    rt.collect().unwrap();
    assert!(weak.upgrade().unwrap().same_object(&root));
}

/// Root creation and release on one thread interleave with collections on
/// another: rooted objects are never reclaimed and nothing deadlocks. The
/// interleaving is uncontrolled because every interleaving must satisfy the
/// assertions; the test is bounded by `HANDSHAKE`, not by sleeps.
#[test]
fn root_changes_on_another_thread_interleave_safely_with_collection() {
    let rt = Runtime::new();
    let keep = new_rooted_object(&rt);
    let weak = keep.downgrade();
    let (done_tx, done_rx) = mpsc::channel();
    let worker = {
        let weak = weak.clone();
        thread::spawn(move || {
            for _ in 0..2000 {
                let again = weak.upgrade().expect("kept alive by `keep`");
                let copy = again.clone();
                assert!(copy.same_object(&again));
                assert!(again.downgrade().is_live());
            }
            done_tx.send(()).unwrap();
        })
    };
    let deadline = Instant::now() + HANDSHAKE;
    while done_rx.try_recv().is_err() {
        assert!(Instant::now() < deadline, "worker did not finish");
        rt.collect().unwrap();
        assert!(weak.is_live());
    }
    worker.join().unwrap();
    let stats = rt.collect().unwrap();
    assert_eq!((stats.live, stats.reclaimed), (1, 0));
    assert_eq!(rt.heap_stats().persistent_roots, 1);
    assert!(weak.upgrade().unwrap().same_object(&keep));
}

/// clj-quickjs-65: strings are primitives outside the object table
/// (immutable `Arc` storage, `docs/architecture.md` "String values"). A
/// string held only by a frame local, by a closure capture, or by a
/// persistent collection keeps its contents across collections between
/// budgeted runs, and adds no heap object. The program is JavaScript source
/// (a synthetic fixture).
#[test]
fn strings_in_frames_captures_and_collections_survive_collections() {
    let rt = Runtime::new();
    let exe = clj_quickjs::frontend::compile_script(
        "strings",
        r#"let local = "lo" + "cal";
           let captured = "cap" + "tured";
           let read = () => captured;
           let m = __rt.map("k", "in" + " map");
           let i = 0;
           while (i < 20) { i = i + 1; }
           local === "local" && read() === "captured" && __rt.get(m, "k") === "in map""#,
    )
    .unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut runs = 0;
    let result = loop {
        runs += 1;
        match task
            .run_with(Budget::instructions(7), &mut NoEffects)
            .unwrap()
        {
            RunOutcome::Yielded => {
                let stats = rt.collect().unwrap();
                // At most the closure and the map: no string is an object.
                assert!(stats.live <= 2, "{stats:?}");
                assert_eq!(stats.reclaimed, 0);
            }
            other => break complete(other),
        }
    };
    assert!(runs > 5, "collections ran mid-program ({runs} runs)");
    assert_eq!(result.view(), ValueView::Bool(true));
}

/// Synthetic JavaScript fixture, not light-cljs output. The Map is the
/// only root path to both objects after the task and host roots are dropped.
#[test]
fn map_entries_are_traced() {
    use clj_quickjs::frontend::compile_function;
    let rt = Runtime::new();
    let key = new_rooted_object(&rt);
    let value = new_rooted_object(&rt);
    let key_weak = key.downgrade();
    let value_weak = value.downgrade();
    let code =
        compile_function("function make(k, v) { let m = new Map(); m.set(k, v); return m; }")
            .unwrap();
    let task = rt.spawn(&code, &[&key, &value]).unwrap();
    let map = complete(task.run().unwrap());
    let map_weak = map.downgrade();
    drop((task, key, value));
    rt.collect().unwrap();
    assert!(key_weak.upgrade().is_ok());
    assert!(value_weak.upgrade().is_ok());
    assert!(map_weak.upgrade().unwrap().same_object(&map));
    drop(map);
    rt.collect().unwrap();
    for weak in [key_weak, value_weak, map_weak] {
        assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    }
}

/// Tombstones retain positions, never the removed key/value heap edges.
#[test]
fn map_deleted_and_cleared_entries_are_reclaimed() {
    use clj_quickjs::frontend::compile_function;
    for operation in ["m.delete(k)", "m.clear()"] {
        let rt = Runtime::new();
        let key = new_rooted_object(&rt);
        let value = new_rooted_object(&rt);
        let key_weak = key.downgrade();
        let value_weak = value.downgrade();
        let code = compile_function(&format!(
            "function make(k, v) {{ let m = new Map(); m.set(k, v); {operation}; return m; }}"
        ))
        .unwrap();
        let task = rt.spawn(&code, &[&key, &value]).unwrap();
        let map = complete(task.run().unwrap());
        drop((task, key, value));
        rt.collect().unwrap();
        assert_eq!(key_weak.upgrade().unwrap_err(), HandleError::Reclaimed);
        assert_eq!(value_weak.upgrade().unwrap_err(), HandleError::Reclaimed);
        assert!(map.downgrade().is_live());
    }
}

/// Synthetic fixture, not light-cljs output: these edges are independent
/// of the entry storage, and the Map is their only remaining root path.
#[test]
fn map_properties_and_prototype_are_traced() {
    use clj_quickjs::frontend::compile_function;
    let rt = Runtime::new();
    let prototype = new_rooted_object(&rt);
    let property = new_rooted_object(&rt);
    let prototype_weak = prototype.downgrade();
    let property_weak = property.downgrade();
    let code = compile_function("function make(p, v) { let m = new Map(); m.own = v; Object.setPrototypeOf(m, p); return m; }").unwrap();
    let task = rt.spawn(&code, &[&prototype, &property]).unwrap();
    let map = complete(task.run().unwrap());
    let map_weak = map.downgrade();
    drop((task, prototype, property));
    rt.collect().unwrap();
    assert!(prototype_weak.upgrade().is_ok());
    assert!(property_weak.upgrade().is_ok());
    assert!(map_weak.upgrade().unwrap().same_object(&map));
    drop(map);
    rt.collect().unwrap();
    for weak in [prototype_weak, property_weak, map_weak] {
        assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    }
}

/// Synthetic source fixture, not light-cljs output. The buffer and ordinary
/// property/prototype edges are reachable only through the returned view.
#[test]
fn typed_array_retains_buffer_and_properties() {
    use clj_quickjs::frontend::compile_function;
    let rt = Runtime::new();
    let property = new_rooted_object(&rt);
    let prototype = new_rooted_object(&rt);
    let property_weak = property.downgrade();
    let prototype_weak = prototype.downgrade();
    let code = compile_function("function make(p, proto) { let a = new Uint16Array(4); a[0] = 42; a.own = p; Object.setPrototypeOf(a, proto); return a; }").unwrap();
    let task = rt.spawn(&code, &[&property, &prototype]).unwrap();
    let array = complete(task.run().unwrap());
    let array_weak = array.downgrade();
    drop((task, property, prototype));
    rt.collect().unwrap();
    assert!(property_weak.is_live());
    assert!(prototype_weak.is_live());
    assert_eq!(rt.heap_usage().objects.array_buffer, 1);
    assert_eq!(rt.heap_usage().objects.typed_array, 1);
    assert_eq!(rt.heap_usage().array_buffer_bytes, 8);
    drop(array);
    rt.collect().unwrap();
    assert!(!array_weak.is_live());
    assert!(!property_weak.is_live());
    assert!(!prototype_weak.is_live());
    assert_eq!(rt.heap_usage().objects.array_buffer, 0);
    assert_eq!(rt.heap_usage().objects.typed_array, 0);
    assert_eq!(rt.heap_usage().array_buffer_bytes, 0);
}
