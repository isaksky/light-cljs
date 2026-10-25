//! Garbage collection through the embedding API: persistent roots, scoped
//! access, weak handles, task frames, and the "collect now" entry point.
//!
//! Early VM tests: programs are hand-assembled bytecode, not JavaScript
//! source support.

use std::sync::mpsc;
use std::thread;
use std::time::{Duration, Instant};

use big_boy_pants_js::{
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

/// Synthetic fixtures, not light-cljs output. A timer collects after the
/// script's kept-object boundary, with and without an ordinary strong root.
#[test]
fn weak_ref_cleared_after_job_and_collection() {
    use big_boy_pants_js::frontend::compile_script;
    #[derive(Default)]
    struct Record(Vec<bool>);
    impl Host for Record {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            let Primitive::Bool(value) = call.arg else {
                panic!("bool expected")
            };
            self.0.push(value);
            Ok(Primitive::Undefined)
        }
    }
    for target in ["{}", "Symbol('target')"] {
        for strong in [false, true] {
            let rt = Runtime::new();
            let source = format!(
                "let target = {target}; const w = new WeakRef(target); if (!{strong}) target = null; os.setTimeout(() => {{ std.gc(); __rt.effect(0, w.deref() === undefined); if ({strong} && w.deref() !== target) throw 'lost strong target'; }}, 0); 42"
            );
            let task = rt
                .spawn(&compile_script("weak-ref-next-job", &source).unwrap(), &[])
                .unwrap();
            task.install_sim_host_bindings().unwrap();
            let mut host = Record::default();
            assert_eq!(
                complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()).view(),
                ValueView::Number(42.0)
            );
            assert_eq!(host.0, [!strong], "{target}, strong={strong}");
        }
    }
}

/// Synthetic fixtures, not light-cljs output. Only the task's kept set
/// retains the constructor target while the owning task is Waiting.
#[test]
fn kept_objects_survive_need_wait() {
    use big_boy_pants_js::frontend::compile_script;
    for (target, kind) in [("{}", "object"), ("Symbol('target')", "symbol")] {
        let rt = Runtime::new();
        let source = format!(
            "let w = new WeakRef({target}); __rt.need(176, 1); typeof w.deref() === '{kind}'"
        );
        let task = rt
            .spawn(&compile_script("weak-ref-kept", &source).unwrap(), &[])
            .unwrap();
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        rt.collect().unwrap();
        let request = task.take_producer_requests().pop().unwrap();
        let ready = rt.root(Primitive::Undefined);
        let wakes = rt
            .complete(
                request.key(),
                request.producer(),
                big_boy_pants_js::Completion::Ready(&ready),
            )
            .unwrap();
        assert_eq!(wakes.task_ids(), [task.id()]);
        // Migration does not clear the set either.
        let result = thread::scope(|s| s.spawn(|| complete(task.run().unwrap())).join().unwrap());
        assert_eq!(result.view(), ValueView::Bool(true), "{target}");
    }
}

/// Synthetic fixtures, not light-cljs output. Constructor, successful
/// forwarded/native deref, and every subsequent job have separate lifetimes.
#[test]
fn weak_ref_deref_keeps_target_until_its_job_ends() {
    use big_boy_pants_js::frontend::compile_script;
    #[derive(Default)]
    struct Record(Vec<Primitive>);
    impl Host for Record {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            self.0.push(call.arg);
            Ok(Primitive::Undefined)
        }
    }
    for (target, kind) in [("{}", "object"), ("Symbol('target')", "symbol")] {
        for deref in [
            "w.deref()",
            "Reflect.apply(WeakRef.prototype.deref, w, [])",
            "__rt.reduce(w.deref.bind(w), 0, __rt.vector(1))",
        ] {
            let rt = Runtime::new();
            let source = format!(
                r#"
            let target = {target}; let w = new WeakRef(target);
            os.setTimeout(() => {{
                {deref}; target = null; std.gc();
                if (typeof w.deref() !== '{kind}') throw 'lost deref target';
                __rt.need(176, 1);
                __rt.effect(0, typeof w.deref() === '{kind}');
                Promise.resolve().then(() => {{
                    std.gc(); __rt.effect(0, w.deref() === undefined);
                }});
            }}, 0);
            os.setTimeout(() => {{
                std.gc(); __rt.effect(0, w.deref() === undefined);
            }}, 0);
            true
        "#
            );
            let task = rt
                .spawn(&compile_script("weak-ref-deref-job", &source).unwrap(), &[])
                .unwrap();
            task.install_sim_host_bindings().unwrap();
            let mut host = Record::default();
            let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
            assert!(
                matches!(outcome, RunOutcome::Waiting(_)),
                "{deref}: {outcome:?}"
            );
            rt.collect().unwrap();
            let request = task.take_producer_requests().pop().unwrap();
            let ready = rt.root(Primitive::Undefined);
            let wakes = rt
                .complete(
                    request.key(),
                    request.producer(),
                    big_boy_pants_js::Completion::Ready(&ready),
                )
                .unwrap();
            assert_eq!(wakes.task_ids(), [task.id()]);
            // Resume the timer on a different worker, in the same job.
            let outcome = thread::scope(|s| {
                s.spawn(|| task.run_with(Budget::UNLIMITED, &mut host).unwrap())
                    .join()
                    .unwrap()
            });
            assert_eq!(complete(outcome).view(), ValueView::Bool(true), "{deref}");
            assert_eq!(
                host.0,
                [
                    Primitive::Bool(true),
                    Primitive::Bool(true),
                    Primitive::Bool(true)
                ],
                "{target}: {deref}"
            );
        }
    }
}

