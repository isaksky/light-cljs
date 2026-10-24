//! The VM `need` instruction (`__rt.need`, pellet clj-quickjs-23) in the
//! handoff windows of `docs/design.md` §8, forced with test hooks
//! ([`HookPoint`]), never sleeps. Consumers run JavaScript source compiled
//! with `frontend::compile_script` (inline snippets).

use std::sync::{Arc, Mutex};

use super::handoff_tests::{DEF, KIND, complete, hooks, key};
use super::*;
use crate::Runtime;
use crate::dep::{CellStatus, HookPoint};
use crate::frontend::compile_script;

/// `local = 2`, then `local * 100 + __rt.need(DEF, KIND)`: the product is a
/// live operand across the need.
const SOURCE: &str = "let local = 2; local * 100 + __rt.need(3, 1);";

fn source_consumer(rt: &Runtime) -> Task {
    assert_eq!((DEF, KIND), (DependencyId::new(3), ResultKind::new(1)));
    rt.spawn(&compile_script("need", SOURCE).unwrap(), &[])
        .unwrap()
}

fn number(outcome: RunOutcome) -> f64 {
    match outcome {
        RunOutcome::Complete(v) => match v.view() {
            crate::ValueView::Number(n) => n,
            other => panic!("expected a number, got {other:?}"),
        },
        other => panic!("expected Complete, got {other:?}"),
    }
}

/// While suspended, the task keeps the pending result destination and the
/// operand and local values; the next run injects the value at the need.
#[test]
fn a_suspended_need_keeps_its_destination_operands_and_locals() {
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    let task = source_consumer(&rt);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt)));
    assert!(task.take_producer_requests().is_empty(), "claimed up front");
    let (pc, operands, locals) = task.saved_state().unwrap();
    assert_eq!(operands, [Value::Number(200.0)]);
    // `local`, beside the script's completion-value slot.
    assert!(locals.contains(&Value::Number(2.0)), "{locals:?}");
    let pending = match &*task.0.body.lock().unwrap() {
        TaskBody::Ready(vm) => vm.pending_need.unwrap(),
        TaskBody::Finished => panic!("suspended, not finished"),
    };
    assert_eq!(pending.key, key(&rt));
    assert_eq!(pending.pc + 1, pc, "the position is already past the need");
    let _ = complete(&rt, &req, 5.0).into_runnable();
    assert_eq!(number(task.run().unwrap()), 205.0);
}

/// The completion lands after the VM need subscribed but before the park's
/// CAS: the park yields instead of parking, the pending need stays, and the
/// next run reads the value without waiting again.
#[test]
fn completion_inside_the_park_window_yields_and_resumes_at_the_need() {
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    let task = source_consumer(&rt);
    let req = Arc::new(Mutex::new(Some(req)));
    {
        let (rt, req) = (rt.clone(), Arc::clone(&req));
        hooks(&task).once(HookPoint::ParkBeforeCas, move || {
            let req = req.lock().unwrap().take().unwrap();
            let wakeups = complete(&rt, &req, 6.0);
            assert!(matches!(wakeups.deliveries(), [(_, Wake::Remembered)]));
        });
    }
    assert!(matches!(task.run().unwrap(), RunOutcome::Yielded));
    assert!(req.lock().unwrap().is_none(), "the hook completed the cell");
    assert_eq!(task.status(), TaskStatus::Ready);
    assert_eq!(number(task.run().unwrap()), 206.0);
    assert_eq!(task.dependency_log().waiting(), None);
}

/// The completion lands between the need's fast-path read and its
/// subscription: the slow path's recheck reads it and the run continues
/// without suspending.
#[test]
fn completion_before_subscription_is_read_in_the_same_run() {
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    let task = source_consumer(&rt);
    let req = Arc::new(Mutex::new(Some(req)));
    {
        let (rt, req) = (rt.clone(), Arc::clone(&req));
        hooks(&task).once(HookPoint::NeedAfterPendingRead, move || {
            let req = req.lock().unwrap().take().unwrap();
            assert!(complete(&rt, &req, 7.0).deliveries().is_empty());
        });
    }
    assert_eq!(number(task.run().unwrap()), 207.0);
    assert!(req.lock().unwrap().is_none(), "the hook completed the cell");
}

