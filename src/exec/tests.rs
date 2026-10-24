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
        .start()
        .unwrap();
    let (w0, w1) = (WorkerId::new(0), WorkerId::new(1));
    exec.submit_with(gated.clone(), Placement::Worker(w1));
    assert!(exec.wait_for(TIMEOUT, |ev| dispatched(ev, gated.id())));
    exec.submit_with(owner.clone(), Placement::Worker(w0));
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
    let handle = Rooted::new(rt.heap(), locals[0]);
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
        exec.submit_with(intruder.clone(), Placement::Worker(w0));
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