/// Synthetic fixture, not light-cljs output. Collect before the first deref
/// so the constructor's kept root is the sole strong path to each target.
#[test]
fn weak_ref_constructor_keeps_target_at_collection_point() {
    use big_boy_pants_js::frontend::compile_script;
    for (target, kind) in [("{}", "object"), ("Symbol('target')", "symbol")] {
        let rt = Runtime::new();
        let source =
            format!("let w = new WeakRef({target}); std.gc(); typeof w.deref() === '{kind}'");
        let task = rt
            .spawn(
                &compile_script("weak-ref-constructor", &source).unwrap(),
                &[],
            )
            .unwrap();
        task.install_sim_host_bindings().unwrap();
        assert_eq!(
            complete(task.run().unwrap()).view(),
            ValueView::Bool(true),
            "{target}"
        );
    }
}

/// Synthetic fixture, not light-cljs output. Weak targets do not weaken
/// ordinary property/prototype edges, and those edges die with their owner.
#[test]
fn weak_ref_properties_and_prototype_are_traced() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let code = compile_function("function make() { let p = {}; let v = {}; let w = new WeakRef({}); w.own = v; Object.setPrototypeOf(w, p); return __rt.vector(w, p, v); }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let result = complete(task.run().unwrap());
    let (weak_ref, edges) = rt.scope(|s| {
        let r = s.local(&result).unwrap();
        (
            s.persist(s.nth(r, 0).unwrap()),
            [
                s.downgrade(s.nth(r, 1).unwrap()),
                s.downgrade(s.nth(r, 2).unwrap()),
            ],
        )
    });
    drop((task, result));
    rt.collect().unwrap();
    assert!(edges.iter().all(|edge| edge.is_live()));
    drop(weak_ref);
    rt.collect().unwrap();
    assert!(edges.iter().all(|edge| !edge.is_live()));
    assert_eq!(rt.heap_usage().objects.weak_ref, 0);
}

/// Synthetic fixture, not light-cljs output. Neither a VM budget yield nor
/// a detached generator activation ends the enclosing task's current job.
#[test]
fn weak_ref_kept_objects_survive_yields_and_generator_suspend() {
    use big_boy_pants_js::frontend::compile_script;
    let rt = Runtime::new();
    let code = compile_script("weak-ref-generator", "function* make() { let w = new WeakRef({}); yield 0; return typeof w.deref() === 'object'; } let g = make(); g.next(); g.next().value").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let mut host = big_boy_pants_js::host::sim::SimHost::new(&rt);
    // A fixed instruction bound fails instead of hanging on a broken resume.
    for _ in 0..1000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                rt.collect().unwrap();
            }
            RunOutcome::Complete(value) => {
                assert_eq!(value.view(), ValueView::Bool(true));
                return;
            }
            other => panic!("unexpected {other:?}"),
        }
    }
    panic!("generator did not finish");
}

