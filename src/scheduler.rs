//! The compiler's work pool: a fixed set of scoped worker threads, phase
//! queues, batched claims and work stealing, on the standard library alone
//! (docs/compiler-concurrency.md, "The scheduler").
//!
//! The pool knows nothing about the compiler. A [`Job`] says which phase
//! queue it belongs to, what it is estimated to cost in time and in bytes,
//! and its priority (the length of the dependency chain it heads, longest
//! first). The caller supplies a function that creates one slot's state on
//! the thread that owns it, such as a worker's isolate, and one that runs a
//! job against that state; it drives the build from the calling thread
//! through a [`Coordinator`]: it submits ready jobs, consumes their outcomes
//! in completion order, registers successors and alone decides when the
//! build is done. Nothing here decides output order: canonical order comes
//! from the caller's numbering.
//!
//! `slots` counts CPU work slots including the coordinator's own, so a pool
//! of N slots has N-1 worker threads. With one slot nothing is spawned:
//! [`Coordinator::next_outcome`] runs each job inline on the calling thread through
//! the same queues, claims and accounting.
//!
//! All scheduling state sits behind one mutex, held for bookkeeping only;
//! jobs, slot constructors and the destructors of dropped jobs run with it
//! released. Workers wait on one condition variable for runnable work and
//! the coordinator on another for outcomes, each around a predicate checked
//! under the mutex. There is no polling sleep.
#![forbid(unsafe_code)]

use std::{
    any::Any,
    collections::VecDeque,
    fmt,
    num::NonZeroUsize,
    panic::{self, AssertUnwindSafe},
    sync::{
        Condvar, Mutex, MutexGuard,
        atomic::{AtomicBool, Ordering},
    },
    thread,
};

/// CPU work slots for a build (`--jobs N`): the coordinator's own counts as
/// one, so there are at most N-1 worker threads. The default is 1.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Slots(NonZeroUsize);

impl Slots {
    /// One slot: every job runs inline on the calling thread.
    pub const ONE: Slots = Slots(NonZeroUsize::MIN);

    /// `count` slots, or `None` for zero.
    pub fn new(count: usize) -> Option<Slots> {
        NonZeroUsize::new(count).map(Slots)
    }

    /// The number of slots.
    pub fn get(self) -> usize {
        self.0.get()
    }

    /// The worker threads a pool of these slots spawns.
    pub fn workers(self) -> usize {
        self.0.get() - 1
    }
}

impl Default for Slots {
    fn default() -> Slots {
        Slots::ONE
    }
}

/// What the pool needs to know about a unit of work. Running it is the
/// caller's function, so a job is plain owned data that may cross threads;
/// engine handles never enter a queue.
pub trait Job: Send {
    /// The phase queue the job waits in, below [`Config::phases`].
    fn phase(&self) -> usize;

    /// Its estimated cost, in whatever unit the caller uses consistently;
    /// batch limits count it ([`Limits::batch_cost`]).
    fn cost(&self) -> u64 {
        1
    }

    /// The bytes it is estimated to hold until its outcome is consumed;
    /// admission counts them ([`Limits::admitted_bytes`]).
    fn bytes(&self) -> u64 {
        0
    }

    /// Higher runs first within its phase, ties in submission order: the
    /// design's critical-path-first, by the dependency chain the job heads.
    fn priority(&self) -> u64 {
        0
    }

    /// A stable identity for the test-only [`Perturbation`], which keys its
    /// delays, fixed assignment and log on it.
    #[cfg(debug_assertions)]
    fn key(&self) -> u64 {
        0
    }
}

/// The scheduler's bounds. Byte estimates guide admission; they are not a
/// hard memory bound.
#[derive(Clone, Debug)]
pub struct Limits {
    /// The most jobs one claim from a shared queue, or one steal, takes.
    pub batch_jobs: usize,
    /// The most estimated cost one claim or steal takes in all, though its
    /// first job is always taken.
    pub batch_cost: u64,
    /// Jobs admitted at once: queued, running and completed but not yet
    /// consumed. The completion deque reserves room for all of them, so a
    /// worker never waits to publish. Further submissions wait unrunnable
    /// in the backlog, highest priority first, except that a phase whose
    /// backlog has waited `phase_quantum` admissions is admitted next.
    pub admitted_jobs: usize,
    /// Estimated bytes admitted at once. A job is always admitted when
    /// nothing else is, however large.
    pub admitted_bytes: u64,
    /// Consecutive jobs a worker runs in one phase before it reconsiders
    /// which ready phase to serve, choosing the one served longest ago; and
    /// the admissions a phase's backlog waits at most while others admit.
    pub phase_quantum: usize,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits {
            batch_jobs: 4,
            batch_cost: u64::MAX,
            admitted_jobs: 4096,
            admitted_bytes: 1 << 30,
            phase_quantum: 32,
        }
    }
}

