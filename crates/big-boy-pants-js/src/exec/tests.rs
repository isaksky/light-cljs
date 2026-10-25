//! Transient-builder ownership across migration. Crate-internal because the
//! test takes the builder handle out of a suspended task
//! (`Task::saved_state`), which no public API can do yet. Early VM tests:
//! hand-assembled bytecode, not JavaScript source support.

use std::sync::{Arc, Condvar, Mutex};
use std::time::Duration;

use super::*;
use crate::heap::Rooted;
use crate::host::HostCall;
use crate::value::{Primitive, ValueView};
use crate::vm::{Assembler, Opcode};
use crate::{BuilderError, Runtime, VmErrorKind};

const TIMEOUT: Duration = Duration::from_secs(30);

/// A host whose every effect waits (bounded) until the gate opens.
#[derive(Clone, Default)]
struct Gate(Arc<(Mutex<bool>, Condvar)>);

impl Gate {
    fn open(&self) {
        *self.0.0.lock().unwrap() = true;
        self.0.1.notify_all();
    }
}

impl Host for Gate {
    fn effect(&mut self, call: HostCall) -> Result<Primitive, String> {
        let open = self.0.0.lock().unwrap();
        let (open, _) = self.0.1.wait_timeout_while(open, TIMEOUT, |o| !*o).unwrap();
        if *open {
            Ok(call.arg)
        } else {
            Err("gate never opened".to_string())
        }
    }
}

fn dispatched(events: &[Event], task: TaskId) -> bool {
    events
        .iter()
        .any(|e| e.task == Some(task) && matches!(e.kind, EventKind::Dispatched { .. }))
}

/// The owner creates a vector builder and appends on worker 0, yields at
/// its budget, and is placed on worker 1 (held busy by a gated run). While
/// it waits in the queue, tasks given its builder handle are rejected on
/// worker 0. Then it resumes on worker 1, appends again and freezes: the
/// builder followed the task, not the thread.
#[test]
fn a_task_keeps_its_builder_across_workers_and_other_tasks_are_rejected() {
    use Opcode::*;
    let num = Primitive::Number;
    let rt = Runtime::new();
    // let b = vector_builder(); conj!(b, 1); /* budget: yield */ conj!(b, 2); return persistent!(b)
    let mut asm = Assembler::new();
    asm.op(NewVectorBuilder).set_local(0);
    asm.get_local(0).constant(num(1.0)).op(BuilderConj).op(Pop);
    asm.get_local(0).constant(num(2.0)).op(BuilderConj);
    asm.op(Freeze).op(Return);
    let owner = rt.spawn(&asm.finish("owner", 0, 1).unwrap(), &[]).unwrap();
    let mut asm = Assembler::new();
    asm.constant(num(0.0)).host_effect(1).op(Return);
    let gated = rt.spawn(&asm.finish("gated", 0, 0).unwrap(), &[]).unwrap();

    let gate = Gate::default();
    let host_gate = gate.clone();
    let exec = ExecutorConfig::new()
        .budget(Budget::instructions(6))
        .host(move |_| Box::new(host_gate.clone()))
        .placement(|r| match (r.reason, r.last_worker) {
            (EnqueueReason::Yielded, Some(last)) => {
                Placement::Worker(WorkerId::new((last.index() + 1) % r.workers))
            }
            _ => Placement::Any,
        })
        .start(&rt)
        .unwrap();
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    exec.submit_with(gated.clone(), Placement::Worker(w1))
        .unwrap();
    assert!(exec.wait_for(TIMEOUT, |ev| dispatched(ev, gated.id())));
    exec.submit_with(owner.clone(), Placement::Worker(w0))
        .unwrap();
    let yielded_to_w1 = |ev: &[Event]| {
        ev.iter().any(|e| {
            e.task == Some(owner.id())
                && e.kind
                    == EventKind::Enqueued {
                        reason: EnqueueReason::Yielded,
                        placement: Placement::Worker(w1),
                    }
        })
    };
    assert!(
        exec.wait_for(TIMEOUT, yielded_to_w1),
        "{:#?}",
        exec.events()
    );

    // Queued for the busy worker 1: runnable, VM state saved, builder open.
    assert_eq!(owner.status(), TaskStatus::Ready);
    let (_, _, locals) = owner.saved_state().unwrap();
    let handle = Rooted::new(rt.heap(), locals[0].clone());
    for op in [BuilderConj, BuilderAssoc, Freeze] {
        let mut asm = Assembler::new();
        asm.get_local(0);
        match op {
            BuilderConj => asm.constant(num(9.0)),
            BuilderAssoc => asm.constant(num(0.0)).constant(num(9.0)),
            _ => &mut asm,
        };
        asm.op(op).op(Return);
        let intruder = rt
            .spawn(&asm.finish("intruder", 1, 1).unwrap(), &[&handle])
            .unwrap();
        exec.submit_with(intruder.clone(), Placement::Worker(w0))
            .unwrap();
        let err = match exec.take_outcome(intruder.id(), TIMEOUT) {
            Some(RunOutcome::Failed(e)) => e,
            other => panic!("expected Failed, got {other:?}"),
        };
        let expected = BuilderError::NotOwner {
            owner: owner.id(),
            caller: intruder.id(),
        };
        assert_eq!(err.kind(), VmErrorKind::Builder(expected));
    }
    assert_eq!(
        owner.status(),
        TaskStatus::Ready,
        "intruders changed nothing"
    );

    gate.open();
    let value = match exec.take_outcome(owner.id(), TIMEOUT) {
        Some(RunOutcome::Complete(v)) => v,
        other => panic!("expected Complete, got {other:?}"),
    };
    let items = rt.scope(|s| {
        let items = s.items(s.local(&value).unwrap()).unwrap();
        items.into_iter().map(|i| s.view(i)).collect::<Vec<_>>()
    });
    assert_eq!(items, [ValueView::Number(1.0), ValueView::Number(2.0)]);
    let runs: Vec<(Option<WorkerId>, EventKind)> = exec
        .events()
        .into_iter()
        .filter(|e| e.task == Some(owner.id()))
        .filter(|e| matches!(e.kind, EventKind::Dispatched { .. }))
        .map(|e| (e.worker, e.kind))
        .collect();
    assert_eq!(
        runs,
        [
            (Some(w0), EventKind::Dispatched { execution: 1 }),
            (Some(w1), EventKind::Dispatched { execution: 2 }),
        ]
    );
    exec.shutdown(TIMEOUT).unwrap();
}