#[test]
fn suspended_generator_operands_and_completions_are_traced() {
    // Synthetic fixtures, not light-cljs output. Only a saved operand or
    // pending completion retains the object while the task waits.
    use big_boy_pants_js::frontend::compile_script;
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
                big_boy_pants_js::Completion::Ready(&ready),
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
    use big_boy_pants_js::frontend::compile_script;
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
            big_boy_pants_js::Completion::Ready(&ready),
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
    let exe = big_boy_pants_js::frontend::compile_script(
        "strings",
        // Block locals isolate string storage from the script global
        // environment's independently allocated lexical cells.
        r#"{ let local = "lo" + "cal";
           let captured = "cap" + "tured";
           let read = () => captured;
           let m = __rt.map("k", "in" + " map");
           let i = 0;
           while (i < 20) { i = i + 1; }
           local === "local" && read() === "captured" && __rt.get(m, "k") === "in map" }"#,
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
    use big_boy_pants_js::frontend::compile_function;
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
    use big_boy_pants_js::frontend::compile_function;
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
    use big_boy_pants_js::frontend::compile_function;
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
    use big_boy_pants_js::frontend::compile_function;
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

/// Adapted from QuickJS tests/test_builtin.js test_weak_map at revision
/// 7d1c13c5d8f8. Synthetic fixture, not light-cljs output. Copyright and
/// MIT permission notice: docs/attribution.md, "JavaScript WeakMap tests".
#[test]
fn weak_map_drops_entries_of_unreachable_keys() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let code = compile_function(
        r#"function make() {
        let a = new WeakMap(); let tab = []; let values = __rt.vector(); let o;
        for (let i = 0; i < 10; i++) {
            let v = {};
            if (i & 1) o = Symbol('x' + i); else o = {id: i};
            tab[i] = [o, v]; values = __rt.conj(values, v); a.set(o, v);
        }
        o = null;
        for (let i = 0; i < 5; i++) a.delete(tab[i][0]);
        for (let i = 5; i < 10; i++) tab[i][0] = null;
        return __rt.vector(a, values);
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let result = complete(task.run().unwrap());
    let (map, weak_values) = rt.scope(|s| {
        let result = s.local(&result).unwrap();
        let map = s.persist(s.nth(result, 0).unwrap());
        let values = s.nth(result, 1).unwrap();
        let weak = (0..10)
            .map(|i| s.downgrade(s.nth(values, i).unwrap()))
            .collect::<Vec<_>>();
        (map, weak)
    });
    assert_eq!(map.weak_map_live_entry_count(), Some(5));
    drop((task, result));
    rt.collect().unwrap();
    assert_eq!(map.weak_map_live_entry_count(), Some(0));
    assert!(map.downgrade().is_live());
    for weak in weak_values {
        assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    }
}

/// QuickJS test_weak_map_cycles, same source/revision and notice as above.
/// The second value is also an object so both value lifetimes are observable.
#[test]
fn weak_map_cycles_are_reclaimed() {
    use big_boy_pants_js::frontend::{compile_function, compile_script};
    let rt = Runtime::new();
    let code = compile_function(
        r#"function make() {
        const weak1 = new WeakMap(); const weak2 = new WeakMap();
        const parent = {}; const child = {parent}; parent.child = child;
        const value1 = {}; const value2 = {};
        weak1.set(child, value1); weak2.set(value1, value2);
        return __rt.vector(weak1, weak2, parent, child, value1, value2);
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let result = complete(task.run().unwrap());
    let (maps, weak) = rt.scope(|s| {
        let r = s.local(&result).unwrap();
        let maps = [
            s.persist(s.nth(r, 0).unwrap()),
            s.persist(s.nth(r, 1).unwrap()),
        ];
        let weak = (2..6)
            .map(|i| s.downgrade(s.nth(r, i).unwrap()))
            .collect::<Vec<_>>();
        (maps, weak)
    });
    drop((task, result));
    rt.collect().unwrap();
    for map in &maps {
        assert_eq!(map.weak_map_live_entry_count(), Some(0));
    }
    for weak in weak {
        assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    }
    rt.collect().unwrap();

    // Exercise get after collection on the owning task. The two needs are
    // deterministic GC boundaries; no task transfer bypasses ownership.
    let code = compile_script(
        "weak-cycle-resume",
        r#"
        const weak1 = new WeakMap(); const weak2 = new WeakMap();
        (function() { const parent = {}; const child = {parent}; parent.child = child;
            const value = {}; weak1.set(child, value); weak2.set(value, 1); })();
        __rt.need(91, 1);
        if (weak2.get({}) !== undefined) throw 'stale entry';
        __rt.need(92, 1);
        __rt.vector(weak1, weak2)
    "#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    for _ in 0..2 {
        assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
        rt.collect().unwrap();
        let requests = task.take_producer_requests();
        assert_eq!(requests.len(), 1);
        let r = &requests[0];
        let ready = rt.root(Primitive::Undefined);
        let wakeups = rt
            .complete(
                r.key(),
                r.producer(),
                big_boy_pants_js::Completion::Ready(&ready),
            )
            .unwrap();
        assert_eq!(wakeups.task_ids(), vec![task.id()]);
    }
    let result = complete(task.run().unwrap());
    rt.scope(|s| {
        let r = s.local(&result).unwrap();
        for i in 0..2 {
            assert_eq!(
                s.persist(s.nth(r, i).unwrap()).weak_map_live_entry_count(),
                Some(0)
            );
        }
    });
}

/// Synthetic fixture, not light-cljs output. A symbol rooted as a primitive
/// participates in exactly the same ephemeron fixpoint as an object key.
#[test]
fn weak_map_value_kept_while_key_reachable() {
    use big_boy_pants_js::frontend::compile_function;
    for key in ["{}", "Symbol('s')"] {
        let rt = Runtime::new();
        let code = compile_function(&format!("function make() {{ let m = new WeakMap(); let k = {key}; let v = {{}}; m.set(k, v); return __rt.vector(m, k, v); }}")).unwrap();
        let task = rt.spawn(&code, &[]).unwrap();
        let result = complete(task.run().unwrap());
        let (map, key, weak) = rt.scope(|s| {
            let r = s.local(&result).unwrap();
            (
                s.persist(s.nth(r, 0).unwrap()),
                s.persist(s.nth(r, 1).unwrap()),
                s.downgrade(s.nth(r, 2).unwrap()),
            )
        });
        drop((task, result));
        for _ in 0..2 {
            rt.collect().unwrap();
            assert_eq!(map.weak_map_live_entry_count(), Some(1));
            assert!(weak.is_live());
        }
        drop(key);
        rt.collect().unwrap();
        assert_eq!(map.weak_map_live_entry_count(), Some(0));
        assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    }
}

/// The symbol intern table and Arc clones in garbage are not roots.
/// Only symbol values/keys reached through the tracing graph activate entries.
#[test]
fn weak_map_symbol_keys_follow_traced_reachability() {
    use big_boy_pants_js::frontend::compile_function;
    for holder in [
        "(function() { let o = {}; o[s] = 1; return o; })()",
        "{symbol: s}",
        "new Map().set(s, 1)",
        "Object(s)",
        "function() { return s; }",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "function make() {{ const s = Symbol('key'); const v = {{}}; const m = new WeakMap(); m.set(s, v); return __rt.vector(m, {holder}, v); }}"
        );
        let task = rt.spawn(&compile_function(&source).unwrap(), &[]).unwrap();
        let result = complete(task.run().unwrap());
        let (map, holder, weak) = rt.scope(|s| {
            let r = s.local(&result).unwrap();
            (
                s.persist(s.nth(r, 0).unwrap()),
                s.persist(s.nth(r, 1).unwrap()),
                s.downgrade(s.nth(r, 2).unwrap()),
            )
        });
        drop((task, result));
        rt.collect().unwrap();
        assert_eq!(map.weak_map_live_entry_count(), Some(1), "{source}");
        assert!(weak.is_live());
        drop(holder);
        rt.collect().unwrap();
        assert_eq!(map.weak_map_live_entry_count(), Some(0), "{source}");
        assert!(!weak.is_live());
    }
    for value in [
        "s",
        "Object(s)",
        "{symbol: s}",
        "(function() { let o = {}; o[s] = s; return o; })()",
    ] {
        let rt = Runtime::new();
        let source = format!(
            "function make() {{ const s = Symbol('cycle'); const m = new WeakMap(); m.set(s, {value}); return m; }}"
        );
        let task = rt.spawn(&compile_function(&source).unwrap(), &[]).unwrap();
        let map = complete(task.run().unwrap());
        drop(task);
        rt.collect().unwrap();
        assert_eq!(map.weak_map_live_entry_count(), Some(0), "{source}");
    }
}

#[test]
fn weak_map_ephemeron_values_activate_symbol_keys() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let code = compile_function("function make() { let a = new WeakMap(); let b = new WeakMap(); let k = {}; let s = Symbol(); let v = {}; a.set(k, s); b.set(s, v); return __rt.vector(a, b, k, v); }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let result = complete(task.run().unwrap());
    let (maps, key, weak) = rt.scope(|s| {
        let r = s.local(&result).unwrap();
        (
            [
                s.persist(s.nth(r, 0).unwrap()),
                s.persist(s.nth(r, 1).unwrap()),
            ],
            s.persist(s.nth(r, 2).unwrap()),
            s.downgrade(s.nth(r, 3).unwrap()),
        )
    });
    drop((task, result));
    rt.collect().unwrap();
    for map in &maps {
        assert_eq!(map.weak_map_live_entry_count(), Some(1));
    }
    assert!(weak.is_live());
    drop(key);
    rt.collect().unwrap();
    for map in &maps {
        assert_eq!(map.weak_map_live_entry_count(), Some(0));
    }
    assert!(!weak.is_live());
}

#[test]
fn weak_map_properties_and_prototype_are_traced() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let prototype = new_rooted_object(&rt);
    let property = new_rooted_object(&rt);
    let weak = [prototype.downgrade(), property.downgrade()];
    let code = compile_function("function make(p, v) { let m = new WeakMap(); m.own = v; Object.setPrototypeOf(m, p); return m; }").unwrap();
    let task = rt.spawn(&code, &[&prototype, &property]).unwrap();
    let map = complete(task.run().unwrap());
    drop((task, prototype, property));
    rt.collect().unwrap();
    for w in &weak {
        assert!(w.is_live());
    }
    drop(map);
    rt.collect().unwrap();
    for w in &weak {
        assert!(!w.is_live());
    }
}