/// A cancellation lands after the VM need subscribed (and claimed the
/// producer) but before the park's CAS: the run ends `Cancelled` with its
/// waiter entry removed, and the claimed producer request is still handed
/// to the embedder, so the cell keeps a producer for other consumers.
#[test]
fn cancellation_inside_the_park_window_keeps_the_producer_request() {
    let rt = Runtime::new();
    let task = source_consumer(&rt);
    {
        let task = task.clone();
        hooks(&task.clone()).once(HookPoint::ParkBeforeCas, move || {
            assert!(matches!(task.cancel(), Cancel::Requested));
        });
    }
    assert!(matches!(task.run().unwrap(), RunOutcome::Cancelled));
    assert_eq!(task.status(), TaskStatus::Cancelled);
    let requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1);
    match rt.cell_status(key(&rt)) {
        CellStatus::Pending { producer, waiters } => {
            assert_eq!(producer, Some(requests[0].producer()));
            assert!(waiters.is_empty(), "the cancelled consumer left the cell");
        }
        other => panic!("{other:?}"),
    }
}

/// Counts leaf host effects.
struct Effects(usize);

impl Host for Effects {
    fn effect(&mut self, _: crate::host::HostCall) -> Result<crate::value::Primitive, String> {
        self.0 += 1;
        Ok(crate::value::Primitive::Number(1.0))
    }
}

/// The budget carries across the need: a ready need that uses the last
/// instruction ends the run before the next one (an effect), and resuming a
/// completed suspended need with a zero budget injects the value without
/// executing anything after it.
#[test]
fn a_need_obeys_exact_instruction_budgets() {
    let src = "let local = 2; local * 100 + __rt.need(3, 1) + __rt.effect(1, 0);";
    let exe = compile_script("need", src).unwrap();
    let at = exe
        .0
        .instrs
        .iter()
        .position(|i| *i == crate::vm::instr::Instr::Need)
        .unwrap();

    // Ready: the need is the run's last instruction.
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    let _ = complete(&rt, &req, 5.0);
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Effects(0);
    let budget = Budget::instructions(at as u64 + 1);
    assert!(matches!(
        task.run_with(budget, &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    let (pc, operands, _) = task.saved_state().unwrap();
    assert_eq!(pc, at + 1);
    assert_eq!(operands, [Value::Number(200.0), Value::Number(5.0)]);
    assert_eq!(host.0, 0, "the effect after the need did not run");
    assert_eq!(
        number(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        206.0
    );
    assert_eq!(host.0, 1);

    // Pending, then resumed with a zero budget.
    let rt = Runtime::new();
    let req = rt.claim(key(&rt)).unwrap();
    let task = rt.spawn(&exe, &[]).unwrap();
    let mut host = Effects(0);
    assert!(matches!(
        task.run_with(Budget::UNLIMITED, &mut host).unwrap(),
        RunOutcome::Waiting(_)
    ));
    let _ = complete(&rt, &req, 6.0).into_runnable();
    assert!(matches!(
        task.run_with(Budget::instructions(0), &mut host).unwrap(),
        RunOutcome::Yielded
    ));
    let (pc, operands, _) = task.saved_state().unwrap();
    assert_eq!(pc, at + 1, "nothing executed");
    assert_eq!(operands, [Value::Number(200.0), Value::Number(6.0)]);
    match &*task.0.body.lock().unwrap() {
        TaskBody::Ready(vm) => assert_eq!(vm.pending_need, None, "injected"),
        TaskBody::Finished => panic!("not finished"),
    }
    assert_eq!(host.0, 0);
    assert_eq!(
        number(task.run_with(Budget::UNLIMITED, &mut host).unwrap()),
        207.0
    );
    assert_eq!(host.0, 1);
}

/// A wake without a completion (a spurious resume) runs the task again: the
/// suspended need finds the cell still pending, subscribes the new
/// execution's handle (claiming nothing more) and suspends again with its
/// state unchanged; the completion then resumes it exactly once.
#[test]
fn a_spurious_resume_resubscribes_the_suspended_need() {
    let rt = Runtime::new();
    let task = source_consumer(&rt);
    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt)));
    let requests = task.take_producer_requests();
    assert_eq!(requests.len(), 1, "the first need claims the producer");
    let saved = task.saved_state().unwrap();
    let first = WakeHandle::new(task.0.deps.runtime, task.id(), 1);
    assert!(matches!(task.wake(first), Wake::Runnable(_)));

    assert!(matches!(task.run().unwrap(), RunOutcome::Waiting(k) if k == key(&rt)));
    assert_eq!(task.saved_state().unwrap(), saved);
    assert!(task.take_producer_requests().is_empty(), "no second claim");
    assert!(
        matches!(task.wake(first), Wake::Stale),
        "the old handle is stale"
    );
    match rt.cell_status(key(&rt)) {
        CellStatus::Pending { waiters, .. } => assert_eq!(waiters, [task.id()]),
        other => panic!("{other:?}"),
    }

    let runnable = complete(&rt, &requests[0], 8.0).into_runnable();
    assert_eq!(runnable.len(), 1);
    assert_eq!(number(runnable[0].run().unwrap()), 208.0);
}
