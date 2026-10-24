//! Synthetic source fixtures for task-owned jobs; enqueue stays Rust-internal.
use super::*;
use crate::frontend::compile_script;
use crate::job::JobSender as RawJobSender;
use crate::{Completion, Primitive, Runtime, ValueView};
use std::sync::mpsc;
use std::time::Duration;

// Tests hold delivery capabilities outside a traced reaction record. Root
// that record's one heap edge explicitly, as any external holder must.
#[derive(Clone)]
struct JobSender {
    sender: RawJobSender,
    _callable: Rooted,
}

impl JobSender {
    fn enqueue(&self, args: &[Rooted]) -> Result<(), crate::job::JobError> {
        self.sender.enqueue(args)
    }

    fn callable_for_test(&self) -> Value {
        self.sender.callable_for_test()
    }
}

#[derive(Default)]
struct Effects(Vec<f64>);
impl Host for Effects {
    fn effect(&mut self, call: crate::HostCall) -> Result<Primitive, String> {
        let Primitive::Number(n) = call.arg else {
            panic!("number expected")
        };
        self.0.push(n);
        Ok(Primitive::Undefined)
    }
}

// Stop immediately after the fixture's setup marker. All job functions are
// created by this task, including closures over its mutable binding cells.
fn setup(source: &str) -> (Runtime, Task, Vec<JobSender>, Effects) {
    let rt = Runtime::new();
    let code = compile_script("jobs.js", source).unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let mut host = Effects::default();
    for _ in 0..2000 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut host).unwrap(),
            RunOutcome::Yielded
        ));
        if host.0 == [-1.0] {
            break;
        }
    }
    assert_eq!(host.0, [-1.0]);
    let mut guard = task.try_begin_run().unwrap();
    let global = guard.vm().unwrap().global_object.unwrap();
    let senders = ["a", "b", "c"]
        .into_iter()
        .filter_map(|name| {
            let key = task.0.heap.keys().lookup(&crate::JsString::from(name))?;
            let value = task
                .0
                .heap
                .read(global, |o| match o {
                    crate::heap::HeapObject::Plain(o) => o.get_own(key).map(|p| p.value.clone()),
                    _ => None,
                })
                .flatten()?;
            Some(JobSender {
                _callable: Rooted::new(&task.0.heap, value.clone()),
                sender: guard.job_sender(value),
            })
        })
        .collect();
    drop(guard);
    (rt, task, senders, host)
}

fn complete(out: RunOutcome) -> ValueView {
    let RunOutcome::Complete(v) = out else {
        panic!("expected Complete, got {out:?}")
    };
    v.view()
}

#[test]
fn jobs_run_after_script_body_in_fifo_order() {
    let (rt, task, jobs, mut host) = setup(
        "let x = 0; function a(n) { __rt.effect(0, x + n); } function b(n) { __rt.effect(0, x + n); } function c(n) { __rt.effect(0, x + n); } __rt.effect(0, -1); x = 10; __rt.effect(0, x); 42;",
    );
    struct Enqueue<'a> {
        jobs: &'a [JobSender],
        rt: &'a Runtime,
        host: &'a mut Effects,
    }
    impl Host for Enqueue<'_> {
        fn effect(&mut self, call: crate::HostCall) -> Result<Primitive, String> {
            for (i, job) in self.jobs.iter().enumerate() {
                job.enqueue(&[self.rt.root(Primitive::Number(i as f64))])
                    .unwrap();
            }
            self.host.effect(call)
        }
    }
    // Only the body uses this host; jobs use the ordinary recorder after a
    // single-instruction budget stops immediately after its effect.
    let mut enqueue = Enqueue {
        jobs: &jobs,
        rt: &rt,
        host: &mut host,
    };
    while enqueue.host.0.len() == 1 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut enqueue)
                .unwrap(),
            RunOutcome::Yielded
        ));
    }
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
    assert_eq!(host.0, [-1.0, 10.0, 10.0, 11.0, 12.0]);
}