/// The descriptor's converted symbol key is the only surviving key path
/// while its value getter waits in a resumable native frame.
#[test]
fn weak_map_symbol_key_in_native_continuation_is_traced() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let value = new_rooted_object(&rt);
    let weak = value.downgrade();
    let code = compile_function(
        r#"function make(v) {
        let key = Symbol(); const m = new WeakMap(); m.set(key, v); v = null;
        const out = {};
        Object.defineProperty(out, key, {get value() { key = null; __rt.need(93, 1); return 1; }});
        return __rt.vector(m, out);
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&value]).unwrap();
    drop(value);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    assert!(weak.is_live());
    let requests = task.take_producer_requests();
    let r = &requests[0];
    let ready = rt.root(Primitive::Undefined);
    let wakeups = rt
        .complete(
            r.key(),
            r.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap();
    assert_eq!(wakeups.task_ids(), vec![task.id()]);
    let result = complete(task.run().unwrap());
    let map = rt.scope(|s| {
        let r = s.local(&result).unwrap();
        s.persist(s.nth(r, 0).unwrap())
    });
    drop((task, result, wakeups));
    rt.collect().unwrap();
    assert_eq!(map.weak_map_live_entry_count(), Some(0));
    assert!(!weak.is_live());
}

#[test]
fn weak_map_unreachable_owner_does_not_retain_values() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let key = new_rooted_object(&rt);
    let value = new_rooted_object(&rt);
    let weak = value.downgrade();
    let code =
        compile_function("function make(k, v) { let m = new WeakMap(); m.set(k, v); return m; }")
            .unwrap();
    let task = rt.spawn(&code, &[&key, &value]).unwrap();
    let map = complete(task.run().unwrap());
    let weak_map = map.downgrade();
    drop((task, map, value));
    rt.collect().unwrap();
    assert!(key.downgrade().is_live());
    assert!(!weak_map.is_live());
    assert!(!weak.is_live());
}

/// Synthetic source fixture, not light-cljs output. Replace the innermost
/// numeric constant through ExecutableSpec to exercise host-loaded symbols.
#[test]
fn weak_map_symbol_keys_in_descendant_code_are_traced() {
    use big_boy_pants_js::frontend::compile_function;
    let rt = Runtime::new();
    let value = new_rooted_object(&rt);
    let weak = value.downgrade();
    let mut owner = compile_function(
        r#"function run(v) {
        function factory() { return function() { return 173; }; }
        const m = new WeakMap(); let key = factory()(); m.set(key, v);
        key = null; v = null;
        __rt.need(94, 1);
        const got = m.get(factory()());
        if (got === undefined) throw 'lost descendant constant';
        return __rt.vector(m, got);
    }"#,
    )
    .unwrap()
    .to_spec();
    let mut factory = owner.functions[0].to_spec();
    let mut child = factory.functions[0].to_spec();
    let index = child
        .constants
        .iter()
        .position(|v| matches!(v, Primitive::Number(173.0)))
        .unwrap();
    child.constants[index] =
        Primitive::Symbol(big_boy_pants_js::JsSymbol::new(Some("constant".into())));
    factory.functions[0] = Executable::load(child).unwrap();
    owner.functions[0] = Executable::load(factory).unwrap();
    let code = Executable::load(owner).unwrap();
    let task = rt.spawn(&code, &[&value]).unwrap();
    drop(value);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    assert!(
        weak.is_live(),
        "descendant code is the only live symbol-key path"
    );
    let requests = task.take_producer_requests();
    let r = &requests[0];
    let ready = rt.root(Primitive::Undefined);
    let wakeups = rt
        .complete(
            r.key(),
            r.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap();
    assert_eq!(wakeups.task_ids(), vec![task.id()]);
    let result = complete(task.run().unwrap());
    let map = rt.scope(|s| {
        let result = s.local(&result).unwrap();
        assert!(s.same_object(s.nth(result, 1).unwrap(), s.upgrade(&weak).unwrap()));
        s.persist(s.nth(result, 0).unwrap())
    });
    drop((task, result, wakeups));
    rt.collect().unwrap();
    assert_eq!(map.weak_map_live_entry_count(), Some(0));
    assert!(
        !weak.is_live(),
        "unrooted Executable handles are not GC roots"
    );
}

