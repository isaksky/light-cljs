//! Synthetic JavaScript fixtures, not light-cljs output.
use super::*;
use crate::frontend::compile_script;
use crate::{HostCall, Primitive, Runtime, ValueView};

#[derive(Default)]
struct Effects(Vec<f64>);
impl Host for Effects {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let Primitive::Number(value) = call.arg else {
            panic!("number expected")
        };
        self.0.push(value);
        Ok(Primitive::Undefined)
    }
}

fn task(rt: &Runtime, source: &str) -> Task {
    let task = rt
        .spawn(&compile_script("sim-facilities.js", source).unwrap(), &[])
        .unwrap();
    task.install_sim_host_bindings().unwrap();
    task
}

fn finish(task: &Task, host: &mut dyn Host) -> ValueView {
    for _ in 0..10000 {
        match task.run_with(Budget::instructions(1), host).unwrap() {
            RunOutcome::Complete(value) => return value.view(),
            RunOutcome::Yielded => (),
            other => panic!("unexpected outcome {other:?}"),
        }
    }
    panic!("task did not finish within the instruction bound");
}

#[test]
fn gc_point_collects_before_next_statement() {
    let rt = Runtime::new();
    // Obtain the weak identity through a script result, then give its
    // published object to the collecting script without retaining a host root.
    let allocation = rt
        .spawn(
            &compile_script("allocate.js", "__rt.vector(175)").unwrap(),
            &[],
        )
        .unwrap();
    let RunOutcome::Complete(object) = allocation.run().unwrap() else {
        panic!("allocator must complete");
    };
    rt.publish(&object).unwrap();
    let weak = object.downgrade();
    let code = crate::frontend::compile_function("function collect(target) { __rt.effect(0, 0); target = null; std.gc(); __rt.effect(0, 1); return 42; }").unwrap();
    let task = rt.spawn(&code, &[&object]).unwrap();
    task.install_sim_host_bindings().unwrap();
    drop(object);
    let mut effects = Effects::default();
    assert!(weak.is_live());
    struct Check<'a>(&'a crate::WeakHandle, &'a mut Effects);
    impl Host for Check<'_> {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            if call.arg == Primitive::Number(0.0) {
                assert!(
                    self.0.is_live(),
                    "the collecting task still holds its argument"
                );
            } else {
                assert!(
                    !self.0.is_live(),
                    "collection must finish before the next statement"
                );
            }
            self.1.effect(call)
        }
    }
    assert_eq!(
        finish(&task, &mut Check(&weak, &mut effects)),
        ValueView::Number(42.0)
    );
    assert_eq!(effects.0, [0.0, 1.0]);
}

