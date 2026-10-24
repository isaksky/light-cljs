//! Synthetic source fixtures for task-owned jobs; enqueue stays Rust-internal.
use super::*;
use crate::frontend::compile_script;
use crate::job::JobSender;
use crate::{Completion, Primitive, Runtime, ValueView};
use std::sync::mpsc;
use std::time::Duration;

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
            Some(guard.job_sender(value))
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