// The suspended-activation case adapts QuickJS tests/test_std.js,
// test_async_gc, revision 7d1c13c5d8f8c6b32893786ed35cdcebd88194d2.
// Copyright (c) 2017-2021 Fabrice Bellard
// Copyright (c) 2017-2021 Charlie Gordon
//
// Permission is hereby granted, free of charge, to any person obtaining a copy
// of this software and associated documentation files (the "Software"), to deal
// in the Software without restriction, including without limitation the rights
// to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
// copies of the Software, and to permit persons to whom the Software is
// furnished to do so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in
// all copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL
// THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN
// THE SOFTWARE.

/// Adapted test_async_gc, QuickJS tests/test_std.js at 7d1c13c5d8f8.
/// Synthetic fixture, not light-cljs output. `keep` retains the pending
/// promise until the done callback's collection point so the activation is
/// reachable during collection. Runtime::collect substitutes for std.gc().
#[test]
fn test_async_gc_suspended_activation() {
    use big_boy_pants_js::frontend::compile_function;
    #[derive(Default)]
    struct Marker(bool);
    impl Host for Marker {
        fn effect(&mut self, _call: HostCall) -> Result<Primitive, String> {
            self.0 = true;
            Ok(Primitive::Undefined)
        }
    }
    let rt = Runtime::new();
    // Initialize the realm before recording the baseline.
    let warm = rt
        .spawn(
            &compile_function("function warm() { Promise.resolve(); }").unwrap(),
            &[],
        )
        .unwrap();
    drop(complete(warm.run().unwrap()));
    drop(warm);
    rt.collect().unwrap();
    let baseline = rt.heap_stats().objects;
    let object = rt.scope(|s| s.persist(s.new_object()));
    let weak = object.downgrade();
    let code = compile_function(
        r#"function body(held) {
        let keep;
        (async function run() {
            let obj = held;
            let done = () => { obj; __rt.effect(0,1); keep = null; };
            Promise.resolve().then(done);
            const p = new Promise(() => {});
            keep = p;
            await p;
        })();
        return 42;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&object]).unwrap();
    drop(object);
    let mut host = Marker::default();
    for _ in 0..2000 {
        let out = task.run_with(Budget::instructions(1), &mut host).unwrap();
        assert!(matches!(out, RunOutcome::Yielded), "{out:?}");
        if host.0 {
            break;
        }
    }
    assert!(host.0, "done callback reached collection point");
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.async_state, 1);
    assert!(weak.is_live());
    let retained = rt.heap_stats().objects;
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()).view(),
        ValueView::Number(42.)
    );
    assert_eq!(task.suspended_async_activations(), ["run"]);
    rt.collect().unwrap();
    assert_eq!(rt.heap_usage().objects.async_state, 0);
    assert!(!weak.is_live());
    assert_eq!(rt.heap_usage().objects.promise, 0);
    assert_eq!(rt.heap_stats().objects, baseline);
    assert!(retained > baseline);
}

#[test]
fn unreachable_async_cycle_can_be_collected_before_task_completion() {
    // Synthetic fixture, not light-cljs output, matching test_async_gc's
    // original topology: only done's closure retains obj, not its activation.
    let rt = Runtime::new();
    let object = rt.scope(|s| s.persist(s.new_object()));
    let weak = object.downgrade();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body(obj) {
        (async function run() {
            let done=()=>{ obj; __rt.need(248,7); return obj; };
            Promise.resolve().then(done);
            const p=new Promise(()=>{});
            await p;
        })();
        return 42;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&object]).unwrap();
    drop(object);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    assert!(weak.is_live(), "done still holds its captured object");
    assert_eq!(
        rt.heap_usage().objects.async_state,
        0,
        "unreachable suspended state is collectable"
    );
    let request = task.take_producer_requests().pop().unwrap();
    let ready = rt.root(Primitive::Undefined);
    let resumed = rt
        .complete(
            request.key(),
            request.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap()
        .into_runnable();
    assert_eq!(resumed.len(), 1);
    assert_eq!(complete(task.run().unwrap()).view(), ValueView::Number(42.));
    assert_eq!(task.suspended_async_activations(), ["run"]);
    rt.collect().unwrap();
    assert!(!weak.is_live());
}

#[derive(Default)]
struct FinalizationEffects(Vec<f64>);
impl Host for FinalizationEffects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            panic!("number expected")
        };
        self.0.push(n);
        Ok(Primitive::Undefined)
    }
}

