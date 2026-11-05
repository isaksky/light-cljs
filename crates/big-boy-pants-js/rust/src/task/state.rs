//! The task execution state machine: one atomic word per task.
//!
//! See the [`crate::task`] module docs for the states, transitions and
//! linearization points; this file is their implementation. Every transition
//! is a single successful compare-and-swap on [`ExecState`]'s word (retried
//! on failure), so no lock is involved and nothing runs inline: a transition
//! only changes the word and reports what happened.
//!
//! Word layout (`u64`):
//!
//! | bits  | field |
//! |-------|-------|
//! | 0–1   | phase: `RUNNABLE`, `EXECUTING`, `PARKED`, `TERMINAL` |
//! | 2     | `NOTIFIED`: a wake for the current execution arrived while it was still executing |
//! | 3     | `CANCELLED`: the consumer was cancelled; sticky, dropped only by a run that completes or fails before observing it |
//! | 4–63  | execution number: how many runs have been claimed (0 before the first) |

use std::sync::atomic::{AtomicU64, Ordering};

const PHASE_MASK: u64 = 0b11;
const RUNNABLE: u64 = 0;
const EXECUTING: u64 = 1;
const PARKED: u64 = 2;
const TERMINAL: u64 = 3;
const NOTIFIED: u64 = 0b100;
const CANCELLED: u64 = 0b1000;
const EXECUTION_SHIFT: u32 = 4;

/// A decoded state word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Not executing and not waiting: a scheduler may claim it.
    Runnable,
    /// Owned by exactly one run guard. `notified`: a wake for this execution
    /// arrived before it finished parking.
    Executing { notified: bool },
    /// Suspended on a dependency subscribed during this execution; only a
    /// wake for this execution (or a cancellation) makes it runnable.
    Parked,
    /// Completed, failed or cancelled; never changes again.
    Terminal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Word(u64);

impl Word {
    const fn new(phase: u64, execution: u64) -> Word {
        Word(phase | (execution << EXECUTION_SHIFT))
    }

    fn phase(self) -> Phase {
        match self.0 & PHASE_MASK {
            RUNNABLE => Phase::Runnable,
            EXECUTING => Phase::Executing {
                notified: self.0 & NOTIFIED != 0,
            },
            PARKED => Phase::Parked,
            _ => Phase::Terminal,
        }
    }

    fn cancelled(self) -> bool {
        self.0 & CANCELLED != 0
    }

    /// A new word for `phase` and `execution` that keeps this word's
    /// `CANCELLED` bit (and nothing else).
    fn keeping_cancel(self, phase: u64, execution: u64) -> Word {
        Word(Word::new(phase, execution).0 | (self.0 & CANCELLED))
    }

    fn execution(self) -> u64 {
        self.0 >> EXECUTION_SHIFT
    }
}

/// A snapshot of the word ([`ExecState::snapshot`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Snapshot {
    pub(crate) phase: Phase,
    pub(crate) cancelled: bool,
    pub(crate) execution: u64,
}

/// Why a claim failed: the phase it found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotRunnable {
    Executing,
    Parked,
    Terminal,
}

/// How the owner ends an execution ([`ExecState::release`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum End {
    /// `Executing → Runnable` (yield, or a guard dropped without an
    /// outcome). A requested cancellation stays requested.
    Runnable,
    /// `Executing → Terminal` after a completion or failure: the run reached
    /// its outcome before observing any cancellation, which is dropped.
    Finished,
    /// `Executing → Terminal` after the run observed the cancellation and
    /// performed terminal cleanup. Requires `CANCELLED`.
    Cancelled,
}

/// What a park did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Park {
    /// `Executing → Parked`: a later wake (or cancellation) makes the task
    /// runnable.
    Parked,
    /// A wake arrived first (`NOTIFIED`): `Executing → Runnable`. The parker
    /// hands the task back to its scheduler itself.
    AlreadyWoken,
    /// The task is cancelled: nothing changed, the caller still owns the
    /// execution and must perform terminal cleanup and release it with
    /// [`End::Cancelled`].
    Cancelled,
}

/// What delivering a wake did to the state word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// `Parked → Runnable`: the deliverer enqueues the task.
    Runnable,
    /// `Executing → Executing + NOTIFIED`: the owner's park will see it.
    Remembered,
    /// Nothing changed: the handle's execution is over or already notified,
    /// or the task is cancelled.
    Stale,
}

