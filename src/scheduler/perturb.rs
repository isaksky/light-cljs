//! The test-only scheduling perturbation (docs/compiler-concurrency.md,
//! "As built (light-cljs-342)"). Debug builds only: release builds have
//! none of it.
//!
//! One seed drives everything: randomized delays at job boundaries, the
//! order and size of each claimed batch, the steal victim and how much it
//! gives, and, with [`Perturbation::fixed`], a forced assignment of every
//! job to one worker. Nothing in the scheduler decides output order, so
//! any seed must give the build of any other; a test that finds otherwise
//! prints the seed and the assignment it logged.
use super::{Entry, Job, Limits, State, batch_len};
use std::{
    sync::{Arc, Mutex, atomic::AtomicUsize},
    thread,
    time::Duration,
};

/// How a pool, and the compiler around it, is perturbed for a stress test.
#[derive(Clone, Debug, Default)]
pub struct Perturbation {
    pub seed: u64,
    /// Run each job only on the worker its [`Job::key`] and the seed choose:
    /// never claimed by another or stolen. With one worker, every job runs
    /// on it, in the seed's order.
    pub fixed: bool,
    /// Bounds in place of the pool's own, such as one-job batches and room
    /// for one admitted job, which keeps every worker saturated.
    pub limits: Option<Limits>,
    /// The compiler analyzes each pooled unit in a fresh isolate, the
    /// reference for persistent ones (as `LIGHT_CLJS_FRESH_UNIT_ISOLATES`).
    pub fresh_isolates: bool,
    /// The compiler panics when an engine asks for the compile-time code of
    /// this Var's unit (`app.core/f`): inside `__lightCompileUnit`, the
    /// callback a running macro's nested demand enters, on whichever
    /// thread's engine runs the macro.
    pub panic_in: Option<String>,
    /// Every job this pool started (the parse queue gets a log of its own),
    /// in start order: the deque index of the worker
    /// that ran it (0 on the inline path) and its key. A test prints it as
    /// the assignment that gave a result.
    pub log: Arc<Mutex<Vec<(usize, u64)>>>,
    /// The compile-time engines the build's pool workers made, whether the
    /// build succeeded or not: one per worker that ran a unit, and one
    /// more each time a failed engine was retired and its worker went on.
    pub isolates: Arc<AtomicUsize>,
}

/// Where a pause is taken.
#[derive(Clone, Copy)]
pub(super) enum Boundary {
    /// A worker claimed a job and is about to run it.
    Start = 1,
    /// A worker ran a job and is about to publish its outcome.
    Finish = 2,
    /// The coordinator is about to submit work.
    Submit = 3,
}

/// A deterministic 64-bit mix (SplitMix64's finalizer).
pub(super) fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

impl Perturbation {
    /// A perturbation by `seed` alone: delays, random batches and steals.
    pub fn seeded(seed: u64) -> Perturbation {
        Perturbation {
            seed,
            ..Default::default()
        }
    }

    /// The jobs' keys by worker, in the order each ran them.
    pub fn assignment(&self) -> Vec<(usize, Vec<u64>)> {
        let log = self.log.lock().unwrap_or_else(|e| e.into_inner());
        let mut workers: Vec<(usize, Vec<u64>)> = Vec::new();
        for &(worker, key) in log.iter() {
            match workers.iter_mut().find(|(w, _)| *w == worker) {
                Some((_, keys)) => keys.push(key),
                None => workers.push((worker, vec![key])),
            }
        }
        workers.sort_by_key(|(w, _)| *w);
        workers
    }

    /// The worker that runs job `key` under [`Self::fixed`], of `workers`.
    pub(super) fn owner(&self, key: u64, workers: usize) -> usize {
        (mix(self.seed ^ mix(key)) % workers as u64) as usize
    }

    /// Sleep, yield or go on at once, as the seed says for this job and
    /// boundary: most boundaries pass, some yield, a few sleep up to a
    /// millisecond, long enough for a peer to claim, steal or finish.
    pub(super) fn pause(&self, key: u64, worker: usize, boundary: Boundary) {
        let hash = mix(self.seed ^ mix(key ^ ((worker as u64) << 40) ^ ((boundary as u64) << 56)));
        match hash % 32 {
            0..=1 => thread::sleep(Duration::from_micros(200 + (hash >> 8) % 800)),
            2..=5 => thread::sleep(Duration::from_micros(20 + (hash >> 8) % 80)),
            6..=11 => thread::yield_now(),
            _ => {}
        }
    }
}

/// A perturbation's state behind the pool's mutex.
pub(super) struct Perturbed {
    pub(super) config: Perturbation,
    rng: u64,
}