/// How a pool runs.
#[derive(Clone, Debug)]
pub struct Config {
    pub slots: Slots,
    /// The number of phase queues; every job's phase is below it.
    pub phases: usize,
    pub limits: Limits,
    /// The stack size of each worker thread, or the platform default.
    pub stack_size: Option<usize>,
    /// The test-only scheduling perturbation, debug builds only.
    #[cfg(debug_assertions)]
    pub perturbation: Option<Perturbation>,
}

impl Config {
    pub fn new(slots: Slots, phases: usize) -> Config {
        Config {
            slots,
            phases,
            limits: Limits::default(),
            stack_size: None,
            #[cfg(debug_assertions)]
            perturbation: None,
        }
    }
}

/// Work the coordinator registers as able to create more jobs, which keeps
/// the build from being quiescent while no job is admitted.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Hold {
    /// A demand suspended until another job's outcome.
    Demand,
    /// An outcome taken but not yet published into the coordinator's state.
    Publication,
}

/// What [`Coordinator::next_outcome`] found.
#[derive(Debug, Eq, PartialEq)]
pub enum Next<O> {
    /// A finished job's outcome, in completion order.
    Outcome(O),
    /// Nothing admitted, waiting or held: the coordinator may declare the
    /// build done once its own state agrees.
    Quiescent,
    /// Nothing admitted or waiting, yet demands or publications are held:
    /// no job can produce what they wait for. That is a cycle, a missing
    /// producer or a bug, for the coordinator to diagnose.
    Stalled { demands: usize, publications: usize },
}

/// A job, or a slot's constructor, panicked. The slot is retired and the
/// build stopped.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WorkerPanic {
    /// The slot that panicked: 0 is the coordinator's (the inline path).
    pub slot: usize,
    pub message: String,
}

impl fmt::Display for WorkerPanic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Not the slot: which one ran a job is the schedule's choice, and
        // a build's diagnostics must not depend on it.
        write!(f, "a compiler worker panicked: {}", self.message)
    }
}

impl std::error::Error for WorkerPanic {}

/// Why [`run`] did not return the coordinator's result.
#[derive(Debug)]
pub enum PoolError {
    /// A worker thread could not be started; the coordinator did not run.
    Spawn(std::io::Error),
    Panic(WorkerPanic),
}

impl fmt::Display for PoolError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PoolError::Spawn(error) => write!(f, "cannot start a compiler worker: {error}"),
            PoolError::Panic(panic) => panic.fmt(f),
        }
    }
}

impl std::error::Error for PoolError {}

/// What the pool did, for `--stats` and tests.
#[derive(Clone, Debug, Default, Eq, PartialEq)]
pub struct PoolCounts {
    /// Jobs started, by any path.
    pub started: u64,
    pub urgent: u64,
    /// Jobs started from a worker's own deque.
    pub local: u64,
    /// Claims from a shared phase queue, and the jobs they took.
    pub claims: u64,
    pub claimed: u64,
    /// Steals from a peer's deque, and the jobs they took.
    pub steals: u64,
    pub stolen: u64,
    /// Times a worker changed the phase it serves.
    pub phase_switches: u64,
    /// Times a worker waited for runnable work.
    pub parks: u64,
    /// The most jobs admitted at once.
    pub peak_admitted: usize,
    /// The most jobs waiting in the backlog at once.
    pub peak_backlog: usize,
    /// Jobs dropped unstarted by [`Coordinator::cancel`].
    pub cancelled: u64,
}

/// A job admitted to a queue, with what the scheduler reads of it.
struct Entry<J> {
    job: J,
    phase: usize,
    priority: u64,
    cost: u64,
    bytes: u64,
    /// Submission order, which breaks priority ties across phases.
    seq: u64,
    #[cfg(debug_assertions)]
    key: u64,
}

impl<J: Job> Entry<J> {
    fn new(job: J, phases: usize) -> Entry<J> {
        let phase = job.phase();
        assert!(
            phase < phases,
            "job phase {phase} is not below the pool's {phases} phases"
        );
        Entry {
            phase,
            priority: job.priority(),
            cost: job.cost(),
            bytes: job.bytes(),
            seq: 0,
            #[cfg(debug_assertions)]
            key: job.key(),
            job,
        }
    }
}

/// Insert `entry` after every entry of at least its priority.
fn insert_by_priority<J>(queue: &mut VecDeque<Entry<J>>, entry: Entry<J>) {
    let at = queue.partition_point(|queued| queued.priority >= entry.priority);
    queue.insert(at, entry);
}

/// How many entries from the front of `entries` one claim or steal takes:
/// at least one, then while the batch limits allow.
fn batch_len<'a, J: 'a>(entries: impl Iterator<Item = &'a Entry<J>>, limits: &Limits) -> usize {
    let mut count = 0;
    let mut cost = 0u64;
    for entry in entries {
        let total = cost.saturating_add(entry.cost);
        if count > 0 && (count >= limits.batch_jobs || total > limits.batch_cost) {
            break;
        }
        count += 1;
        cost = total;
    }
    count
}