/// Adapted from QuickJS tests/test_builtin.js::test_finalization_registry,
/// revision 7d1c13c5d8f8. The MIT notice above applies to this adaptation.
/// Synthetic fixture, not light-cljs output.
#[test]
fn finalization_callbacks_run_as_jobs_after_collection() {
    let rt = Runtime::new();
    let code = big_boy_pants_js::frontend::compile_script(
        "finalization",
        r#"
        __rt.effect(0, 0);
        {
            let expected = {};
            let actual;
            let count = 0;
            let finrec = new FinalizationRegistry(v => { actual = v; count++; __rt.effect(0, 2); });
            finrec.register({}, expected);
            os.setTimeout(() => {
                if (actual !== expected || count !== 1) throw 'object cleanup';
                __rt.effect(0, 3);
            }, 0);
        }
        {
            let expected = 42;
            let actual;
            let count = 0;
            let finrec = new FinalizationRegistry(v => { actual = v; count++; __rt.effect(0, 2); });
            finrec.register({}, expected);
            os.setTimeout(() => {
                if (actual !== expected || count !== 1) throw 'number cleanup';
                __rt.effect(0, 3);
            }, 0);
        }
        std.gc();
        __rt.effect(0, 1);
        42
    "#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    task.install_sim_host_bindings().unwrap();
    let mut host = FinalizationEffects::default();
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()).view(),
        ValueView::Number(42.0)
    );
    assert_eq!(host.0, [0., 1., 2., 2., 3., 3.]);
    rt.collect().unwrap();
    assert_eq!(host.0.len(), 6);
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn finalization_unregister_prevents_callback() {
    let rt = Runtime::new();
    let code = big_boy_pants_js::frontend::compile_script("unregister", r#"
        const r = new FinalizationRegistry(() => { throw 'unregistered cleanup'; });
        const token = {};
        r.register({}, 1, token); r.register({}, 2, token);
        if (r.unregister(token) !== true || r.unregister(token) !== false) throw 'unregister result';
        std.gc(); os.setTimeout(() => { std.gc(); }, 0); 42
    "#).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    task.install_sim_host_bindings().unwrap();
    assert_eq!(complete(task.run().unwrap()).view(), ValueView::Number(42.));
}

/// Synthetic fixture, not light-cljs output. A parked task owns the only
/// strong held-value edge, then a queued job owns it until that job returns.
#[test]
fn finalization_held_value_is_strong() {
    let rt = Runtime::new();
    let held = new_rooted_object(&rt);
    let weak = held.downgrade();
    let target = new_rooted_object(&rt);
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body(target, held) {
        let r = new FinalizationRegistry(v => { __rt.effect(0, 1); });
        r.register(target, held); target = null; held = null; r = null;
        __rt.need(177, 1); return 42;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&target, &held]).unwrap();
    drop(held);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    rt.collect().unwrap();
    assert!(weak.is_live(), "registry retains held value");
    drop(target);
    rt.collect().unwrap();
    rt.collect().unwrap();
    assert!(weak.is_live(), "queued cleanup retains held value");
    let request = task.take_producer_requests().pop().unwrap();
    let ready = rt.root(Primitive::Undefined);
    let wakes = rt
        .complete(
            request.key(),
            request.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap();
    assert_eq!(wakes.task_ids(), [task.id()]);
    let mut host = FinalizationEffects::default();
    drop(complete(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
    ));
    assert_eq!(host.0, [1.]);
    rt.collect().unwrap();
    assert!(!weak.is_live(), "completed cleanup releases held value");
}

/// Synthetic fixture, not light-cljs output.
#[test]
fn finalization_owner_finished_drops_cleanup() {
    let rt = Runtime::new();
    let target = new_rooted_object(&rt);
    let weak_target = target.downgrade();
    let held = new_rooted_object(&rt);
    let weak_held = held.downgrade();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body(target, held) {
        const r = new FinalizationRegistry(() => { throw 'finished owner cleanup'; });
        r.register(target, held); return r;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&target, &held]).unwrap();
    let registry = complete(task.run().unwrap());
    drop((target, held));
    rt.collect().unwrap();
    rt.collect().unwrap();
    assert!(!weak_target.is_live(), "target was actually collected");
    assert!(!weak_held.is_live(), "closed inbox retains no cleanup job");
    assert!(registry.downgrade().is_live());
    assert!(matches!(
        task.run(),
        Err(big_boy_pants_js::RunError::Finished)
    ));
}