#[test]
fn jobs_enqueued_by_jobs_run_later() {
    let (rt, task, jobs, mut host) = setup(
        "function a() { __rt.effect(0, 1); } function b() { __rt.effect(0, 2); } function c() { __rt.effect(0, 3); } __rt.effect(0, -1); 7;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    struct Append<'a>(&'a JobSender, &'a mut Effects);
    impl Host for Append<'_> {
        fn effect(&mut self, call: crate::HostCall) -> Result<Primitive, String> {
            if call.arg == Primitive::Number(1.0) {
                self.0.enqueue(&[]).unwrap();
            }
            self.1.effect(call)
        }
    }
    assert_eq!(
        complete(
            task.run_with(Budget::UNLIMITED, &mut Append(&jobs[2], &mut host))
                .unwrap()
        ),
        ValueView::Number(7.0)
    );
    assert_eq!(host.0, [-1.0, 1.0, 2.0, 3.0]);
    drop(rt);
}

#[test]
fn job_waits_and_resumes() {
    let (rt, task, jobs, mut host) = setup(
        "function a() { __rt.effect(0, 1); __rt.effect(0, __rt.need(174, 1)); } function b() { __rt.effect(0, 3); } __rt.effect(0, -1); ({body: 42});",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    let (tx, rx) = mpsc::channel();
    let t = task.clone();
    let worker = std::thread::spawn(move || {
        let out = t.run_with(Budget::UNLIMITED, &mut host).unwrap();
        tx.send((out, host, std::thread::current().id())).unwrap();
    });
    let (out, mut host, first) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    worker.join().unwrap();
    assert!(matches!(out, RunOutcome::Waiting(_)));
    drop(jobs);
    rt.collect().unwrap();
    let req = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::Number(2.0));
    let wakeups = rt
        .complete(req.key(), req.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(wakeups.task_ids(), [task.id()]);
    assert_ne!(first, std::thread::current().id());
    let RunOutcome::Complete(body) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("complete expected")
    };
    assert!(matches!(body.view(), ValueView::Object));
    assert_eq!(host.0, [-1.0, 1.0, 2.0, 3.0]);
}

#[test]
fn job_exception_fails_task() {
    let (_rt, task, jobs, mut host) = setup(
        "function a() { throw 'job failed'; } function b() { __rt.effect(0, 2); } __rt.effect(0, -1); 42;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    let RunOutcome::Failed(error) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("failure expected")
    };
    assert!(error.message.contains("job failed"), "{error:?}");
    assert_eq!(host.0, [-1.0]);
    assert!(jobs[1].enqueue(&[]).is_err());
}

#[test]
fn body_exception_discards_jobs() {
    let (rt, task, jobs, mut host) = setup(
        "function a(x) { __rt.effect(0, 1); } function b() { __rt.effect(0, 2); } __rt.effect(0, -1); throw 'body failed';",
    );
    let guard = task.try_begin_run().unwrap();
    let object = task
        .0
        .heap
        .alloc(crate::heap::HeapObject::Plain(
            crate::object::OrdinaryObject::new(Some(task.id())),
        ))
        .unwrap();
    let value = Rooted::new(&task.0.heap, Value::Object(object));
    let weak = value.downgrade();
    guard
        .enqueue_job(jobs[0].callable_for_test(), vec![Value::Object(object)])
        .unwrap();
    jobs[1].enqueue(&[]).unwrap();
    drop(value);
    drop(guard);
    rt.collect().unwrap();
    assert!(weak.is_live());
    let RunOutcome::Failed(error) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("body failure expected")
    };
    assert!(error.message.contains("body failed"), "{error:?}");
    assert_eq!(host.0, [-1.0]);
    assert_eq!(task.status(), TaskStatus::Finished);
    assert_eq!(jobs[0].enqueue(&[]), Err(crate::job::JobError::Closed));
    rt.collect().unwrap();
    assert!(!weak.is_live());
}