/// A worker's place in the claim policy.
#[derive(Clone, Copy, Default)]
struct Cursor {
    phase: usize,
    /// Jobs run in `phase` since the worker last chose it.
    streak: usize,
    /// The peer to try stealing from first, rotated after each steal.
    victim: usize,
}

/// Everything behind the mutex.
struct State<J, O> {
    shutdown: bool,
    cancelled: bool,
    /// Work the coordinator or a waiting demand needs next, any phase.
    urgent: VecDeque<Entry<J>>,
    /// Shared ready queues by phase, highest priority first.
    ready: Vec<VecDeque<Entry<J>>>,
    /// Each worker's unstarted jobs by phase. The owner pops from the back,
    /// thieves take from the front.
    local: Vec<Vec<VecDeque<Entry<J>>>>,
    /// Runnable jobs per phase, in `ready` and every `local` deque.
    runnable: Vec<usize>,
    /// Every runnable job: `urgent` and every phase's.
    runnable_total: usize,
    /// Submitted but not admitted, by phase, highest priority first; not
    /// runnable.
    backlog: Vec<VecDeque<Entry<J>>>,
    backlog_len: usize,
    /// The admission clock when each phase's backlog last admitted a job or
    /// stopped being empty, for aging.
    admitted_at: Vec<u64>,
    admit_clock: u64,
    next_seq: u64,
    running: usize,
    /// Outcomes with the bytes their jobs were admitted with.
    completed: VecDeque<(O, u64)>,
    admitted_jobs: usize,
    admitted_bytes: u64,
    demands: usize,
    publications: usize,
    /// Workers waiting for runnable work.
    parked: usize,
    cursors: Vec<Cursor>,
    /// The claim clock when each phase last started a job, for aging.
    served: Vec<u64>,
    clock: u64,
    panic: Option<WorkerPanic>,
    counts: PoolCounts,
    #[cfg(debug_assertions)]
    perturbed: Option<perturb::Perturbed>,
}

/// What one claim found.
struct Claim<J> {
    entry: Entry<J>,
    /// Unstarted jobs the claim left in the claimer's deque, which parked
    /// peers may steal.
    left: usize,
}

impl<J: Job, O> State<J, O> {
    fn new(config: &Config, deques: usize) -> State<J, O> {
        let phases = config.phases;
        assert!(phases > 0, "a pool needs at least one phase");
        State {
            shutdown: false,
            cancelled: false,
            urgent: VecDeque::new(),
            ready: (0..phases).map(|_| VecDeque::new()).collect(),
            local: (0..deques)
                .map(|_| (0..phases).map(|_| VecDeque::new()).collect())
                .collect(),
            runnable: vec![0; phases],
            runnable_total: 0,
            backlog: (0..phases).map(|_| VecDeque::new()).collect(),
            backlog_len: 0,
            admitted_at: vec![0; phases],
            admit_clock: 0,
            next_seq: 0,
            running: 0,
            completed: VecDeque::with_capacity(limits(config).admitted_jobs.max(1)),
            admitted_jobs: 0,
            admitted_bytes: 0,
            demands: 0,
            publications: 0,
            parked: 0,
            cursors: vec![Cursor::default(); deques],
            served: vec![0; phases],
            clock: 0,
            panic: None,
            counts: PoolCounts::default(),
            #[cfg(debug_assertions)]
            perturbed: config.perturbation.clone().map(perturb::Perturbed::new),
        }
    }

    fn phases(&self) -> usize {
        self.ready.len()
    }

    /// Whether `entry` fits the admission bounds now.
    fn has_room(&self, limits: &Limits, bytes: u64) -> bool {
        self.admitted_jobs == 0
            || (self.admitted_jobs < limits.admitted_jobs
                && self.admitted_bytes.saturating_add(bytes) <= limits.admitted_bytes)
    }

    fn count_admission(&mut self, bytes: u64) {
        self.admitted_jobs += 1;
        self.admitted_bytes += bytes;
        self.counts.peak_admitted = self.counts.peak_admitted.max(self.admitted_jobs);
    }

    /// The phase whose backlog admits next: the one waiting longest once it
    /// has waited `phase_quantum` admissions, else the one whose front has
    /// the highest priority, ties in submission order.
    fn backlog_phase(&self, limits: &Limits) -> Option<usize> {
        let waiting = (0..self.phases()).filter(|&phase| !self.backlog[phase].is_empty());
        let aged = waiting
            .clone()
            .filter(|&phase| {
                self.admit_clock - self.admitted_at[phase] >= limits.phase_quantum.max(1) as u64
            })
            .min_by_key(|&phase| self.admitted_at[phase]);
        aged.or_else(|| {
            waiting.min_by_key(|&phase| {
                let front = &self.backlog[phase][0];
                (std::cmp::Reverse(front.priority), front.seq)
            })
        })
    }