/// Synthetic fixture, not light-cljs output. Empty registries still trace
/// their callback's capture, ordinary properties and custom prototype.
#[test]
fn finalization_registry_properties_and_callback_are_traced() {
    let rt = Runtime::new();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function make() {
        let capture = {}; let property = {}; let prototype = {};
        let r = new FinalizationRegistry(() => capture);
        r.own = property; Object.setPrototypeOf(r, prototype);
        return __rt.vector(r, capture, property, prototype);
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let result = complete(task.run().unwrap());
    let (registry, edges) = rt.scope(|s| {
        let r = s.local(&result).unwrap();
        (
            s.persist(s.nth(r, 0).unwrap()),
            [1, 2, 3].map(|i| s.downgrade(s.nth(r, i).unwrap())),
        )
    });
    drop((task, result));
    rt.collect().unwrap();
    assert!(edges.iter().all(|edge| edge.is_live()));
    drop(registry);
    rt.collect().unwrap();
    assert!(edges.iter().all(|edge| !edge.is_live()));
    assert_eq!(rt.heap_usage().objects.finalization_registry, 0);
}

/// Synthetic fixture, not light-cljs output. The target and unregister
/// token may be the same object/symbol: neither slot is a strong edge.
#[test]
fn finalization_targets_and_tokens_are_weak() {
    for target in ["{}", "Symbol('target')", "Symbol.iterator"] {
        let rt = Runtime::new();
        let source = format!(
            r#"
            let count = 0;
            const r = new FinalizationRegistry(v => {{ if (v !== 177) throw 'held'; count++; }});
            let t = {target}; r.register(t, 177, t); t = null;
            std.gc();
            os.setTimeout(() => {{
                if (count !== {expected}) throw 'weak target';
            }}, 0); 42
        "#,
            expected = if target == "Symbol.iterator" { 0 } else { 1 }
        );
        let task = rt
            .spawn(
                &big_boy_pants_js::frontend::compile_script("weak-finalization", &source).unwrap(),
                &[],
            )
            .unwrap();
        task.install_sim_host_bindings().unwrap();
        assert_eq!(complete(task.run().unwrap()).view(), ValueView::Number(42.));
    }
}

/// Synthetic fixture, not light-cljs output. Collection on a foreign
/// thread queues work; the owner runs it, parks in it, then migrates.
#[test]
fn finalization_cleanup_jobs_can_wait_and_migrate() {
    let rt = Runtime::new();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body() {
        let r = new FinalizationRegistry(v => { __rt.need(177, 2); __rt.effect(0, v); });
        r.register({}, 7); r = null; __rt.need(177, 1); return 42;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    thread::scope(|s| s.spawn(|| rt.collect().unwrap()).join().unwrap());
    let mut host = FinalizationEffects::default();
    let resume = || {
        let request = task.take_producer_requests().pop().unwrap();
        let ready = rt.root(Primitive::Undefined);
        rt.complete(
            request.key(),
            request.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap()
    };
    let wakes = resume();
    assert_eq!(wakes.task_ids(), [task.id()]);
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert!(host.0.is_empty());
    rt.collect().unwrap();
    let wakes = resume();
    assert_eq!(wakes.task_ids(), [task.id()]);
    let outcome = thread::scope(|s| {
        s.spawn(|| task.run_with(Budget::UNLIMITED, &mut host).unwrap())
            .join()
            .unwrap()
    });
    assert_eq!(complete(outcome).view(), ValueView::Number(42.));
    assert_eq!(host.0, [7.]);
}

/// Synthetic fixture, not light-cljs output. Cancellation closes admission
/// and releases even otherwise-unreachable registry/held-value cycles.
#[test]
fn finalization_cancelled_owner_releases_registry_roots() {
    let rt = Runtime::new();
    let held = new_rooted_object(&rt);
    let weak = held.downgrade();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body(held) {
        let r = new FinalizationRegistry(() => { throw 'cancelled cleanup'; });
        r.register({}, held); held = null; r = null;
        __rt.need(177, 3);
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&held]).unwrap();
    drop(held);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    assert!(matches!(
        task.cancel(),
        big_boy_pants_js::Cancel::Runnable(_)
    ));
    assert!(matches!(task.run().unwrap(), RunOutcome::Cancelled));
    rt.collect().unwrap();
    assert!(!weak.is_live());
    assert_eq!(rt.heap_usage().objects.finalization_registry, 0);
}

/// Synthetic fixture, not light-cljs output. Notification consumption must
/// outlive the task handle when a host still roots the registry.
#[test]
fn finalization_dropped_owner_releases_held_values() {
    let rt = Runtime::new();
    let target = new_rooted_object(&rt);
    let held = new_rooted_object(&rt);
    let weak = held.downgrade();
    let weak_target = target.downgrade();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body(target, held) {
        const r = new FinalizationRegistry(() => { throw 'dropped owner'; });
        r.register(target, held); return r;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&target, &held]).unwrap();
    let registry = complete(task.run().unwrap());
    drop((task, target, held));
    rt.collect().unwrap();
    rt.collect().unwrap();
    assert!(registry.downgrade().is_live());
    assert!(!weak_target.is_live());
    assert!(
        !weak.is_live(),
        "cleared cell releases its held value without an owner task"
    );
}

/// Synthetic fixture, not light-cljs output. A stale report must neither
/// consume a replacement cell nor remove its owner-task retention root.
#[test]
fn finalization_delayed_notification_preserves_replacement_cell() {
    use big_boy_pants_js::{CollectionObserver, CollectionReport};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};
    struct Gate {
        first: AtomicBool,
        entered: mpsc::Sender<()>,
        release: Mutex<mpsc::Receiver<()>>,
    }
    impl CollectionObserver for Gate {
        fn collected(&self, _: &CollectionReport) {
            if self.first.swap(false, Ordering::SeqCst) {
                self.entered.send(()).unwrap();
                self.release
                    .lock()
                    .unwrap()
                    .recv_timeout(HANDSHAKE)
                    .expect("release old report");
            }
        }
    }
    let rt = Runtime::new();
    let old = new_rooted_object(&rt);
    let old_weak = old.downgrade();
    let token = new_rooted_object(&rt);
    let target = new_rooted_object(&rt);
    let held = new_rooted_object(&rt);
    let weak_held = held.downgrade();
    let code = big_boy_pants_js::frontend::compile_function(
        r#"function body(old, token, target, held) {
        let r = new FinalizationRegistry(v => { __rt.effect(0, v === 0 ? 0 : 1); });
        r.register(old, 0, token); old = null;
        __rt.need(177, 11);
        if (!r.unregister(token)) throw 'missing old cell';
        r.register(target, held);
        r = null; token = null; target = null; held = null;
        __rt.need(177, 12); return 42;
    }"#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[&old, &token, &target, &held]).unwrap();
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(_)));
    let (entered, observed) = mpsc::channel();
    let (release, released) = mpsc::channel();
    let gate = Arc::new(Gate {
        first: AtomicBool::new(true),
        entered,
        release: Mutex::new(released),
    });
    rt.observe_collections(Arc::downgrade(&gate) as _);
    drop(old);
    let resume = || {
        let request = task.take_producer_requests().pop().unwrap();
        let ready = rt.root(Primitive::Undefined);
        rt.complete(
            request.key(),
            request.producer(),
            big_boy_pants_js::Completion::Ready(&ready),
        )
        .unwrap()
    };
    let mut host = FinalizationEffects::default();
    thread::scope(|scope| {
        let (done, completed) = mpsc::channel();
        let runtime = &rt;
        let collector = scope.spawn(move || {
            let result = runtime.collect();
            done.send(result).unwrap();
        });
        observed
            .recv_timeout(HANDSHAKE)
            .expect("first report paused after clearing");
        assert!(!old_weak.is_live());
        let wakes = resume();
        assert_eq!(wakes.task_ids(), [task.id()]);
        assert!(matches!(
            task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
            RunOutcome::Waiting(_)
        ));
        drop(held);
        rt.collect().unwrap(); // A newer report completes before the old one.
        assert!(weak_held.is_live());
        assert!(host.0.is_empty());
        release.send(()).unwrap();
        completed
            .recv_timeout(HANDSHAKE)
            .expect("old delivery finished")
            .unwrap();
        collector.join().unwrap();
    });
    rt.collect().unwrap();
    assert!(
        weak_held.is_live(),
        "replacement retention survives stale delivery"
    );
    drop(target);
    rt.collect().unwrap();
    let wakes = resume();
    assert_eq!(wakes.task_ids(), [task.id()]);
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()).view(),
        ValueView::Number(42.)
    );
    assert_eq!(
        host.0,
        [1.],
        "only the replacement cell is cleaned, exactly once"
    );
    rt.collect().unwrap();
    assert!(!weak_held.is_live());
}