#[test]
fn job_activation_rejects_excess_stack_values() {
    use crate::heap::HeapObject;
    use crate::vm::builtin::{Builtin, BuiltinFunction};
    use crate::vm::function::FunctionObject;

    let body = crate::frontend::compile_function("function body() { return 42; }").unwrap();
    let tail = crate::frontend::compile_function("function tail() { __rt.effect(0, 2); }").unwrap();
    for (source, argc) in [
        (Some("function job(x) { __rt.effect(0, 1); return x; }"), 16),
        (
            Some("function job() { __rt.effect(0, 1); return arguments[0]; }"),
            16,
        ),
        (None, 16),
    ] {
        let code = source.map(|s| crate::frontend::compile_function(s).unwrap());
        let required = code.as_ref().map_or(2 * (argc + 1), |code| {
            usize::from(code.0.locals)
                + code.0.max_stack
                + if code.0.uses_arguments { argc } else { 0 }
        });
        for allowed in [false, true] {
            let rt = Runtime::with_limits(crate::VmLimits {
                max_stack_values: required - usize::from(!allowed),
                ..crate::VmLimits::DEFAULT
            });
            let task = rt.spawn(&body, &[]).unwrap();
            let guard = task.try_begin_run().unwrap();
            let callable = match &code {
                Some(code) => HeapObject::Function(FunctionObject::new(
                    Arc::clone(&code.0),
                    Box::new([]),
                    task.id(),
                )),
                None => HeapObject::Builtin(BuiltinFunction {
                    which: Builtin::MathAbs,
                    properties: crate::object::OrdinaryObject::new(None),
                }),
            };
            let callable = Value::Object(task.0.heap.alloc(callable).unwrap());
            let sender = JobSender {
                sender: guard.job_sender(callable.clone()),
                _callable: Rooted::new(&task.0.heap, callable.clone()),
            };
            guard
                .enqueue_job(callable, vec![Value::Number(7.0); argc])
                .unwrap();
            let tail = task
                .0
                .heap
                .alloc(HeapObject::Function(FunctionObject::new(
                    Arc::clone(&tail.0),
                    Box::new([]),
                    task.id(),
                )))
                .unwrap();
            guard.enqueue_job(Value::Object(tail), vec![]).unwrap();
            drop(guard);
            let mut host = Effects::default();
            let outcome = task.run_with(Budget::UNLIMITED, &mut host).unwrap();
            if allowed {
                assert_eq!(complete(outcome), ValueView::Number(42.0));
                assert_eq!(
                    host.0,
                    if code.is_some() {
                        vec![1.0, 2.0]
                    } else {
                        vec![2.0]
                    }
                );
            } else {
                let RunOutcome::Failed(error) = outcome else {
                    panic!("oversized job must fail: {source:?}")
                };
                assert_eq!(error.kind, crate::VmErrorKind::StackOverflow);
                assert_eq!(
                    error.message,
                    if code.is_some() {
                        "job activation exceeds the stack limit"
                    } else {
                        "job arguments exceed the stack limit"
                    }
                );
                assert!(host.0.is_empty());
            }
            assert_eq!(task.status(), TaskStatus::Finished);
            assert_eq!(sender.enqueue(&[]), Err(crate::job::JobError::Closed));
        }
    }
}

#[test]
fn job_boundary_hook_runs_between_jobs() {
    let (_rt, task, jobs, mut host) = setup(
        "function a() { __rt.effect(0, 1); } function b() { __rt.effect(0, 2); } __rt.effect(0, -1); 42;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    let counter = Arc::clone(&task.0.jobs.boundaries);
    struct Boundaries<'a>(&'a std::sync::atomic::AtomicUsize, &'a mut Effects);
    impl Host for Boundaries<'_> {
        fn effect(&mut self, call: crate::HostCall) -> Result<Primitive, String> {
            let Primitive::Number(n) = call.arg else {
                unreachable!()
            };
            assert_eq!(
                self.0.load(std::sync::atomic::Ordering::Relaxed),
                n as usize
            );
            self.1.effect(call)
        }
    }
    assert_eq!(
        complete(
            task.run_with(Budget::UNLIMITED, &mut Boundaries(&counter, &mut host))
                .unwrap()
        ),
        ValueView::Number(42.0)
    );
    assert_eq!(counter.load(std::sync::atomic::Ordering::Relaxed), 3);
}