    /// Admit backlog jobs in [`State::backlog_phase`] order while they fit;
    /// return how many became runnable.
    fn admit(&mut self, limits: &Limits) -> usize {
        let mut admitted = 0;
        while let Some(phase) = self.backlog_phase(limits) {
            let bytes = self.backlog[phase][0].bytes;
            if self.cancelled || !self.has_room(limits, bytes) {
                break;
            }
            let entry = self.backlog[phase].pop_front().expect("front exists");
            self.backlog_len -= 1;
            self.admit_clock += 1;
            self.admitted_at[phase] = self.admit_clock;
            self.count_admission(entry.bytes);
            self.runnable[entry.phase] += 1;
            self.runnable_total += 1;
            let phase = entry.phase;
            insert_by_priority(&mut self.ready[phase], entry);
            admitted += 1;
        }
        admitted
    }

    /// Queue `entry` for admission and admit what fits.
    fn submit(&mut self, limits: &Limits, mut entry: Entry<J>) -> usize {
        entry.seq = self.next_seq;
        self.next_seq += 1;
        let phase = entry.phase;
        if self.backlog[phase].is_empty() {
            // Its wait starts now.
            self.admitted_at[phase] = self.admit_clock;
        }
        insert_by_priority(&mut self.backlog[phase], entry);
        self.backlog_len += 1;
        self.counts.peak_backlog = self.counts.peak_backlog.max(self.backlog_len);
        self.admit(limits)
    }

    /// Mark `entry` started.
    fn start(&mut self, entry: Entry<J>) -> Entry<J> {
        self.running += 1;
        self.clock += 1;
        self.served[entry.phase] = self.clock;
        self.counts.started += 1;
        entry
    }

    fn take_runnable(&mut self, phase: usize) {
        self.runnable[phase] -= 1;
        self.runnable_total -= 1;
    }

    /// The phase worker `w` serves next, which has runnable work: its
    /// current one until the quantum is spent, then the runnable phase
    /// served longest ago, ties going to the phases after the current one.
    fn choose_phase(&mut self, w: usize, limits: &Limits) -> usize {
        let phases = self.phases();
        let cursor = self.cursors[w];
        if self.runnable[cursor.phase] > 0 && cursor.streak < limits.phase_quantum.max(1) {
            self.cursors[w].streak += 1;
            return cursor.phase;
        }
        let mut best: Option<usize> = None;
        for step in 1..=phases {
            let phase = (cursor.phase + step) % phases;
            if self.runnable[phase] > 0 && best.is_none_or(|b| self.served[phase] < self.served[b])
            {
                best = Some(phase);
            }
        }
        let phase = best.expect("a runnable phase exists");
        if phase != cursor.phase {
            self.counts.phase_switches += 1;
        }
        self.cursors[w].phase = phase;
        self.cursors[w].streak = 1;
        phase
    }

    /// The next job for worker `w`, in the design's order: cancellation,
    /// urgent work, the local deque, a batch from the phase's shared queue,
    /// a steal from a peer, then another ready phase. `None` means nothing
    /// is runnable anywhere, or the build is cancelled or shutting down.
    fn claim(&mut self, w: usize, limits: &Limits) -> Option<Claim<J>> {
        if self.shutdown || self.cancelled {
            return None;
        }
        #[cfg(debug_assertions)]
        if self.perturbed.is_some() {
            return self.claim_perturbed(w, limits);
        }
        if let Some(entry) = self.urgent.pop_front() {
            self.runnable_total -= 1;
            self.counts.urgent += 1;
            return Some(Claim {
                entry: self.start(entry),
                left: 0,
            });
        }
        if self.runnable_total == 0 {
            return None;
        }
        let phase = self.choose_phase(w, limits);
        if let Some(entry) = self.local[w][phase].pop_back() {
            self.take_runnable(phase);
            self.counts.local += 1;
            return Some(Claim {
                entry: self.start(entry),
                left: 0,
            });
        }
        let (taken, stolen) = if !self.ready[phase].is_empty() {
            self.counts.claims += 1;
            (self.claim_batch(w, phase, limits), false)
        } else {
            self.counts.steals += 1;
            (self.steal(w, phase, limits), true)
        };
        if stolen {
            self.counts.stolen += taken as u64;
        } else {
            self.counts.claimed += taken as u64;
        }
        let entry = self.local[w][phase]
            .pop_back()
            .expect("a claim leaves at least one job");
        self.take_runnable(phase);
        Some(Claim {
            entry: self.start(entry),
            left: taken - 1,
        })
    }

    /// Move a bounded batch from the front of `phase`'s shared queue into
    /// worker `w`'s empty deque, the first job at the back where the owner
    /// pops it, so thieves take from the far end. Return the batch size.
    fn claim_batch(&mut self, w: usize, phase: usize, limits: &Limits) -> usize {
        let count = batch_len(self.ready[phase].iter(), limits);
        let batch: Vec<_> = self.ready[phase].drain(..count).collect();
        let local = &mut self.local[w][phase];
        for entry in batch.into_iter().rev() {
            local.push_back(entry);
        }
        count
    }