/// Synthetic fixture, not light-cljs output. Indexed cleanup removes each
/// surviving cell once, interleaved with bulk token unregistration.
#[test]
fn finalization_bulk_cleanup_preserves_once_per_cell() {
    let rt = Runtime::new();
    let code = big_boy_pants_js::frontend::compile_script(
        "bulk-finalization",
        r#"
        let count = 0; let sum = 0; const seen = [];
        let r = new FinalizationRegistry(v => {
            if (seen[v]) throw 'duplicate cleanup';
            seen[v] = true; count++; sum += v;
        });
        const token = {};
        for (let i = 0; i < 4096; i++) r.register({}, i, i % 2 === 0 ? token : undefined);
        if (!r.unregister(token) || r.unregister(token)) throw 'bulk unregister';
        r = null;
        std.gc();
        os.setTimeout(() => { if (count !== 2048 || sum !== 4194304) throw 'bulk cleanup'; }, 0);
        42
    "#,
    )
    .unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    task.install_sim_host_bindings().unwrap();
    assert_eq!(complete(task.run().unwrap()).view(), ValueView::Number(42.));
}

/// Synthetic fixture, not light-cljs output. A panicking external observer
/// must not strand a cell after its target slot was permanently cleared.
#[test]
fn finalization_observer_unwind_consumes_cleared_cells() {
    use big_boy_pants_js::{CollectionObserver, CollectionReport};
    use std::sync::Arc;
    struct Panic;
    impl CollectionObserver for Panic {
        fn collected(&self, _: &CollectionReport) {
            panic!("observer unwind");
        }
    }
    let rt = Runtime::new();
    let target = new_rooted_object(&rt);
    let held = new_rooted_object(&rt);
    let weak = held.downgrade();
    let code = big_boy_pants_js::frontend::compile_function("function body(target, held) { const r = new FinalizationRegistry(() => 0); r.register(target, held); return r; }").unwrap();
    let task = rt.spawn(&code, &[&target, &held]).unwrap();
    let registry = complete(task.run().unwrap());
    drop((task, target, held));
    let observer = Arc::new(Panic);
    rt.observe_collections(Arc::downgrade(&observer) as _);
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| rt.collect())).is_err());
    drop(observer);
    rt.collect().unwrap();
    assert!(registry.downgrade().is_live());
    assert!(!weak.is_live());
}

/// Synthetic fixtures, not light-cljs output. Membership is the only
/// strong path to a Set element; deletion and clear remove that path.
#[test]
fn set_entries_are_traced_and_removed_entries_reclaimed() {
    use big_boy_pants_js::frontend::compile_function;
    for operation in ["", "s.delete(k)", "s.clear()"] {
        let rt = Runtime::new();
        let key = new_rooted_object(&rt);
        let weak = key.downgrade();
        let code = compile_function(&format!(
            "function make(k) {{let s = new Set([k]); {operation}; return s;}}"
        ))
        .unwrap();
        let task = rt.spawn(&code, &[&key]).unwrap();
        let set = complete(task.run().unwrap());
        drop((task, key));
        rt.collect().unwrap();
        assert_eq!(weak.is_live(), operation.is_empty());
        assert!(set.downgrade().is_live());
        drop(set);
        rt.collect().unwrap();
        assert_eq!(weak.upgrade().unwrap_err(), HandleError::Reclaimed);
    }
}

/// Synthetic fixture, not light-cljs output. Only the iterator's internal
/// source slot can retain the Set and its member after the task is dropped.
#[test]
fn set_cursor_retains_source_and_elements() {
    use big_boy_pants_js::frontend::compile_function;
    for method in ["values", "entries"] {
        let rt = Runtime::new();
        let key = new_rooted_object(&rt);
        let weak = key.downgrade();
        let code = compile_function(&format!(
            "function make(k) {{return new Set([k]).{method}();}}"
        ))
        .unwrap();
        let task = rt.spawn(&code, &[&key]).unwrap();
        let cursor = complete(task.run().unwrap());
        drop((task, key));
        rt.collect().unwrap();
        assert!(weak.is_live());
        assert_eq!(rt.heap_usage().objects.js_set, 1);
        drop(cursor);
        rt.collect().unwrap();
        assert!(!weak.is_live());
        assert_eq!(rt.heap_usage().objects.js_set, 0);
    }
}