#[test]
fn job_budget_yields_without_replaying_body_or_jobs() {
    let (_rt, task, jobs, mut host) = setup(
        "function a() { __rt.effect(0, 1); } function b() { __rt.effect(0, 2); } __rt.effect(0, -1); __rt.effect(0, 0); 42;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    let mut yields = 0;
    for _ in 0..1000 {
        match task.run_with(Budget::instructions(1), &mut host).unwrap() {
            RunOutcome::Yielded => {
                yields += 1;
            }
            out => {
                assert_eq!(complete(out), ValueView::Number(42.0));
                break;
            }
        }
    }
    assert_eq!(task.status(), TaskStatus::Finished);
    assert!(yields > 3);
    assert_eq!(host.0, [-1.0, 0.0, 1.0, 2.0]);
}

#[test]
fn cross_task_delivery_does_not_run_jobs_during_publication_or_wait() {
    let (rt, task, jobs, mut host) =
        setup("function a(n) { __rt.effect(0, n); } __rt.effect(0, -1); __rt.need(174, 1); 42;");
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    struct Publish(JobSender, Rooted);
    impl Host for Publish {
        fn effect(&mut self, _: crate::HostCall) -> Result<Primitive, String> {
            self.0.enqueue(std::slice::from_ref(&self.1)).unwrap();
            Ok(Primitive::Undefined)
        }
    }
    let other = rt
        .spawn(
            &compile_script("publisher.js", "__rt.effect(0, 0);").unwrap(),
            &[],
        )
        .unwrap();
    let sender = jobs[0].clone();
    let value = rt.root(Primitive::Number(5.0));
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        tx.send(
            other
                .run_with(Budget::UNLIMITED, &mut Publish(sender, value))
                .unwrap(),
        )
        .unwrap()
    });
    assert!(matches!(
        rx.recv_timeout(Duration::from_secs(10)).unwrap(),
        RunOutcome::Complete(_)
    ));
    worker.join().unwrap();
    assert_eq!(task.status(), TaskStatus::Waiting);
    assert_eq!(host.0, [-1.0]);
    rt.collect().unwrap();
    let req = task.take_producer_requests().pop().unwrap();
    let done = rt.root(Primitive::Undefined);
    let wakes = rt
        .complete(req.key(), req.producer(), Completion::Ready(&done))
        .unwrap();
    assert_eq!(wakes.task_ids(), [task.id()]);
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
    assert_eq!(host.0, [-1.0, 5.0]);
}

#[test]
fn cross_task_bigint_arguments_survive_collection() {
    let (rt, task, jobs, mut host) = setup(
        "function a(small, large, zero) { if (small === 17n && large === 515377520732011331036461129765621272702107522001n && zero === 0n) { __rt.effect(0, 1); } else { __rt.effect(0, 0); } } __rt.effect(0, -1); 77n;",
    );
    let producer = rt
        .spawn(
            &compile_script(
                "bigint-publisher.js",
                "515377520732011331036461129765621272702107522001n;",
            )
            .unwrap(),
            &[],
        )
        .unwrap();
    let small = rt.root(Primitive::BigInt(crate::JsBigInt::from(17)));
    let zero = rt.root(Primitive::BigInt(crate::JsBigInt::from(0)));
    let sender = jobs[0].clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let RunOutcome::Complete(large) = producer.run().unwrap() else {
            panic!("BigInt producer must complete")
        };
        tx.send(sender.enqueue(&[small, large, zero])).unwrap();
    });
    rx.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
    worker.join().unwrap();
    // The publishing task and its argument roots have been dropped. The
    // receiving queue retains the inline and shared-limb primitives.
    rt.collect().unwrap();
    assert_eq!(host.0, [-1.0]);
    let RunOutcome::Complete(value) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("receiver must complete after its job")
    };
    assert_eq!(value.bigint(), Some(crate::JsBigInt::from(77)));
    assert_eq!(host.0, [-1.0, 1.0]);
}