#[test]
fn timers_run_after_jobs_in_order() {
    let rt = Runtime::new();
    let task = task(
        &rt,
        "function j() { __rt.effect(0, 1); } function later() { __rt.effect(0, 3); } os.setTimeout(function() { __rt.effect(0, 2); }, 0); os.setTimeout(function() { __rt.effect(0, 4); }, 0); __rt.effect(0, 0); 42;",
    );
    let mut effects = Effects::default();
    for _ in 0..1000 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut effects)
                .unwrap(),
            RunOutcome::Yielded
        ));
        if effects.0 == [0.0] {
            break;
        }
    }
    assert_eq!(effects.0, [0.0]);
    let (j, later) = {
        let mut guard = task.try_begin_run().unwrap();
        let global = guard.vm().unwrap().global_object.unwrap();
        let sender = |name: &str| {
            let key = task
                .0
                .heap
                .keys()
                .lookup(&crate::JsString::from(name))
                .unwrap();
            let value = task
                .0
                .heap
                .read(global, |object| match object {
                    crate::heap::HeapObject::Plain(object) => {
                        object.get_own(key).unwrap().value.clone()
                    }
                    _ => unreachable!(),
                })
                .unwrap();
            (
                guard.job_sender(value.clone()),
                Rooted::new(&task.0.heap, value),
            )
        };
        (sender("j"), sender("later"))
    };
    j.0.enqueue(&[]).unwrap();
    struct Enqueue<'a>(&'a crate::job::JobSender, &'a mut Effects);
    impl Host for Enqueue<'_> {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            if call.arg == Primitive::Number(2.0) {
                self.0.enqueue(&[]).unwrap();
            }
            self.1.effect(call)
        }
    }
    assert_eq!(
        finish(&task, &mut Enqueue(&later.0, &mut effects)),
        ValueView::Number(42.0)
    );
    assert_eq!(effects.0, [0.0, 1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn timers_use_virtual_time_clear_and_resumable_conversion() {
    let rt = Runtime::new();
    let task = task(
        &rt,
        "let delay = { valueOf() { __rt.effect(0, 8); std.gc(); return 2; } }; let cancelled = os.setTimeout(function() { __rt.effect(0, 99); }, 1); os.clearTimeout(cancelled); os.setTimeout(function() { __rt.effect(0, 2); os.setTimeout(function() { __rt.effect(0, 4); }, 2); }, delay); os.setTimeout(function() { __rt.effect(0, 1); }, -5); os.setTimeout(function() { __rt.effect(0, 3); }, 3); __rt.effect(0, 0); 42;",
    );
    let mut effects = Effects::default();
    assert_eq!(finish(&task, &mut effects), ValueView::Number(42.0));
    assert_eq!(effects.0, [8.0, 0.0, 1.0, 2.0, 3.0, 4.0]);
}

#[test]
fn sim_bindings_are_opt_in_and_validate_operands() {
    let rt = Runtime::new();
    let plain = rt
        .spawn(
            &compile_script(
                "absent.js",
                "typeof std === 'undefined' && typeof os === 'undefined'",
            )
            .unwrap(),
            &[],
        )
        .unwrap();
    assert_eq!(
        finish(&plain, &mut Effects::default()),
        ValueView::Bool(true)
    );
    let task = task(
        &rt,
        "let n = 0; try { os.setTimeout(42, 0); } catch(e) { if(e instanceof TypeError) n++; } try { os.setTimeout(function(){}, Infinity); } catch(e) { if(e instanceof RangeError) n++; } try { os.setTimeout(function(){}, Symbol()); } catch(e) { if(e instanceof TypeError) n++; } n;",
    );
    task.install_sim_host_bindings().unwrap();
    assert_eq!(
        finish(&task, &mut Effects::default()),
        ValueView::Number(3.0)
    );
}

#[test]
fn timer_callbacks_are_roots_and_can_wait_and_migrate() {
    let rt = Runtime::new();
    let task = task(
        &rt,
        "let onlyTimer = { answer: 42 }; os.setTimeout((function(capture) { return function() { std.gc(); __rt.effect(0, capture.answer + __rt.need(175, 1)); }; })(onlyTimer), 0); onlyTimer = null; std.gc(); 99;",
    );
    let key = crate::DependencyKey::new(
        rt.world(),
        crate::DependencyId::new(175),
        crate::ResultKind::new(1),
    );
    let producer = rt.claim(key).unwrap();
    assert!(
        matches!(task.run_with(Budget::UNLIMITED, &mut Effects::default()).unwrap(), RunOutcome::Waiting(k) if k == key)
    );
    rt.request_collection().unwrap();
    let value = rt.root(Primitive::Number(1.0));
    let wakes = rt
        .complete(key, producer.producer(), crate::Completion::Ready(&value))
        .unwrap();
    assert_eq!(wakes.into_runnable().len(), 1);
    let (send, receive) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let mut effects = Effects::default();
        let result = finish(&task, &mut effects);
        send.send((result, effects.0)).unwrap();
    });
    let (result, effects) = receive
        .recv_timeout(std::time::Duration::from_secs(10))
        .expect("timer must resume on another worker");
    assert_eq!(result, ValueView::Number(99.0));
    assert_eq!(effects, [43.0]);
}

