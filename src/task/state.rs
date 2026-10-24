//! The task execution state machine: one atomic word per task.
//!
//! See the [`crate::task`] module docs for the states, transitions and
//! linearization points; this file is their implementation. Every transition
//! is a single compare-and-swap (or, for the owner leaving `Executing`, a
//! store) on [`ExecState`]'s word, so no lock is involved and nothing runs
//! inline: a transition only changes the word and reports what happened.
//!
//! Word layout (`u64`):
//!
//! | bits  | field |
//! |-------|-------|
//! | 0–1   | phase: `RUNNABLE`, `EXECUTING`, `PARKED`, `TERMINAL` |
//! | 2     | `NOTIFIED`: a wake for the current execution arrived while it was still executing |
//! | 3–63  | execution number: how many runs have been claimed (0 before the first) |

use std::sync::atomic::{AtomicU64, Ordering};

const PHASE_MASK: u64 = 0b11;
const RUNNABLE: u64 = 0;
const EXECUTING: u64 = 1;
const PARKED: u64 = 2;
const TERMINAL: u64 = 3;
const NOTIFIED: u64 = 0b100;
const EXECUTION_SHIFT: u32 = 3;

/// A decoded state word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Phase {
    /// Not executing and not waiting: a scheduler may claim it.
    Runnable,
    /// Owned by exactly one run guard. `notified`: a wake for this execution
    /// arrived before it finished parking.
    Executing { notified: bool },
    /// Suspended on a dependency subscribed during this execution; only a
    /// wake for this execution makes it runnable.
    Parked,
    /// Completed, failed or abandoned; never changes again.
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

    fn execution(self) -> u64 {
        self.0 >> EXECUTION_SHIFT
    }
}

/// Why a claim failed: the phase it found.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NotRunnable {
    Executing,
    Parked,
    Terminal,
}

/// What a park did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Park {
    /// `Executing → Parked`: a later wake makes the task runnable.
    Parked,
    /// A wake arrived first (`NOTIFIED`): `Executing → Runnable`. The parker
    /// hands the task back to its scheduler itself.
    AlreadyWoken,
}

/// What delivering a wake did to the state word.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Delivery {
    /// `Parked → Runnable`: the deliverer enqueues the task.
    Runnable,
    /// `Executing → Executing + NOTIFIED`: the owner's park will see it.
    Remembered,
    /// Nothing changed: the handle's execution is over or already notified.
    Stale,
}

/// The atomic state word of one task.
#[derive(Debug)]
pub(crate) struct ExecState(AtomicU64);

impl ExecState {
    pub(crate) fn new() -> ExecState {
        ExecState(AtomicU64::new(Word::new(RUNNABLE, 0).0))
    }

    /// Current phase and execution number. A snapshot.
    pub(crate) fn snapshot(&self) -> (Phase, u64) {
        // Acquire: a caller that sees `Runnable` after a wake also sees what
        // the waker published before it (not relied on for correctness).
        let w = Word(self.0.load(Ordering::Acquire));
        (w.phase(), w.execution())
    }