#[test]
fn queue_roots_arguments_and_cancellation_releases_them() {
    let (rt, task, jobs, mut host) =
        setup("function a(x) { __rt.effect(0, x.n); } __rt.effect(0, -1); a = undefined; 42;");
    let guard = task.try_begin_run().unwrap();
    let object = task
        .0
        .heap
        .alloc(crate::heap::HeapObject::Plain(
            crate::object::OrdinaryObject::new(Some(task.id())),
        ))
        .unwrap();
    let value = Rooted::new(&task.0.heap, Value::Object(object));
    let weak = value.downgrade();
    let foreign = Runtime::new().root(Primitive::Number(1.0));
    assert_eq!(
        jobs[0].enqueue(&[foreign]),
        Err(crate::job::JobError::ForeignRuntime)
    );
    // Cross-task arguments must pass publication even though the callable
    // itself can capture this task's mutable state.
    assert_eq!(
        jobs[0].enqueue(std::slice::from_ref(&value)),
        Err(crate::job::JobError::NotShareable)
    );
    let global = match &*guard.body {
        TaskBody::Ready(vm) => vm.global_object.unwrap(),
        _ => unreachable!(),
    };
    let key = task
        .0
        .heap
        .keys()
        .lookup(&crate::JsString::from("a"))
        .unwrap();
    let callable = task
        .0
        .heap
        .read(global, |o| match o {
            crate::heap::HeapObject::Plain(o) => o.get_own(key).unwrap().value.clone(),
            _ => unreachable!(),
        })
        .unwrap();
    guard
        .enqueue_job(callable, vec![Value::Object(object)])
        .unwrap();
    drop(guard);
    drop(value);
    drop(jobs);
    rt.collect().unwrap();
    assert!(weak.upgrade().is_ok());
    assert!(matches!(task.cancel(), Cancel::Requested));
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Cancelled
    ));
    rt.collect().unwrap();
    assert!(weak.upgrade().is_err());
    assert_eq!(host.0, [-1.0]);
}

#[test]
fn inbox_gc_and_body_result_survive_a_job_wait() {
    let (rt, task, jobs, mut host) = setup(
        "function a() { __rt.need(174, 1); } function b(n) { __rt.effect(0, n); } __rt.effect(0, -1); a = undefined; b = undefined; ({answer: 42});",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[rt.root(Primitive::Number(9.0))]).unwrap();
    drop(jobs);
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    let (body, queued) = {
        let guard = task.try_begin_run();
        assert!(matches!(guard, Err(RunError::Waiting)));
        // Diagnostic reads do not claim execution of the parked task.
        let TaskBody::Ready(vm) = &*task.0.body.lock().unwrap() else {
            unreachable!()
        };
        let value = Rooted::new(&task.0.heap, vm.script_result.clone().unwrap());
        let queued = Rooted::new(&task.0.heap, task.0.jobs.next_for_test()).downgrade();
        (value.downgrade(), queued)
    };
    rt.collect().unwrap();
    assert!(body.upgrade().is_ok());
    assert!(queued.upgrade().is_ok());
    let req = task.take_producer_requests().pop().unwrap();
    let done = rt.root(Primitive::Undefined);
    let wakes = rt
        .complete(req.key(), req.producer(), Completion::Ready(&done))
        .unwrap();
    assert_eq!(wakes.task_ids(), [task.id()]);
    let RunOutcome::Complete(result) = task.run_with(Budget::UNLIMITED, &mut host).unwrap() else {
        panic!("complete")
    };
    assert!(result.downgrade().upgrade().is_ok());
    assert_eq!(host.0, [-1.0, 9.0]);
    drop(result);
    rt.collect().unwrap();
    assert!(body.upgrade().is_err());
    assert!(queued.upgrade().is_err());
}