#[test]
fn collection_point_delivers_weak_notifications_before_resuming() {
    use crate::heap::gc::{CollectionObserver, CollectionReport, ConditionalVisitor, WeakSlot};
    use std::sync::atomic::{AtomicUsize, Ordering};
    #[derive(Debug)]
    struct Notifying(WeakSlot);
    impl Trace for Notifying {
        fn trace(&self, _: &mut dyn FnMut(crate::heap::gc::TraceEdge)) {}
        fn trace_conditional(&mut self, visitor: &mut dyn ConditionalVisitor) {
            self.0.trace_conditional(visitor);
        }
    }
    impl crate::heap::NativePayload for Notifying {
        fn shareability(&self) -> crate::value::share::Shareability {
            crate::value::share::Shareability::TaskOwned
        }
    }
    struct QueueNotification {
        sender: crate::job::JobSender,
        _callable: Rooted,
        count: AtomicUsize,
    }
    impl CollectionObserver for QueueNotification {
        fn collected(&self, report: &CollectionReport) {
            for notification in &report.cleared_weak_slots {
                assert_eq!(notification.token(), 175);
                self.sender.enqueue(&[]).unwrap();
                self.count.fetch_add(1, Ordering::SeqCst);
            }
        }
    }
    let rt = Runtime::new();
    let task = task(
        &rt,
        "function cleanup() { __rt.effect(0, 2); } var target = {}; __rt.effect(0, 0); target = null; std.gc(); __rt.effect(0, 1); 42;",
    );
    let mut effects = Effects::default();
    for _ in 0..1000 {
        assert!(matches!(
            task.run_with(Budget::instructions(1), &mut effects)
                .unwrap(),
            RunOutcome::Yielded
        ));
        if effects.0 == [0.0] {
            break;
        }
    }
    assert_eq!(effects.0, [0.0]);
    let (_owner, observer) = {
        let mut guard = task.try_begin_run().unwrap();
        let global = guard.vm().unwrap().global_object.unwrap();
        let get = |name: &str| {
            let key = task
                .0
                .heap
                .keys()
                .lookup(&crate::JsString::from(name))
                .unwrap();
            task.0
                .heap
                .read(global, |object| match object {
                    crate::heap::HeapObject::Plain(object) => {
                        object.get_own(key).unwrap().value.clone()
                    }
                    _ => unreachable!(),
                })
                .unwrap()
        };
        let Value::Object(target) = get("target") else {
            panic!("object expected")
        };
        let callback = get("cleanup");
        let owner = task
            .0
            .heap
            .alloc(crate::heap::HeapObject::Native(Box::new(Notifying(
                WeakSlot::notifying(target, 175),
            ))))
            .unwrap();
        let owner = Rooted::new(&task.0.heap, Value::Object(owner));
        let observer = Arc::new(QueueNotification {
            sender: guard.job_sender(callback.clone()),
            _callable: Rooted::new(&task.0.heap, callback),
            count: AtomicUsize::new(0),
        });
        (owner, observer)
    };
    rt.observe_collections(Arc::downgrade(&observer) as _);
    struct Check<'a>(&'a QueueNotification, &'a mut Effects);
    impl Host for Check<'_> {
        fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
            assert_eq!(
                self.0.count.load(Ordering::SeqCst),
                1,
                "notification must already have queued its job"
            );
            self.1.effect(call)
        }
    }
    assert_eq!(
        finish(&task, &mut Check(&observer, &mut effects)),
        ValueView::Number(42.0)
    );
    assert_eq!(effects.0, [0.0, 1.0, 2.0]);
}

#[test]
fn collection_point_rejects_an_enclosing_heap_scope() {
    let rt = Runtime::new();
    let task = task(&rt, "std.gc(); 42;");
    rt.scope(|_| {
        let RunOutcome::Failed(error) = task.run().unwrap() else {
            panic!("enclosing access must be refused");
        };
        assert!(error.message.contains("no enclosing heap access"));
    });
}

#[test]
fn failed_and_cancelled_tasks_discard_timers() {
    let rt = Runtime::new();
    let failed = task(
        &rt,
        "os.setTimeout(function() { __rt.effect(0, 99); }, 0); throw 1;",
    );
    let mut effects = Effects::default();
    assert!(matches!(
        failed.run_with(Budget::UNLIMITED, &mut effects).unwrap(),
        RunOutcome::Failed(_)
    ));
    assert!(effects.0.is_empty());
    let failed_job = task(
        &rt,
        "os.setTimeout(function() { throw 1; }, 0); os.setTimeout(function() { __rt.effect(0, 99); }, 0); 42;",
    );
    assert!(matches!(
        failed_job
            .run_with(Budget::UNLIMITED, &mut effects)
            .unwrap(),
        RunOutcome::Failed(_)
    ));
    assert!(effects.0.is_empty());
    let cancelled = task(
        &rt,
        "os.setTimeout(function() { __rt.effect(0, 99); }, 0); __rt.need(176, 1); 42;",
    );
    assert!(matches!(
        cancelled.run_with(Budget::UNLIMITED, &mut effects).unwrap(),
        RunOutcome::Waiting(_)
    ));
    assert!(matches!(cancelled.cancel(), Cancel::Runnable(_)));
    assert!(matches!(cancelled.run().unwrap(), RunOutcome::Cancelled));
    assert!(effects.0.is_empty());
    assert_eq!(cancelled.status(), TaskStatus::Cancelled);
}

#[test]
fn timers_accept_native_and_bound_callbacks() {
    let rt = Runtime::new();
    let task = task(
        &rt,
        "os.setTimeout(std.gc, 0); os.setTimeout((function(n) { __rt.effect(0, n); }).bind(null, 7), 0); 42;",
    );
    let mut effects = Effects::default();
    assert_eq!(finish(&task, &mut effects), ValueView::Number(42.0));
    assert_eq!(effects.0, [7.0]);
}