impl Perturbed {
    pub(super) fn new(config: Perturbation) -> Perturbed {
        Perturbed {
            rng: mix(config.seed ^ 0x5EED),
            config,
        }
    }

    /// A number below `bound`, which is at least one.
    fn below(&mut self, bound: usize) -> usize {
        self.rng = mix(self.rng);
        (self.rng % bound.max(1) as u64) as usize
    }
}

impl<J: Job, O> State<J, O> {
    /// [`State::claim`] under a perturbation: urgent work first as always;
    /// then, under a fixed assignment, a random one of this worker's jobs
    /// or none; otherwise a random-sized batch from a random place in the
    /// shared queue, or from a random peer's deque.
    pub(super) fn claim_perturbed(&mut self, w: usize, limits: &Limits) -> Option<super::Claim<J>> {
        if let Some(entry) = self.urgent.pop_front() {
            self.runnable_total -= 1;
            self.counts.urgent += 1;
            return Some(self.start_logged(w, entry, 0));
        }
        if self.runnable_total == 0 {
            return None;
        }
        let fixed = self.perturbed.as_ref().is_some_and(|p| p.config.fixed);
        if fixed {
            return self.claim_assigned(w);
        }
        let phase = self.choose_phase(w, limits);
        if !self.local[w][phase].is_empty() {
            self.take_runnable(phase);
            self.counts.local += 1;
            let entry = self.local[w][phase].pop_back().expect("not empty");
            return Some(self.start_logged(w, entry, 0));
        }
        let taken = if !self.ready[phase].is_empty() {
            let len = self.ready[phase].len();
            let p = self.perturbed.as_mut().expect("perturbed");
            let at = p.below(len);
            let most = batch_len(self.ready[phase].iter().skip(at), limits);
            let count = 1 + self.perturbed.as_mut().expect("perturbed").below(most);
            let batch: Vec<_> = self.ready[phase].drain(at..at + count).collect();
            for entry in batch.into_iter().rev() {
                self.local[w][phase].push_back(entry);
            }
            self.counts.claims += 1;
            self.counts.claimed += count as u64;
            count
        } else {
            let victims: Vec<usize> = (0..self.local.len())
                .filter(|&v| v != w && !self.local[v][phase].is_empty())
                .collect();
            let p = self.perturbed.as_mut().expect("perturbed");
            let victim = victims[p.below(victims.len())];
            let half = self.local[victim][phase].len().div_ceil(2);
            let most = batch_len(self.local[victim][phase].iter().take(half), limits);
            let count = 1 + self.perturbed.as_mut().expect("perturbed").below(most);
            let taken: Vec<_> = self.local[victim][phase].drain(..count).collect();
            self.local[w][phase].extend(taken);
            self.counts.steals += 1;
            self.counts.stolen += count as u64;
            count
        };
        let entry = self.local[w][phase].pop_back().expect("a claim leaves one");
        self.take_runnable(phase);
        Some(self.start_logged(w, entry, taken - 1))
    }

    /// A random one of the runnable jobs assigned to worker `w`, from any
    /// phase, or none: nobody else may run them. Under a fixed assignment
    /// every job waits in the shared queues, local pushes included
    /// ([`super::Worker::push_local`]).
    fn claim_assigned(&mut self, w: usize) -> Option<super::Claim<J>> {
        let workers = self.local.len();
        let config = &self.perturbed.as_ref().expect("perturbed").config;
        let mut mine: Vec<(usize, usize)> = Vec::new();
        for phase in 0..self.phases() {
            for (i, entry) in self.ready[phase].iter().enumerate() {
                if config.owner(entry.key, workers) == w {
                    mine.push((phase, i));
                }
            }
        }
        if mine.is_empty() {
            return None;
        }
        let pick = self
            .perturbed
            .as_mut()
            .expect("perturbed")
            .below(mine.len());
        let (phase, i) = mine[pick];
        let entry = self.ready[phase].remove(i).expect("a listed job");
        self.take_runnable(phase);
        self.counts.claims += 1;
        self.counts.claimed += 1;
        Some(self.start_logged(w, entry, 0))
    }

    /// Whether a job pushed locally goes to the shared queue instead, to
    /// wait for its owner.
    pub(super) fn assigns(&self) -> bool {
        self.perturbed.as_ref().is_some_and(|p| p.config.fixed)
    }

    fn start_logged(&mut self, w: usize, entry: Entry<J>, left: usize) -> super::Claim<J> {
        if let Some(p) = &self.perturbed {
            (p.config.log.lock().unwrap_or_else(|e| e.into_inner())).push((w, entry.key));
        }
        super::Claim {
            entry: self.start(entry),
            left,
        }
    }
}