    /// Move about half of a peer's unstarted `phase` jobs, from the front of
    /// its deque and capped by the batch limits, onto the back of worker
    /// `w`'s, rotating the victim. Return how many moved.
    fn steal(&mut self, w: usize, phase: usize, limits: &Limits) -> usize {
        let peers = self.local.len();
        let start = self.cursors[w].victim;
        let victim = (0..peers)
            .map(|step| (start + step) % peers)
            .find(|&v| v != w && !self.local[v][phase].is_empty())
            .expect("runnable work in a phase with an empty shared queue is in a peer's deque");
        self.cursors[w].victim = (victim + 1) % peers;
        let available = self.local[victim][phase].len();
        let half = available.div_ceil(2);
        let count = batch_len(self.local[victim][phase].iter().take(half), limits);
        let taken: Vec<_> = self.local[victim][phase].drain(..count).collect();
        self.local[w][phase].extend(taken);
        count
    }

    /// Return a retired worker's unstarted jobs to the shared queues.
    fn retire(&mut self, w: usize) {
        for phase in 0..self.phases() {
            let jobs: Vec<_> = self.local[w][phase].drain(..).collect();
            for entry in jobs {
                insert_by_priority(&mut self.ready[phase], entry);
            }
        }
    }

    /// Take every unstarted job out of the queues and the backlog, for the
    /// caller to drop with the mutex released.
    fn drain_unstarted(&mut self) -> Vec<J> {
        let mut admitted: Vec<Entry<J>> = self.urgent.drain(..).collect();
        for queue in &mut self.ready {
            admitted.extend(queue.drain(..));
        }
        for queue in self.local.iter_mut().flatten() {
            admitted.extend(queue.drain(..));
        }
        self.admitted_jobs -= admitted.len();
        self.admitted_bytes -= admitted.iter().map(|entry| entry.bytes).sum::<u64>();
        let mut dropped: Vec<J> = admitted.into_iter().map(|entry| entry.job).collect();
        for queue in &mut self.backlog {
            dropped.extend(queue.drain(..).map(|entry| entry.job));
        }
        self.backlog_len = 0;
        self.runnable.iter_mut().for_each(|count| *count = 0);
        self.runnable_total = 0;
        self.counts.cancelled += dropped.len() as u64;
        dropped
    }

    /// The coordinator's view once no outcome is waiting.
    fn quiescence(&self) -> Option<Next<O>> {
        if self.admitted_jobs > 0 || self.backlog_len > 0 {
            return None;
        }
        Some(if self.demands == 0 && self.publications == 0 {
            Next::Quiescent
        } else {
            Next::Stalled {
                demands: self.demands,
                publications: self.publications,
            }
        })
    }

    /// Take the oldest outcome, freeing its admission.
    fn consume(&mut self, limits: &Limits) -> Option<(O, usize)> {
        let (outcome, bytes) = self.completed.pop_front()?;
        self.admitted_jobs -= 1;
        self.admitted_bytes -= bytes;
        Some((outcome, self.admit(limits)))
    }
}

/// The state every slot shares.
struct Shared<J, O> {
    state: Mutex<State<J, O>>,
    /// Workers waiting for runnable work.
    work: Condvar,
    /// The coordinator waiting for an outcome.
    outcomes: Condvar,
    /// Mirrors `State::cancelled` for running jobs to poll.
    cancelled: AtomicBool,
    limits: Limits,
    phases: usize,
    #[cfg(debug_assertions)]
    perturbation: Option<Perturbation>,
}

impl<J: Job, O> Shared<J, O> {
    fn lock(&self) -> MutexGuard<'_, State<J, O>> {
        // A panic under the lock is a scheduler bug; shutdown still needs
        // the state, so poisoning is not a second failure.
        self.state
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Wake workers for `count` newly runnable jobs: one for one, all for
    /// several.
    fn wake(&self, count: usize) {
        // Under a fixed assignment only the job's owner may run it.
        #[cfg(debug_assertions)]
        if count > 0 && self.perturbation.as_ref().is_some_and(|p| p.fixed) {
            self.work.notify_all();
            return;
        }
        match count {
            0 => {}
            1 => self.work.notify_one(),
            _ => self.work.notify_all(),
        }
    }

    /// Set shutdown and wake both wait sets, on every exit path.
    fn shut_down(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.lock().shutdown = true;
        self.work.notify_all();
        self.outcomes.notify_all();
    }
}

/// Sets shutdown when the coordinator's scope ends, however it ends.
struct ShutdownGuard<'a, J: Job, O>(&'a Shared<J, O>);

impl<J: Job, O> Drop for ShutdownGuard<'_, J, O> {
    fn drop(&mut self) {
        self.0.shut_down();
    }
}

/// What a running job sees of the pool.
pub struct Worker<'a, J: Job, O> {
    shared: &'a Shared<J, O>,
    /// This slot's deque index.
    index: usize,
    slot: usize,
}