/// What a cancellation request did to the state word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CancelDelivery {
    /// `Parked → Runnable + CANCELLED`: the canceller enqueues the task, whose
    /// next run observes the cancellation.
    Runnable,
    /// `CANCELLED` set on a runnable or executing task: its current run (at
    /// its next observation point) or its next run observes it.
    Requested,
    /// `CANCELLED` was already set on a task that is not yet terminal:
    /// nothing changed.
    AlreadyRequested,
    /// The task is terminal (cancelled or not): nothing changed.
    Finished,
}

/// The atomic state word of one task.
#[derive(Debug)]
pub(crate) struct ExecState(AtomicU64);

impl ExecState {
    pub(crate) fn new() -> ExecState {
        ExecState(AtomicU64::new(Word::new(RUNNABLE, 0).0))
    }

    /// Current phase, cancellation bit and execution number. A snapshot.
    pub(crate) fn snapshot(&self) -> Snapshot {
        // Acquire: a caller that sees `Runnable` after a wake also sees what
        // the waker published before it (not relied on for correctness).
        let w = Word(self.0.load(Ordering::Acquire));
        Snapshot {
            phase: w.phase(),
            cancelled: w.cancelled(),
            execution: w.execution(),
        }
    }

    /// `Runnable(n) → Executing(n + 1)`: the execution claim. On success the
    /// caller is the only owner until it calls `release` or `park`, and gets
    /// the new execution number. `CANCELLED` is kept: a cancelled task is
    /// still claimed, because its run is where the cancellation is observed.
    ///
    /// Ordering: `Acquire` on success pairs with the `Release` of whichever
    /// transition made the task runnable (the previous owner's release or
    /// park, a waker or a canceller), so the new owner sees everything they
    /// did first.
    pub(crate) fn claim(&self) -> Result<u64, NotRunnable> {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            let w = Word(current);
            match w.phase() {
                Phase::Runnable => {}
                Phase::Executing { .. } => return Err(NotRunnable::Executing),
                Phase::Parked => return Err(NotRunnable::Parked),
                Phase::Terminal => return Err(NotRunnable::Terminal),
            }
            let execution = w.execution() + 1;
            match self.0.compare_exchange_weak(
                current,
                w.keeping_cancel(EXECUTING, execution).0,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(execution),
                Err(actual) => current = actual,
            }
        }
    }

    /// The owner of `execution` ends it (see [`End`]). A pending `NOTIFIED`
    /// is discarded: the task is runnable (or finished) anyway.
    ///
    /// A CAS loop, not a store: while the phase is `Executing` a waker may
    /// set `NOTIFIED` and a canceller may set `CANCELLED`, and the latter
    /// must not be lost. Both only ever add a bit, so the loop terminates.
    /// `Release` publishes the run's effects to the next claim's `Acquire`.
    pub(crate) fn release(&self, execution: u64, end: End) {
        self.release_with(execution, end, || {});
    }

    /// [`ExecState::release`] with `before_cas` run after each read of the
    /// word, as in [`ExecState::park`] (the test hook point).
    fn release_with(&self, execution: u64, end: End, mut before_cas: impl FnMut()) {
        let mut current = self.0.load(Ordering::Relaxed);
        loop {
            let w = Word(current);
            debug_assert!(matches!(w.phase(), Phase::Executing { .. }));
            debug_assert_eq!(w.execution(), execution, "only the owner releases");
            let next = match end {
                End::Runnable => w.keeping_cancel(RUNNABLE, execution),
                End::Finished => Word::new(TERMINAL, execution),
                End::Cancelled => {
                    debug_assert!(w.cancelled(), "a cancelled end without a request");
                    Word(Word::new(TERMINAL, execution).0 | CANCELLED)
                }
            };
            before_cas();
            match self.0.compare_exchange_weak(
                current,
                next.0,
                Ordering::Release,
                Ordering::Relaxed,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }

    /// The owner of `execution` suspends it (see [`Park`]). `before_cas` runs
    /// after each read of the word and before the CAS that depends on it
    /// (a test hook point; a no-op otherwise).
    ///
    /// Linearization point: the read that finds `CANCELLED`, or the
    /// successful CAS to `Parked` or (finding `NOTIFIED`) to `Runnable`.
    /// Cancellation takes precedence over a remembered wake. Ordering:
    /// `Release` on the CAS publishes the saved state to the waker's or
    /// canceller's `Acquire`; `Acquire` on the reads pairs with the waker's
    /// `Release`, so the owner sees the completion that woke it.
    pub(crate) fn park(&self, execution: u64, mut before_cas: impl FnMut()) -> Park {
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let w = Word(current);
            debug_assert_eq!(w.execution(), execution, "only the owner parks");
            if w.cancelled() {
                return Park::Cancelled;
            }
            let (next, park) = match w.phase() {
                Phase::Executing { notified: true } => {
                    (Word::new(RUNNABLE, execution), Park::AlreadyWoken)
                }
                Phase::Executing { notified: false } => {
                    (Word::new(PARKED, execution), Park::Parked)
                }
                other => unreachable!("park by the owner found {other:?}"),
            };
            before_cas();
            match self
                .0
                .compare_exchange(current, next.0, Ordering::Release, Ordering::Acquire)
            {
                Ok(_) => return park,
                // A waker set NOTIFIED or a canceller set CANCELLED: reread.
                Err(actual) => current = actual,
            }
        }
    }

    /// Deliver a wake for `execution` (see [`Delivery`]). `before_cas` as in
    /// [`ExecState::park`].
    ///
    /// Linearization point: the successful CAS (`Parked → Runnable` or
    /// setting `NOTIFIED`), or the read that finds the handle stale. At most
    /// one delivery per execution changes the word: both CASes expect a word
    /// with this execution number and neither `NOTIFIED` nor `CANCELLED`, and
    /// both produce a word no other delivery for the same execution accepts.
    /// A cancelled task is never woken: a wake that loses its CAS to a
    /// cancellation rereads `CANCELLED` and is stale. Ordering: `AcqRel` on
    /// success, so the completion published before the wake is visible to
    /// whoever claims or parks next.
    pub(crate) fn wake(&self, execution: u64, mut before_cas: impl FnMut()) -> Delivery {
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let w = Word(current);
            if w.execution() != execution || w.cancelled() {
                return Delivery::Stale;
            }
            let (next, delivery) = match w.phase() {
                Phase::Parked => (Word::new(RUNNABLE, execution).0, Delivery::Runnable),
                Phase::Executing { notified: false } => (current | NOTIFIED, Delivery::Remembered),
                Phase::Executing { notified: true } | Phase::Runnable | Phase::Terminal => {
                    return Delivery::Stale;
                }
            };
            before_cas();
            match self
                .0
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return delivery,
                Err(actual) => current = actual,
            }
        }
    }

    /// Request cancellation (see [`CancelDelivery`]). `before_cas` as in
    /// [`ExecState::park`].
    ///
    /// Linearization point: the successful CAS that sets `CANCELLED` (and,
    /// from `Parked`, makes the task runnable), or the read that finds it
    /// already set or the task terminal. A cancellation and a wake of the
    /// same parked execution both CAS from the same `Parked` word, so exactly
    /// one of them makes the task runnable; the loser rereads. Ordering:
    /// `AcqRel` on success, so the owner's park or release that follows (or
    /// the next claim) sees the request.
    pub(crate) fn cancel(&self, mut before_cas: impl FnMut()) -> CancelDelivery {
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let w = Word(current);
            if w.phase() == Phase::Terminal {
                return CancelDelivery::Finished;
            }
            if w.cancelled() {
                return CancelDelivery::AlreadyRequested;
            }
            let (next, delivery) = match w.phase() {
                Phase::Terminal => unreachable!("checked above"),
                Phase::Parked => (
                    Word::new(RUNNABLE, w.execution()).0 | CANCELLED,
                    CancelDelivery::Runnable,
                ),
                Phase::Runnable | Phase::Executing { .. } => {
                    (current | CANCELLED, CancelDelivery::Requested)
                }
            };
            before_cas();
            match self
                .0
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
            {
                Ok(_) => return delivery,
                Err(actual) => current = actual,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn phase(s: &ExecState) -> (Phase, u64) {
        let snap = s.snapshot();
        (snap.phase, snap.execution)
    }

    fn cancelled(s: &ExecState) -> bool {
        s.snapshot().cancelled
    }

    #[test]
    fn the_word_encodes_phase_notification_and_execution() {
        let s = ExecState::new();
        assert_eq!(phase(&s), (Phase::Runnable, 0));
        assert_eq!(s.claim(), Ok(1));
        assert_eq!(s.claim(), Err(NotRunnable::Executing));
        assert_eq!(s.wake(1, || {}), Delivery::Remembered);
        assert_eq!(phase(&s), (Phase::Executing { notified: true }, 1));
        assert_eq!(s.wake(1, || {}), Delivery::Stale, "second wake");
        assert_eq!(s.park(1, || {}), Park::AlreadyWoken);
        assert_eq!(phase(&s), (Phase::Runnable, 1));
        assert_eq!(s.wake(1, || {}), Delivery::Stale, "the suspension is over");
        assert_eq!(s.claim(), Ok(2));
        assert_eq!(s.park(2, || {}), Park::Parked);
        assert_eq!(s.claim(), Err(NotRunnable::Parked));
        assert_eq!(s.wake(1, || {}), Delivery::Stale, "older execution");
        assert_eq!(s.wake(3, || {}), Delivery::Stale, "future execution");
        assert_eq!(s.wake(2, || {}), Delivery::Runnable);
        assert_eq!(s.wake(2, || {}), Delivery::Stale, "duplicate");
        assert_eq!(s.claim(), Ok(3));
        s.release(3, End::Finished);
        assert_eq!(phase(&s), (Phase::Terminal, 3));
        assert_eq!(s.claim(), Err(NotRunnable::Terminal));
        assert_eq!(s.wake(3, || {}), Delivery::Stale);
        assert_eq!(s.cancel(|| {}), CancelDelivery::Finished);
        assert!(!cancelled(&s));
    }

    /// The park CAS loses to a wake that sets `NOTIFIED` after park read the
    /// word: park rereads and reports `AlreadyWoken` instead of parking.
    #[test]
    fn a_wake_between_park_read_and_cas_is_not_lost() {
        let s = ExecState::new();
        let n = s.claim().unwrap();
        let mut fired = false;
        let park = s.park(n, || {
            if !fired {
                fired = true;
                assert_eq!(s.wake(n, || {}), Delivery::Remembered);
            }
        });
        assert_eq!(park, Park::AlreadyWoken);
        assert_eq!(phase(&s), (Phase::Runnable, n));
    }

    /// The wake CAS loses to the owner parking after the wake read
    /// `Executing`: the wake rereads `Parked` and makes the task runnable.
    #[test]
    fn a_park_between_wake_read_and_cas_is_woken() {
        let s = ExecState::new();
        let n = s.claim().unwrap();
        let mut fired = false;
        let delivery = s.wake(n, || {
            if !fired {
                fired = true;
                assert_eq!(s.park(n, || {}), Park::Parked);
            }
        });
        assert_eq!(delivery, Delivery::Runnable);
        assert_eq!(phase(&s), (Phase::Runnable, n));
    }

    /// Cancellation in every phase: sticky across claims and releases,
    /// observed by park (ahead of a remembered wake), never woken, terminal
    /// only through the owner's cancelled release.
    #[test]
    fn cancellation_is_sticky_and_blocks_every_later_wake() {
        // Runnable: requested; the next claim keeps it and park observes it.
        let s = ExecState::new();
        assert_eq!(s.cancel(|| {}), CancelDelivery::Requested);
        assert_eq!(s.cancel(|| {}), CancelDelivery::AlreadyRequested);
        let n = s.claim().unwrap();
        assert!(cancelled(&s));
        assert_eq!(s.wake(n, || {}), Delivery::Stale, "never remembered");
        assert_eq!(s.park(n, || {}), Park::Cancelled);
        assert_eq!(phase(&s), (Phase::Executing { notified: false }, n));
        s.release(n, End::Runnable);
        assert!(cancelled(&s), "a plain release keeps the request");
        let n = s.claim().unwrap();
        s.release(n, End::Cancelled);
        assert_eq!(phase(&s), (Phase::Terminal, n));
        assert!(cancelled(&s));
        assert_eq!(s.claim(), Err(NotRunnable::Terminal));
        assert_eq!(s.cancel(|| {}), CancelDelivery::Finished);

        // Executing with a remembered wake: cancellation wins at park.
        let s = ExecState::new();
        let n = s.claim().unwrap();
        assert_eq!(s.wake(n, || {}), Delivery::Remembered);
        assert_eq!(s.cancel(|| {}), CancelDelivery::Requested);
        assert_eq!(s.park(n, || {}), Park::Cancelled);

        // Parked: the cancellation makes it runnable; the wake is stale.
        let s = ExecState::new();
        let n = s.claim().unwrap();
        assert_eq!(s.park(n, || {}), Park::Parked);
        assert_eq!(s.cancel(|| {}), CancelDelivery::Runnable);
        assert_eq!(phase(&s), (Phase::Runnable, n));
        assert_eq!(s.wake(n, || {}), Delivery::Stale);
        assert_eq!(s.cancel(|| {}), CancelDelivery::AlreadyRequested);

        // A run that finishes before observing drops the request.
        let s = ExecState::new();
        let n = s.claim().unwrap();
        assert_eq!(s.cancel(|| {}), CancelDelivery::Requested);
        s.release(n, End::Finished);
        assert!(!cancelled(&s));
        assert_eq!(s.cancel(|| {}), CancelDelivery::Finished);
    }

    /// Cancel and wake race for one parked execution: whichever CAS is
    /// second rereads, so exactly one of them reports the task to enqueue.
    #[test]
    fn cancel_and_wake_of_a_parked_execution_enqueue_once() {
        // The cancel lands between the wake's read and CAS.
        let s = ExecState::new();
        let n = s.claim().unwrap();
        assert_eq!(s.park(n, || {}), Park::Parked);
        let mut fired = false;
        let delivery = s.wake(n, || {
            if !fired {
                fired = true;
                assert_eq!(s.cancel(|| {}), CancelDelivery::Runnable);
            }
        });
        assert_eq!(delivery, Delivery::Stale);

        // The wake lands between the cancel's read and CAS.
        let s = ExecState::new();
        let n = s.claim().unwrap();
        assert_eq!(s.park(n, || {}), Park::Parked);
        let mut fired = false;
        let cancel = s.cancel(|| {
            if !fired {
                fired = true;
                assert_eq!(s.wake(n, || {}), Delivery::Runnable);
            }
        });
        assert_eq!(cancel, CancelDelivery::Requested);
        assert_eq!(phase(&s), (Phase::Runnable, n));
        assert!(cancelled(&s));
    }

    /// A cancellation between a plain release's read and CAS fails the CAS;
    /// the release rereads and keeps the bit, so the task is runnable with
    /// the request recorded: the next claim keeps it and every wake of that
    /// execution is stale. (A store of the word computed before the
    /// cancellation would lose it and let the task resume ordinarily.)
    #[test]
    fn cancellation_between_release_read_and_cas_is_preserved() {
        let s = ExecState::new();
        let n = s.claim().unwrap();
        let mut fired = false;
        s.release_with(n, End::Runnable, || {
            if !fired {
                fired = true;
                assert_eq!(s.cancel(|| {}), CancelDelivery::Requested);
            }
        });
        assert!(fired);
        assert_eq!(phase(&s), (Phase::Runnable, n));
        assert!(cancelled(&s), "the release kept the request");
        let next = s.claim().unwrap();
        assert!(cancelled(&s), "the claim kept the request");
        assert_eq!(s.wake(next, || {}), Delivery::Stale);
        assert_eq!(s.park(next, || {}), Park::Cancelled);
    }

    /// A cancellation between park's read and CAS fails the CAS; park
    /// rereads and reports `Cancelled` with the owner still executing.
    #[test]
    fn a_cancel_between_park_read_and_cas_is_observed() {
        let s = ExecState::new();
        let n = s.claim().unwrap();
        let mut fired = false;
        let park = s.park(n, || {
            if !fired {
                fired = true;
                assert_eq!(s.cancel(|| {}), CancelDelivery::Requested);
            }
        });
        assert_eq!(park, Park::Cancelled);
        assert_eq!(phase(&s), (Phase::Executing { notified: false }, n));
    }
}