#[test]
fn jobs_keep_local_closures_and_respect_entry_frame_limits() {
    let rt = Runtime::with_limits(crate::VmLimits {
        max_call_depth: 1,
        ..crate::VmLimits::DEFAULT
    });
    let code = crate::frontend::compile_function("function body() { let x = 0; function f() { x = x + 1; __rt.effect(0, x); } __rt.effect(0, -1); x = 40; return 42; }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let mut host = Effects::default();
    for _ in 0..1000 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut host).unwrap(),
            RunOutcome::Yielded
        ));
        if host.0 == [-1.0] {
            break;
        }
    }
    assert_eq!(host.0, [-1.0]);
    let mut guard = task.try_begin_run().unwrap();
    let f = guard.vm().unwrap().snapshot().2.into_iter().find(|v| matches!(v, Value::Object(id) if task.0.heap.read(*id, |o| matches!(o, crate::heap::HeapObject::Function(_))).unwrap_or(false))).unwrap();
    guard.enqueue_job(f.clone(), vec![]).unwrap();
    guard.enqueue_job(f, vec![]).unwrap();
    drop(guard);
    rt.collect().unwrap();
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
    assert_eq!(host.0, [-1.0, 41.0, 42.0]);
}

#[test]
fn inbox_is_traced_while_task_is_at_a_running_safepoint() {
    let (rt, task, jobs, mut host) =
        setup("function a(x) { return x; } __rt.effect(0, -1); a = undefined; 42;");
    let guard = task.try_begin_run().unwrap();
    // The guard registers this thread as a mutator before raw allocation.
    let object = task
        .0
        .heap
        .alloc(crate::heap::HeapObject::Plain(
            crate::object::OrdinaryObject::new(Some(task.id())),
        ))
        .unwrap();
    let value = Rooted::new(&task.0.heap, Value::Object(object));
    let weak = value.downgrade();
    guard
        .enqueue_job(jobs[0].callable_for_test(), vec![Value::Object(object)])
        .unwrap();
    drop(value);
    drop(jobs);
    let runtime = rt.clone();
    let check = weak.clone();
    let (tx, rx) = mpsc::channel();
    let collector = std::thread::spawn(move || {
        runtime.request_collection().unwrap();
        tx.send(check.is_live()).unwrap();
    });
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    while !rt.collection_requested() {
        assert!(
            std::time::Instant::now() < deadline,
            "collector did not request a safepoint"
        );
        std::thread::yield_now();
    }
    assert_eq!(
        complete(guard.run_with(Budget::UNLIMITED, &mut host)),
        ValueView::Number(42.0)
    );
    assert!(rx.recv_timeout(Duration::from_secs(10)).unwrap());
    collector.join().unwrap();
    rt.collect().unwrap();
    assert!(!weak.is_live());
}

#[test]
fn delivery_after_completion_or_drop_is_rejected() {
    let (_rt, task, jobs, mut host) =
        setup("function a() { __rt.effect(0, 1); } __rt.effect(0, -1); 42;");
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
    assert_eq!(jobs[0].enqueue(&[]), Err(crate::job::JobError::Closed));
    drop(task);
    assert_eq!(jobs[0].enqueue(&[]), Err(crate::job::JobError::Closed));
    assert_eq!(host.0, [-1.0]);
}

