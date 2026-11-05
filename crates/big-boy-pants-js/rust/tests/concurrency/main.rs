//! Deterministic adversarial concurrency and cross-feature regression suite
//! (pellet clj-quickjs-14; `docs/design.md` §15).
//!
//! Every scenario composes the production code (tasks and their state word,
//! dependency cells, the two-worker reference executor, the VM `need`, the
//! collector, atoms, resumable natives) under the bounded harness of
//! [`harness`]: windows are opened with gates inside host effects or inside
//! the driver, placement hooks decide migration, and every wait is bounded,
//! so a lost wakeup or a deadlock fails with a trace instead of hanging.
//! Nothing sleeps; no assertion depends on timing.
//!
//! Guarantees exercised (each scenario also gets the harness's invariant
//! checks: single owner per task, increasing execution numbers, one terminal
//! release, effects only inside their task's runs, nothing left parked):
//!
//! * **No lost wakeups.** Every completion window (before registration,
//!   subscribed but not parked, parked) ends with the task run again or
//!   finished; `matrix`, `stress`.
//! * **No concurrent execution of a task.** Checked by the driver while it
//!   runs (a task inside two drivers is a violation) and from the record.
//! * **One producer per key.** Many consumers and host claims race for one
//!   cell; exactly one producer request exists; `stress`.
//! * **Terminal outcome stability.** Completed cells and finished tasks
//!   reject every later completion, cancellation and wake; `stress`.
//! * **World/result-kind isolation.** Completing one world's or kind's cell
//!   wakes only its waiters, and its producer cannot complete the other;
//!   `stress`.
//! * **Ready-read recording.** A need that finds its result complete records
//!   the edge without waiting; `matrix`, `stress`.
//! * **No effect replay.** Pre-wait effects run once across waits,
//!   migration, collection and spurious resumes; everywhere.
//! * **Cross-feature.** A native fold's callback suspended under `finally`
//!   across collection and migration, an atom replaced while a parked task
//!   holds an earlier read, and recursion (deep frames, mutually recursive
//!   published callables) kept apart from a genuine active wait cycle, and
//!   a producer failure racing cancellations of waiters guarded by
//!   `catch`/`finally`; `cross_feature`.
//!
//! Run: `cargo test --test concurrency` (one test:
//! `cargo test --test concurrency <name> -- --exact`; seeded schedules:
//! `cargo test --test concurrency seeded`). Fixtures are synthetic
//! (`tests/fixtures/js/concurrency_*.js`), not light-cljs output.

mod cross_feature;
mod harness;
mod matrix;
mod stress;

use big_boy_pants_js::Executable;

use harness::compile;

pub fn consumer_exe() -> Executable {
    compile(include_str!("../fixtures/js/concurrency_consumer.js"))
}

pub fn blocker_exe() -> Executable {
    compile(include_str!("../fixtures/js/concurrency_blocker.js"))
}

pub fn done_exe() -> Executable {
    compile(include_str!("../fixtures/js/concurrency_done.js"))
}

/// The harness itself: a hang, a lost wakeup and a gate left closed each
/// fail the scenario (with the trace on stderr) instead of hanging or
/// passing. Short bounds keep these quick; they only bound, never order.
mod self_test {
    use std::sync::mpsc;
    use std::time::Duration;

    use big_boy_pants_js::RunOutcome;
    use big_boy_pants_js::exec::Placement;

    use crate::harness::{Config, scenario, w};
    use crate::{blocker_exe, consumer_exe};

    fn short() -> Config {
        Config {
            step: Duration::from_secs(2),
            gate: Duration::from_millis(100),
            scenario: Duration::from_secs(5),
            ..Config::default()
        }
    }

    #[test]
    #[should_panic(expected = "hung")]
    fn a_hanging_scenario_fails_at_its_bound() {
        let config = Config {
            scenario: Duration::from_millis(200),
            ..short()
        };
        scenario("hang", config, |_| {
            // Blocks forever: nothing ever sends.
            let (_tx, rx) = mpsc::channel::<()>();
            let _ = rx.recv();
        });
    }

    #[test]
    #[should_panic(expected = "lost wakeup")]
    fn a_task_left_parked_is_reported_as_a_lost_wakeup() {
        scenario("lost wakeup", short(), |h| {
            let task = h.spawn(h.rt().world(), &consumer_exe(), &[1.0, 5.0, 1.0]);
            let id = h.submit(&task, Placement::Worker(w(0)));
            h.wait_parked(id);
            // The cell is never published: the task stays parked.
        });
    }

    #[test]
    #[should_panic(expected = "still blocked at gate")]
    fn a_gate_left_closed_when_the_body_returns_fails() {
        scenario("gate left closed at return", short(), |h| {
            h.arm(1);
            let task = h.spawn(h.rt().world(), &blocker_exe(), &[1.0]);
            let id = h.submit(&task, Placement::Any);
            h.wait_gated(id, 1);
            // Returns without opening the gate.
        });
    }

    #[test]
    #[should_panic(expected = "timed out at gate")]
    fn a_gate_left_closed_fails_the_scenario() {
        scenario("closed gate", short(), |h| {
            h.arm(1);
            let task = h.spawn(h.rt().world(), &consumer_exe(), &[1.0, 5.0, 1.0]);
            let id = h.submit(&task, Placement::Any);
            // The bounded gate fails the run (a host error) instead of
            // holding the worker forever.
            assert!(matches!(h.outcome(id), RunOutcome::Failed(_)));
        });
    }
}