/// A claim refused while a diagnostic reader holds the task body (the task
/// stays runnable) neither loses the task nor counts as a run: the task is
/// requeued as `Retried`, the placement hook still sees no last worker, and
/// it completes once the body is released.
#[test]
fn a_transiently_refused_claim_is_retried_without_moving_the_last_worker() {
    use Opcode::*;
    let rt = Runtime::new();
    let mut asm = Assembler::new();
    asm.constant(Primitive::Number(4.0)).op(Return);
    let task = rt.spawn(&asm.finish("four", 0, 0).unwrap(), &[]).unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let seen = Arc::clone(&requests);
    let exec = ExecutorConfig::new()
        .placement(move |r| {
            seen.lock().unwrap().push(*r);
            Placement::Worker(WorkerId::new(1))
        })
        .start(&rt)
        .unwrap();
    let held = task.hold_body();
    exec.submit(task.clone()).unwrap();
    let refused = |ev: &[Event]| {
        ev.iter().any(|e| {
            e.task == Some(task.id())
                && e.kind == EventKind::NotDispatched(RunError::AlreadyRunning)
        })
    };
    assert!(exec.wait_for(TIMEOUT, refused), "{:#?}", exec.events());
    drop(held);
    match exec.take_outcome(task.id(), TIMEOUT) {
        Some(RunOutcome::Complete(v)) => assert_eq!(v.view(), ValueView::Number(4.0)),
        other => panic!("expected Complete, got {other:?}"),
    }
    let requests = requests.lock().unwrap();
    assert_eq!(requests[0].reason, EnqueueReason::Submitted);
    let retried: Vec<&DispatchRequest> = requests
        .iter()
        .filter(|r| r.reason == EnqueueReason::Retried)
        .collect();
    assert!(!retried.is_empty());
    assert!(
        retried.iter().all(|r| r.last_worker.is_none()),
        "{retried:?}"
    );
    exec.shutdown(TIMEOUT).unwrap();
}

/// Answers "continue" to every effect until `stop` is set, after opening
/// `entered`.
#[derive(Clone, Default)]
struct Spin {
    entered: Gate,
    stop: Arc<std::sync::atomic::AtomicBool>,
}

impl Spin {
    fn wait_entered(&self) -> bool {
        let open = self.entered.0.0.lock().unwrap();
        let cv = &self.entered.0.1;
        *cv.wait_timeout_while(open, TIMEOUT, |o| !*o).unwrap().0
    }
}

impl Host for Spin {
    fn effect(&mut self, _: HostCall) -> Result<Primitive, String> {
        self.entered.open();
        let stop = self.stop.load(std::sync::atomic::Ordering::Acquire);
        Ok(Primitive::Bool(!stop))
    }
}