    /// `Runnable(n) → Executing(n + 1)`: the execution claim. On success the
    /// caller is the only owner until it calls `release` or `park`, and gets
    /// the new execution number.
    ///
    /// Ordering: `Acquire` on success pairs with the `Release` of whichever
    /// transition made the task runnable (the previous owner's release or
    /// park, or a waker), so the new owner sees everything they did first.
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
                Word::new(EXECUTING, execution).0,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                Ok(_) => return Ok(execution),
                Err(actual) => current = actual,
            }
        }
    }

    /// The owner of `execution` ends it: `Executing → Runnable` (yield, or a
    /// guard dropped without an outcome) or `Executing → Terminal`. A pending
    /// `NOTIFIED` is discarded: the task is runnable (or finished) anyway.
    ///
    /// A plain store is enough: while the phase is `Executing` the only other
    /// writer is a waker setting `NOTIFIED`, whose CAS then fails and rereads
    /// a phase that is no longer `Executing`. `Release` publishes the run's
    /// effects to the next claim's `Acquire`.
    pub(crate) fn release(&self, execution: u64, terminal: bool) {
        debug_assert!(matches!(
            Word(self.0.load(Ordering::Relaxed)).phase(),
            Phase::Executing { .. }
        ));
        let phase = if terminal { TERMINAL } else { RUNNABLE };
        self.0
            .store(Word::new(phase, execution).0, Ordering::Release);
    }

    /// The owner of `execution` suspends it (see [`Park`]). `before_cas` runs
    /// after each read of the word and before the CAS that depends on it
    /// (a test hook point; a no-op otherwise).
    ///
    /// Linearization point: the successful CAS to `Parked`, or the read that
    /// finds `NOTIFIED`. Ordering: `Release` on the CAS publishes the saved
    /// state to the waker's `Acquire`; `Acquire` on the read of `NOTIFIED`
    /// pairs with the waker's `Release`, so the owner sees the completion
    /// that woke it.
    pub(crate) fn park(&self, execution: u64, mut before_cas: impl FnMut()) -> Park {
        let parked = Word::new(PARKED, execution).0;
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let w = Word(current);
            debug_assert_eq!(w.execution(), execution, "only the owner parks");
            match w.phase() {
                Phase::Executing { notified: true } => {
                    self.0
                        .store(Word::new(RUNNABLE, execution).0, Ordering::Release);
                    return Park::AlreadyWoken;
                }
                Phase::Executing { notified: false } => {}
                other => unreachable!("park by the owner found {other:?}"),
            }
            before_cas();
            match self
                .0
                .compare_exchange(current, parked, Ordering::Release, Ordering::Acquire)
            {
                Ok(_) => return Park::Parked,
                // The only concurrent change is a waker setting NOTIFIED.
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
    /// with this execution number and no `NOTIFIED`, and both produce a word
    /// no other delivery for the same execution accepts. Ordering: `AcqRel`
    /// on success, so the completion published before the wake is visible
    /// to whoever claims or parks next.
    pub(crate) fn wake(&self, execution: u64, mut before_cas: impl FnMut()) -> Delivery {
        let mut current = self.0.load(Ordering::Acquire);
        loop {
            let w = Word(current);
            if w.execution() != execution {
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_word_encodes_phase_notification_and_execution() {
        let s = ExecState::new();
        assert_eq!(s.snapshot(), (Phase::Runnable, 0));
        assert_eq!(s.claim(), Ok(1));
        assert_eq!(s.claim(), Err(NotRunnable::Executing));
        assert_eq!(s.wake(1, || {}), Delivery::Remembered);
        assert_eq!(s.snapshot(), (Phase::Executing { notified: true }, 1));
        assert_eq!(s.wake(1, || {}), Delivery::Stale, "second wake");
        assert_eq!(s.park(1, || {}), Park::AlreadyWoken);
        assert_eq!(s.snapshot(), (Phase::Runnable, 1));
        assert_eq!(s.wake(1, || {}), Delivery::Stale, "the suspension is over");
        assert_eq!(s.claim(), Ok(2));
        assert_eq!(s.park(2, || {}), Park::Parked);
        assert_eq!(s.claim(), Err(NotRunnable::Parked));
        assert_eq!(s.wake(1, || {}), Delivery::Stale, "older execution");
        assert_eq!(s.wake(3, || {}), Delivery::Stale, "future execution");
        assert_eq!(s.wake(2, || {}), Delivery::Runnable);
        assert_eq!(s.wake(2, || {}), Delivery::Stale, "duplicate");
        assert_eq!(s.claim(), Ok(3));
        s.release(3, true);
        assert_eq!(s.snapshot(), (Phase::Terminal, 3));
        assert_eq!(s.claim(), Err(NotRunnable::Terminal));
        assert_eq!(s.wake(3, || {}), Delivery::Stale);
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
        assert_eq!(s.snapshot(), (Phase::Runnable, n));
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
        assert_eq!(s.snapshot(), (Phase::Runnable, n));
    }
}