impl<J: Job, O> Worker<'_, J, O> {
    /// The slot running the job: 0 is the coordinator's, on the inline
    /// path; workers are 1 to N-1.
    pub fn slot(&self) -> usize {
        self.slot
    }

    /// Whether the build is cancelled or stopping (a panic, or the end of
    /// the coordinator's closure), for a long job to stop early; it is
    /// checked again before every claim.
    pub fn cancelled(&self) -> bool {
        self.shared.cancelled.load(Ordering::Relaxed)
    }

    /// Queue a job whose readiness is already known on this worker's own
    /// deque, under the same admission as a submission, where this worker
    /// runs it next unless a peer steals it. When admission is full it goes
    /// to the backlog instead. Anything that touches ordered state belongs
    /// in an outcome for the coordinator, not here.
    pub fn push_local(&self, job: J) {
        let entry = Entry::new(job, self.shared.phases);
        let mut state = self.shared.lock();
        if state.cancelled {
            state.counts.cancelled += 1;
            drop(state);
            return;
        }
        #[cfg(debug_assertions)]
        let assigned = state.assigns();
        #[cfg(not(debug_assertions))]
        let assigned = false;
        if !assigned && state.backlog_len == 0 && state.has_room(&self.shared.limits, entry.bytes) {
            state.count_admission(entry.bytes);
            state.runnable[entry.phase] += 1;
            state.runnable_total += 1;
            let phase = entry.phase;
            state.local[self.index][phase].push_back(entry);
            let parked = state.parked;
            drop(state);
            if parked > 0 {
                self.shared.work.notify_one();
            }
        } else {
            let admitted = state.submit(&self.shared.limits, entry);
            drop(state);
            self.shared.wake(admitted);
        }
    }
}

type MakeSlot<'a, W> = &'a (dyn Fn(usize) -> W + Sync);
type Execute<'a, J, O, W> = &'a (dyn Fn(&mut W, J, &Worker<'_, J, O>) -> O + Sync);

/// Run one job with panics caught at the job boundary, creating the slot's
/// state on first use.
fn execute_caught<J: Job, O, W>(
    worker: &Worker<'_, J, O>,
    slot_state: &mut Option<W>,
    make: MakeSlot<'_, W>,
    execute: Execute<'_, J, O, W>,
    job: J,
) -> Result<O, WorkerPanic> {
    panic::catch_unwind(AssertUnwindSafe(|| {
        let state = slot_state.get_or_insert_with(|| make(worker.slot));
        execute(state, job, worker)
    }))
    .map_err(|payload| WorkerPanic {
        slot: worker.slot,
        message: panic_message(payload.as_ref()),
    })
}

fn panic_message(payload: &(dyn Any + Send)) -> String {
    if let Some(text) = payload.downcast_ref::<&str>() {
        (*text).to_owned()
    } else if let Some(text) = payload.downcast_ref::<String>() {
        text.clone()
    } else {
        "a non-string panic payload".to_owned()
    }
}

/// Publish a finished job's result: its outcome, or its panic, which
/// retires the slot and stops the build.
fn publish<J: Job, O>(
    shared: &Shared<J, O>,
    state: &mut State<J, O>,
    index: usize,
    bytes: u64,
    result: Result<O, WorkerPanic>,
) -> bool {
    state.running -= 1;
    match result {
        Ok(outcome) => {
            debug_assert!(state.completed.len() < state.completed.capacity());
            state.completed.push_back((outcome, bytes));
            shared.outcomes.notify_one();
            true
        }
        Err(panic) => {
            state.retire(index);
            state.panic.get_or_insert(panic);
            state.shutdown = true;
            // Running jobs that poll for cancellation stop too, so the
            // scope can join them.
            shared.cancelled.store(true, Ordering::Relaxed);
            shared.work.notify_all();
            shared.outcomes.notify_all();
            false
        }
    }
}

/// A worker thread's loop: claim, run with the mutex released, publish;
/// park only when nothing is runnable anywhere.
fn work<J: Job, O, W>(
    shared: &Shared<J, O>,
    index: usize,
    make: MakeSlot<'_, W>,
    execute: Execute<'_, J, O, W>,
) {
    let worker = Worker {
        shared,
        index,
        slot: index + 1,
    };
    let mut slot_state = None;
    let mut state = shared.lock();
    loop {
        if state.shutdown {
            break;
        }
        let Some(Claim { entry, left }) = state.claim(index, &shared.limits) else {
            state.parked += 1;
            state.counts.parks += 1;
            state = shared
                .work
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            state.parked -= 1;
            continue;
        };
        let wake = left.min(state.parked);
        drop(state);
        for _ in 0..wake {
            shared.work.notify_one();
        }
        let bytes = entry.bytes;
        #[cfg(debug_assertions)]
        let key = entry.key;
        #[cfg(debug_assertions)]
        if let Some(p) = &shared.perturbation {
            p.pause(key, index, perturb::Boundary::Start);
        }
        let result = execute_caught(&worker, &mut slot_state, make, execute, entry.job);
        #[cfg(debug_assertions)]
        if let Some(p) = &shared.perturbation {
            p.pause(key, index, perturb::Boundary::Finish);
        }
        let failed = result.is_err();
        if failed {
            // The slot's scratch state is discarded with its panic.
            drop(slot_state.take());
        }
        state = shared.lock();
        if !publish(shared, &mut state, index, bytes, result) {
            break;
        }
    }
    drop(state);
    drop(slot_state);
}

/// The calling thread's handle on the pool: it submits ready jobs, consumes
/// outcomes and decides when the build is done.
pub struct Coordinator<'a, J: Job, O, W> {
    shared: &'a Shared<J, O>,
    slots: Slots,
    /// The coordinator's own slot state on the inline path.
    inline: Option<Inline<'a, J, O, W>>,
}

struct Inline<'a, J: Job, O, W> {
    make: MakeSlot<'a, W>,
    execute: Execute<'a, J, O, W>,
    state: Option<W>,
}