/// The rendezvous root snapshot under the executor (`docs/design.md` §11):
/// a task running on worker 0 (its frames reported by its runner, parked at
/// a VM safepoint), a task queued behind it, a task parked on a dependency
/// (held only by the executor) and a completed cell's result are each roots,
/// and nothing else is: no root comes from the executor's own state or a
/// Rust local.
#[test]
fn root_snapshot_has_running_queued_and_parked_tasks_and_cell_results() {
    use crate::heap::gc::RootCategory;
    use crate::value::Value;
    use crate::{Completion, DependencyId, DependencyKey, Need, ResultKind};
    use Opcode::*;
    let rt = Runtime::new();
    let heap = rt.heap();
    let (def, kind) = (DependencyId::new(1), ResultKind::new(1));
    let key = DependencyKey::new(rt.world(), def, kind);
    let producer = rt.claim(key).unwrap();
    let object_id = |r: &Rooted| match r.value_in(heap).unwrap() {
        Value::Object(id) => id,
        other => panic!("expected an object, got {other:?}"),
    };
    let new_object = || {
        rt.scope(|s| {
            let o = s.new_object();
            s.persist(o)
        })
    };
    // `function(kept) { while (spin()); return kept; }`
    let mut asm = Assembler::new();
    let (top, done) = (asm.label(), asm.label());
    asm.bind(top);
    asm.constant(Primitive::Null).host_effect(0);
    asm.jump_if_false(done).jump(top);
    asm.bind(done);
    asm.get_local(0).op(Return);
    let spinner = asm.finish("spinner", 1, 1).unwrap();
    // `function(kept) { return kept; }`
    let mut asm = Assembler::new();
    asm.get_local(0).op(Return);
    let identity = asm.finish("identity", 1, 1).unwrap();

    let (running, queued, parked) = (new_object(), new_object(), new_object());
    let published = rt.scope(|s| {
        let v = s.new_vector(&[]).unwrap();
        s.persist(v)
    });
    let mut expected = vec![
        (RootCategory::TaskFrame, object_id(&running)),
        (RootCategory::TaskFrame, object_id(&queued)),
        (RootCategory::TaskFrame, object_id(&parked)),
        (RootCategory::DependencyOutcome, object_id(&published)),
    ];
    expected.sort_by_key(|&(_, id)| id);
    let spin_task = rt.spawn(&spinner, &[&running]).unwrap();
    let queued_task = rt.spawn(&identity, &[&queued]).unwrap();
    let park_task = rt.spawn(&identity, &[&parked]).unwrap();
    let (spin_id, queued_id, park_id) = (spin_task.id(), queued_task.id(), park_task.id());
    let pkey = DependencyKey::new(rt.world(), DependencyId::new(2), kind);
    let pproducer = rt.claim(pkey).unwrap();
    let wakeups = rt
        .complete(pkey, pproducer.producer(), Completion::Ready(&published))
        .unwrap();
    assert!(wakeups.task_ids().is_empty());
    drop((running, queued, parked, published));

    let spin = Spin::default();
    let host = spin.clone();
    let exec = ExecutorConfig::new()
        .host(move |_| Box::new(host.clone()))
        .driver(move |mut guard, cx| {
            if guard.task_id() == spin_id {
                guard.run_with(Budget::UNLIMITED, &mut *cx.host)
            } else if guard.task_id() == park_id {
                match guard.need(def, kind) {
                    Ok(Need::Waiting { produce: None, .. }) => guard.park(),
                    Ok(Need::Ready(_)) => guard.run_with(cx.budget, &mut *cx.host),
                    other => panic!("unexpected need result {other:?}"),
                }
            } else {
                guard.run_with(cx.budget, &mut *cx.host)
            }
        })
        .start(&rt)
        .unwrap();
    exec.submit(park_task).unwrap();
    let parked_now = |ev: &[Event]| {
        ev.iter().any(|e| {
            e.task == Some(park_id)
                && matches!(e.kind, EventKind::Released(OutcomeKind::Waiting(_)))
        })
    };
    assert!(exec.wait_for(TIMEOUT, parked_now), "{:#?}", exec.events());
    exec.submit_with(spin_task, Placement::Worker(WorkerId::new(0)))
        .unwrap();
    assert!(spin.wait_entered(), "the spinner never ran");
    exec.submit_with(queued_task, Placement::Worker(WorkerId::new(0)))
        .unwrap();

    let mut snapshot = heap.root_snapshot_at(true).unwrap();
    snapshot.sort_by_key(|&(_, id)| id);
    snapshot.dedup();
    assert_eq!(snapshot, expected);

    spin.stop.store(true, std::sync::atomic::Ordering::Release);
    for id in [spin_id, queued_id] {
        assert!(matches!(
            exec.take_outcome(id, TIMEOUT),
            Some(RunOutcome::Complete(_))
        ));
    }
    let value = rt.root(Primitive::Number(1.0));
    let wakeups = rt
        .complete(key, producer.producer(), Completion::Ready(&value))
        .unwrap();
    assert_eq!(exec.wake(wakeups).unwrap(), 1);
    assert!(matches!(
        exec.take_outcome(park_id, TIMEOUT),
        Some(RunOutcome::Complete(_))
    ));
    exec.shutdown(TIMEOUT).unwrap();
}