#[test]
fn bound_and_builtin_jobs_use_the_ordinary_call_path() {
    let (_rt, task, jobs, mut host) = setup(
        "function a(x, y) { __rt.effect(0, x + y); } a = a.bind(undefined, 4, 5); function b() {} b = Math.abs; __rt.effect(0, -1); 42;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
    assert_eq!(host.0, [-1.0, 9.0]);
}

#[test]
fn cancellation_during_a_job_wait_runs_finally_and_discards_tail() {
    let (_rt, task, jobs, mut host) = setup(
        "function a() { try { __rt.need(174, 1); } finally { __rt.effect(0, 1); } } function b() { __rt.effect(0, 2); } __rt.effect(0, -1); 42;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert!(matches!(task.cancel(), Cancel::Runnable(_)));
    assert!(matches!(
        task.run_with(Budget::instructions(1), &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    assert_eq!(jobs[1].enqueue(&[]), Err(crate::job::JobError::Closed));
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Cancelled
    ));
    assert_eq!(host.0, [-1.0, 1.0]);
}

#[test]
fn reaction_sender_edges_are_traced_without_permanent_roots() {
    #[derive(Debug)]
    struct Reaction(RawJobSender);
    impl Trace for Reaction {
        fn trace(&self, visit: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {
            self.0.trace(visit);
        }
    }
    impl crate::heap::NativePayload for Reaction {
        fn shareability(&self) -> crate::value::share::Shareability {
            crate::value::share::Shareability::TaskOwned
        }
    }
    let rt = Runtime::new();
    let code = crate::frontend::compile_function("function body() { let holder; function f() { return holder; } __rt.effect(0, -1); return 42; }").unwrap();
    let task = rt.spawn(&code, &[]).unwrap();
    let mut host = Effects::default();
    for _ in 0..1000 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut host).unwrap(),
            RunOutcome::Yielded
        ));
        if host.0 == [-1.0] {
            break;
        }
    }
    assert_eq!(host.0, [-1.0]);
    let mut guard = task.try_begin_run().unwrap();
    let f = guard.vm().unwrap().snapshot().2.into_iter().find(|v| matches!(v, Value::Object(id) if task.0.heap.read(*id, |o| matches!(o, crate::heap::HeapObject::Function(_))).unwrap_or(false))).unwrap();
    let Value::Object(function) = f.clone() else {
        unreachable!()
    };
    let holder = task
        .0
        .heap
        .read(function, |o| match o {
            crate::heap::HeapObject::Function(f) => f.captures[0].clone(),
            _ => unreachable!(),
        })
        .unwrap();
    let Value::Object(cell) = holder else {
        unreachable!()
    };
    let reaction = task
        .0
        .heap
        .alloc(crate::heap::HeapObject::Native(Box::new(Reaction(
            guard.job_sender(f.clone()),
        ))))
        .unwrap();
    task.0
        .heap
        .with_object_mut(cell, |o| match o {
            crate::heap::HeapObject::Cell(c) => c.value = Some(Value::Object(reaction)),
            _ => unreachable!(),
        })
        .unwrap();
    let rooted = Rooted::new(&task.0.heap, Value::Object(reaction));
    let weak = Rooted::new(&task.0.heap, f).downgrade();
    drop(guard);
    drop(task);
    rt.collect().unwrap();
    assert!(weak.is_live(), "reaction must trace its callable");
    drop(rooted);
    rt.collect().unwrap();
    assert!(
        !weak.is_live(),
        "unreachable reaction/callable cycles must be reclaimed"
    );
}