impl<J: Job, O, W> Coordinator<'_, J, O, W> {
    /// The pool's slots, the coordinator's included.
    pub fn slots(&self) -> Slots {
        self.slots
    }

    /// Admit `job` when it fits the bounds, else hold it in the backlog;
    /// one newly runnable job wakes one worker.
    pub fn submit(&mut self, job: J) {
        let entry = Entry::new(job, self.shared.phases);
        #[cfg(debug_assertions)]
        if let Some(p) = &self.shared.perturbation {
            p.pause(entry.key, usize::MAX, perturb::Boundary::Submit);
        }
        let mut state = self.shared.lock();
        if state.cancelled {
            state.counts.cancelled += 1;
            return;
        }
        let admitted = state.submit(&self.shared.limits, entry);
        drop(state);
        self.shared.wake(admitted);
    }

    /// Submit several jobs under one lock; a batch wakes every worker.
    pub fn submit_batch(&mut self, jobs: impl IntoIterator<Item = J>) {
        let entries: Vec<_> = jobs
            .into_iter()
            .map(|job| Entry::new(job, self.shared.phases))
            .collect();
        #[cfg(debug_assertions)]
        if let Some(p) = &self.shared.perturbation
            && let Some(first) = entries.first()
        {
            p.pause(first.key, usize::MAX, perturb::Boundary::Submit);
        }
        let mut state = self.shared.lock();
        if state.cancelled {
            state.counts.cancelled += entries.len() as u64;
            drop(state);
            return;
        }
        let mut admitted = 0;
        for entry in entries {
            admitted += state.submit(&self.shared.limits, entry);
        }
        drop(state);
        self.shared.wake(admitted);
    }

    /// Run `job` before any queued work, as soon as a slot frees: work the
    /// coordinator or a waiting demand needs now. It is admitted at once,
    /// whatever the bounds, since what waits for it may hold the room.
    pub fn submit_urgent(&mut self, job: J) {
        let entry = Entry::new(job, self.shared.phases);
        let mut state = self.shared.lock();
        if state.cancelled {
            state.counts.cancelled += 1;
            return;
        }
        state.count_admission(entry.bytes);
        state.urgent.push_back(entry);
        state.runnable_total += 1;
        // Room for its outcome too, beyond the bounds.
        let room = state.admitted_jobs.saturating_sub(state.completed.len());
        state.completed.reserve(room);
        drop(state);
        self.shared.work.notify_one();
    }

    /// Register work that may create more jobs although none is admitted.
    pub fn hold(&mut self, hold: Hold) {
        let mut state = self.shared.lock();
        match hold {
            Hold::Demand => state.demands += 1,
            Hold::Publication => state.publications += 1,
        }
    }

    /// Release what [`Coordinator::hold`] registered.
    pub fn release(&mut self, hold: Hold) {
        let mut state = self.shared.lock();
        let count = match hold {
            Hold::Demand => &mut state.demands,
            Hold::Publication => &mut state.publications,
        };
        *count = count
            .checked_sub(1)
            .unwrap_or_else(|| panic!("released a {hold:?} that was not held"));
    }

    /// Stop starting jobs: drop every unstarted one and refuse new ones.
    /// Running jobs finish, and [`Coordinator::next_outcome`] still returns their
    /// outcomes; [`Worker::cancelled`] lets a long one stop early.
    pub fn cancel(&mut self) {
        self.shared.cancelled.store(true, Ordering::Relaxed);
        let mut state = self.shared.lock();
        state.cancelled = true;
        let dropped = state.drain_unstarted();
        drop(state);
        self.shared.outcomes.notify_all();
        drop(dropped);
    }

    /// What the pool has done so far.
    pub fn counts(&self) -> PoolCounts {
        self.shared.lock().counts.clone()
    }

    /// Jobs submitted but not yet admitted, which a caller producing work
    /// faster than it runs can use to hold back.
    pub fn backlogged(&self) -> usize {
        self.shared.lock().backlog_len
    }

    /// Workers waiting for runnable work now.
    pub fn parked_workers(&self) -> usize {
        self.shared.lock().parked
    }

    /// The next outcome, in completion order, waiting for one while jobs
    /// are admitted. Register an outcome's successors (submissions and
    /// holds) before calling again: only then does [`Next::Quiescent`] mean
    /// that no work remains. On the inline path this runs the next job on
    /// the calling thread. A panic stops the build; every later call
    /// returns it again.
    pub fn next_outcome(&mut self) -> Result<Next<O>, WorkerPanic> {
        if self.inline.is_some() {
            return self.next_inline();
        }
        let shared = self.shared;
        let mut state = shared.lock();
        loop {
            if let Some(panic) = &state.panic {
                return Err(panic.clone());
            }
            if let Some((outcome, admitted)) = state.consume(&shared.limits) {
                drop(state);
                shared.wake(admitted);
                return Ok(Next::Outcome(outcome));
            }
            if let Some(next) = state.quiescence() {
                return Ok(next);
            }
            assert!(
                state.running > 0 || state.runnable_total > 0,
                "scheduler bug: {} admitted jobs are neither runnable, running nor completed",
                state.admitted_jobs
            );
            state = shared
                .outcomes
                .wait(state)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// [`Coordinator::next_outcome`] with one slot: claim and run on this thread.
    fn next_inline(&mut self) -> Result<Next<O>, WorkerPanic> {
        let shared = self.shared;
        loop {
            let mut state = shared.lock();
            if let Some(panic) = &state.panic {
                return Err(panic.clone());
            }
            if let Some((outcome, _)) = state.consume(&shared.limits) {
                return Ok(Next::Outcome(outcome));
            }
            if let Some(next) = state.quiescence() {
                return Ok(next);
            }
            let Claim { entry, .. } = state
                .claim(0, &shared.limits)
                .expect("admitted work with nothing running inline is runnable");
            drop(state);
            let inline = self.inline.as_mut().expect("the inline path");
            let worker = Worker {
                shared,
                index: 0,
                slot: 0,
            };
            let bytes = entry.bytes;
            let result = execute_caught(
                &worker,
                &mut inline.state,
                inline.make,
                inline.execute,
                entry.job,
            );
            if result.is_err() {
                drop(inline.state.take());
            }
            let mut state = shared.lock();
            publish(shared, &mut state, 0, bytes, result);
        }
    }
}

/// Run a build on a pool of `config.slots` slots. `make(slot)` creates a
/// slot's state on the thread that owns it, when that slot first runs a
/// job; `execute` runs a job against it. `coordinate` drives the build on
/// the calling thread and its result is returned once every worker has
/// joined. However `coordinate` ends, including by panicking, shutdown is
/// set and both wait sets are woken before the threads are joined. A job
/// that panicked makes the result [`PoolError::Panic`], whatever
/// `coordinate` returned; a panic in `coordinate` itself resumes after the
/// join.
pub fn run<J, O, W, R>(
    config: &Config,
    make: impl Fn(usize) -> W + Sync,
    execute: impl Fn(&mut W, J, &Worker<'_, J, O>) -> O + Sync,
    coordinate: impl FnOnce(&mut Coordinator<'_, J, O, W>) -> R,
) -> Result<R, PoolError>
where
    J: Job,
    O: Send,
{
    let workers = config.slots.workers();
    let shared = Shared {
        state: Mutex::new(State::new(config, workers.max(1))),
        work: Condvar::new(),
        outcomes: Condvar::new(),
        cancelled: AtomicBool::new(false),
        limits: limits(config),
        phases: config.phases,
        #[cfg(debug_assertions)]
        perturbation: config.perturbation.clone(),
    };
    let make: MakeSlot<'_, W> = &make;
    let execute: Execute<'_, J, O, W> = &execute;
    let result = if workers == 0 {
        let mut coordinator = Coordinator {
            shared: &shared,
            slots: config.slots,
            inline: Some(Inline {
                make,
                execute,
                state: None,
            }),
        };
        let result = coordinate(&mut coordinator);
        drop(coordinator);
        result
    } else {
        thread::scope(|scope| {
            let _guard = ShutdownGuard(&shared);
            for index in 0..workers {
                let mut builder =
                    thread::Builder::new().name(format!("light-cljs-worker-{}", index + 1));
                if let Some(size) = config.stack_size {
                    builder = builder.stack_size(size);
                }
                let shared = &shared;
                builder
                    .spawn_scoped(scope, move || work(shared, index, make, execute))
                    .map_err(PoolError::Spawn)?;
            }
            let mut coordinator = Coordinator {
                shared: &shared,
                slots: config.slots,
                inline: None,
            };
            Ok(coordinate(&mut coordinator))
        })?
    };
    let state = shared
        .state
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    match state.panic {
        Some(panic) => Err(PoolError::Panic(panic)),
        None => Ok(result),
    }
}

/// The pool's bounds: a perturbation's in place of the configured ones.
fn limits(config: &Config) -> Limits {
    #[cfg(debug_assertions)]
    if let Some(limits) = config.perturbation.as_ref().and_then(|p| p.limits.clone()) {
        return limits;
    }
    config.limits.clone()
}

#[cfg(debug_assertions)]
mod perturb;
#[cfg(debug_assertions)]
pub use perturb::Perturbation;

#[cfg(test)]
mod tests;