#[test]
fn budget_is_not_replenished_at_activation_boundaries() {
    let (_rt, task, jobs, mut host) =
        setup("function a() { return 1; } function b() { return 2; } __rt.effect(0, -1); 42;");
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    // Reach the body's final Return without executing it.
    for _ in 0..1000 {
        let mut guard = task.try_begin_run().unwrap();
        let vm = guard.vm().unwrap();
        if matches!(
            vm.top().code.instrs[vm.top().pc],
            crate::vm::instr::Instr::Return
        ) {
            break;
        }
        assert!(matches!(
            guard.run_with(Budget::instructions(1), &mut host),
            RunOutcome::Yielded
        ));
    }
    // The body's Return consumes the entire budget. The first job is set up
    // but cannot execute even its first instruction during this run.
    assert!(matches!(
        task.run_with(Budget::instructions(1), &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    {
        let mut guard = task.try_begin_run().unwrap();
        let vm = guard.vm().unwrap();
        assert_eq!(vm.top().pc, 0);
        assert_eq!(vm.script_result, Some(Value::Number(42.0)));
    }
    // Reach the first job's final Return; its one-instruction run likewise
    // must leave the second job untouched at pc 0.
    for _ in 0..1000 {
        let mut guard = task.try_begin_run().unwrap();
        let vm = guard.vm().unwrap();
        if matches!(
            vm.top().code.instrs[vm.top().pc],
            crate::vm::instr::Instr::Return
        ) {
            break;
        }
        assert!(matches!(
            guard.run_with(Budget::instructions(1), &mut host),
            RunOutcome::Yielded
        ));
    }
    assert!(matches!(
        task.run_with(Budget::instructions(1), &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    {
        let mut guard = task.try_begin_run().unwrap();
        assert_eq!(guard.vm().unwrap().top().pc, 0);
    }
    assert_eq!(
        task.0
            .jobs
            .boundaries
            .load(std::sync::atomic::Ordering::Relaxed),
        2
    );
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
}

#[test]
fn enqueue_racing_final_drain_is_executed_or_closed() {
    for publish_first in [true, false] {
        let (_rt, task, jobs, mut host) =
            setup("function a() { __rt.effect(0, 1); } __rt.effect(0, -1); 42;");
        let (drain_tx, drain_rx) = mpsc::channel();
        let (published_tx, published_rx) = mpsc::channel();
        let mut published_rx = Some(published_rx);
        let (attempt_tx, attempt_rx) = mpsc::channel();
        let sender = jobs[0].clone();
        if publish_first {
            let published_rx = published_rx.take().unwrap();
            // Hold the runner immediately before it acquires the FIFO lock.
            // Publication acquires that lock and completes before draining.
            *task.0.jobs.before_next.lock().unwrap() = Some(Box::new(move || {
                drain_tx.send(()).unwrap();
                assert_eq!(
                    published_rx.recv_timeout(Duration::from_secs(10)).unwrap(),
                    Ok(())
                );
            }));
        } else {
            // Hold the final empty check while it owns the FIFO lock. The
            // publisher reaches push and must acquire the lock after close.
            *task.0.jobs.before_close.lock().unwrap() = Some(Box::new(move || {
                drain_tx.send(()).unwrap();
                attempt_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            }));
            *task.0.jobs.before_push.lock().unwrap() = Some(Box::new(move || {
                attempt_tx.send(()).unwrap();
            }));
        }
        let worker = std::thread::spawn(move || {
            drain_rx.recv_timeout(Duration::from_secs(10)).unwrap();
            let result = sender.enqueue(&[]);
            published_tx.send(result).unwrap();
        });
        assert_eq!(
            complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
            ValueView::Number(42.0)
        );
        if publish_first {
            assert_eq!(host.0, [-1.0, 1.0]);
        } else {
            assert_eq!(
                published_rx
                    .as_ref()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(10))
                    .unwrap(),
                Err(crate::job::JobError::Closed)
            );
            assert_eq!(host.0, [-1.0]);
        }
        worker.join().unwrap();
    }
}

#[test]
fn builtin_job_callback_waits_and_resumes() {
    let (rt, task, jobs, mut host) = setup(
        "let arg = {valueOf() { __rt.effect(0, 1); return __rt.need(174, 1); }}; function a() {} a = Math.abs.bind(undefined, arg); function b() { __rt.effect(0, 3); } __rt.effect(0, -1); a = undefined; b = undefined; arg = undefined; 42;",
    );
    jobs[0].enqueue(&[]).unwrap();
    jobs[1].enqueue(&[]).unwrap();
    drop(jobs);
    let t = task.clone();
    let (tx, rx) = mpsc::channel();
    let worker = std::thread::spawn(move || {
        let out = t.run_with(Budget::UNLIMITED, &mut host).unwrap();
        tx.send((out, host, std::thread::current().id())).unwrap();
    });
    let (out, mut host, first) = rx.recv_timeout(Duration::from_secs(10)).unwrap();
    worker.join().unwrap();
    assert!(matches!(out, RunOutcome::Waiting(_)));
    assert_eq!(host.0, [-1.0, 1.0]);
    assert_eq!(task.stats().unwrap().native_frames, 1);
    rt.collect().unwrap();
    let req = task.take_producer_requests().pop().unwrap();
    let value = rt.root(Primitive::Number(-2.0));
    let wakes = rt
        .complete(req.key(), req.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(wakes.task_ids(), [task.id()]);
    assert_ne!(first, std::thread::current().id());
    assert_eq!(
        complete(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        ValueView::Number(42.0)
    );
    assert_eq!(host.0, [-1.0, 1.0, 3.0]);
}
