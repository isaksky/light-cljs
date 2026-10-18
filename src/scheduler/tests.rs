//! The scheduler on synthetic jobs with injected delays: claim and steal
//! policy, admission, wakeups, aging, completion, panics and shutdown. No
//! engine is involved.

use super::*;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    },
    thread::ThreadId,
    time::{Duration, Instant},
};

/// Run `body` on its own thread and fail if it has not finished within
/// `seconds`: a deadlock fails the test instead of hanging the suite.
fn within<T: Send + 'static>(seconds: u64, body: impl FnOnce() -> T + Send + 'static) -> T {
    let (send, receive) = mpsc::channel();
    thread::spawn(move || {
        let _ = send.send(panic::catch_unwind(AssertUnwindSafe(body)));
    });
    match receive.recv_timeout(Duration::from_secs(seconds)) {
        Ok(Ok(value)) => value,
        Ok(Err(payload)) => panic::resume_unwind(payload),
        Err(_) => panic!("did not finish within {seconds} s: a deadlock or a missed wakeup"),
    }
}

/// A deterministic hash for delays, priorities and costs.
fn mix(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    x = (x ^ (x >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^ (x >> 31)
}

/// Sleep, yield or return at once, as the hash of `id` and `seed` says.
fn delay(id: u64, seed: u64) {
    match mix(id ^ seed.rotate_left(17)) % 16 {
        0 => thread::sleep(Duration::from_micros(50)),
        1..=2 => thread::sleep(Duration::from_micros(5)),
        3..=5 => thread::yield_now(),
        _ => {}
    }
}

#[derive(Debug)]
struct Task {
    id: u64,
    phase: usize,
    priority: u64,
    cost: u64,
    bytes: u64,
}

impl Job for Task {
    fn phase(&self) -> usize {
        self.phase
    }
    fn cost(&self) -> u64 {
        self.cost
    }
    fn bytes(&self) -> u64 {
        self.bytes
    }
    fn priority(&self) -> u64 {
        self.priority
    }
}

fn task(id: u64) -> Task {
    Task {
        id,
        phase: 0,
        priority: 0,
        cost: 1,
        bytes: 0,
    }
}

/// What a synthetic job reports.
#[derive(Debug)]
struct Ran {
    id: u64,
    slot: usize,
    thread: ThreadId,
}

/// A job graph: a `fanout`-ary tree over the ids below `total`, in
/// `phases` phases. A job pushes its odd-numbered children onto its own
/// deque; the coordinator submits the others when it consumes the outcome,
/// so both discovery paths and both queue kinds are exercised.
#[derive(Clone, Copy)]
struct Graph {
    total: u64,
    fanout: u64,
    phases: usize,
    seed: u64,
    bytes: u64,
}

impl Graph {
    fn node(&self, id: u64) -> Task {
        let hash = mix(id ^ self.seed);
        Task {
            id,
            phase: (hash % self.phases as u64) as usize,
            priority: (hash >> 8) % 4,
            cost: 1 + (hash >> 16) % 3,
            bytes: self.bytes,
        }
    }

    /// The children of `id`, with whether the job pushes each locally.
    fn children(&self, id: u64) -> impl Iterator<Item = (u64, bool)> + '_ {
        (1..=self.fanout)
            .map(move |k| (id * self.fanout + k, k % 2 == 1))
            .filter(|(child, _)| *child < self.total)
    }

    /// Run the whole graph on `config` and return each id's run count, the
    /// outcomes and the pool's counts.
    fn run(self, config: &Config) -> (Vec<u32>, Vec<Ran>, PoolCounts) {
        let graph = self;
        let result = run(
            config,
            |_slot| (),
            move |_: &mut (), task: Task, worker: &Worker<'_, Task, Ran>| {
                delay(task.id, graph.seed);
                for (child, local) in graph.children(task.id) {
                    if local {
                        worker.push_local(graph.node(child));
                    }
                }
                Ran {
                    id: task.id,
                    slot: worker.slot(),
                    thread: thread::current().id(),
                }
            },
            |coordinator| {
                let mut runs = vec![0u32; graph.total as usize];
                let mut outcomes = Vec::new();
                coordinator.submit(graph.node(0));
                loop {
                    match coordinator.next_outcome().expect("no job panics") {
                        Next::Outcome(ran) => {
                            runs[ran.id as usize] += 1;
                            // Successors are registered before the next
                            // completion test.
                            coordinator.submit_batch(
                                graph
                                    .children(ran.id)
                                    .filter(|(_, local)| !local)
                                    .map(|(child, _)| graph.node(child)),
                            );
                            outcomes.push(ran);
                        }
                        Next::Quiescent => break,
                        other => panic!("unexpected {other:?}"),
                    }
                }
                (runs, outcomes, coordinator.counts())
            },
        );
        result.expect("the pool runs")
    }
}

fn config(slots: usize, phases: usize) -> Config {
    Config::new(Slots::new(slots).unwrap(), phases)
}

#[test]
fn slots_count_the_coordinator() {
    assert_eq!(Slots::default(), Slots::ONE);
    assert_eq!(Slots::ONE.workers(), 0);
    assert_eq!(Slots::new(8).unwrap().workers(), 7);
    assert_eq!(Slots::new(0), None);
}

impl State<Task, u64> {
    fn submit_job(&mut self, limits: &Limits, task: Task) -> usize {
        let phases = self.phases();
        self.submit(limits, Entry::new(task, phases))
    }
}

fn state(deques: usize, phases: usize, limits: Limits) -> (State<Task, u64>, Limits) {
    let mut config = config(deques + 1, phases);
    config.limits = limits;
    (State::new(&config, deques), config.limits)
}

#[test]
fn claims_follow_the_design_order() {
    let limits = Limits {
        batch_jobs: 3,
        ..Limits::default()
    };
    let (mut s, limits) = state(2, 1, limits);
    for id in 0..8 {
        s.submit_job(&limits, task(id));
    }
    // A batch of three from the shared queue: start the first, leave two
    // in the claimer's deque with the next one at the back.
    let claim = s.claim(0, &limits).unwrap();
    assert_eq!((claim.entry.job.id, claim.left), (0, 2));
    assert_eq!(
        s.local[0][0].iter().map(|e| e.job.id).collect::<Vec<_>>(),
        [2, 1]
    );
    // Urgent work comes before the local deque.
    s.urgent.push_back(Entry::new(task(100), 1));
    s.count_admission(0);
    s.runnable_total += 1;
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 100);
    // Then the owner pops from the back.
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 1);
    // A second worker claims the next batch, 3 4 5.
    assert_eq!(s.claim(1, &limits).unwrap().entry.job.id, 3);
    // Worker 0 takes 2 locally, then batch 6 7.
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 2);
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 6);
    assert!(s.ready[0].is_empty());
    // Worker 1 has 5 4 left; it pops 4, leaving 5; worker 0 has 7.
    assert_eq!(s.claim(1, &limits).unwrap().entry.job.id, 4);
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 7);
    // Worker 0 is empty and the shared queue too: it steals 5 from worker 1.
    let claim = s.claim(0, &limits).unwrap();
    assert_eq!((claim.entry.job.id, claim.left), (5, 0));
    assert_eq!((s.counts.steals, s.counts.stolen), (1, 1));
    assert_eq!(s.runnable_total, 0);
    assert!(s.claim(0, &limits).is_none());
    assert_eq!(s.running, 9);
}

#[test]
fn steals_take_about_half_from_the_far_end_capped_by_the_batch_limits() {
    let limits = Limits {
        batch_jobs: 8,
        ..Limits::default()
    };
    let (mut s, limits) = state(3, 1, limits);
    for id in 0..8 {
        s.submit_job(&limits, task(id));
    }
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 0);
    // Worker 0 holds 7 6 5 4 3 2 1; worker 1 steals four from the front.
    let claim = s.claim(1, &limits).unwrap();
    assert_eq!((claim.entry.job.id, claim.left), (4, 3));
    assert_eq!(
        s.local[1][0].iter().map(|e| e.job.id).collect::<Vec<_>>(),
        [7, 6, 5]
    );
    assert_eq!(
        s.local[0][0].iter().map(|e| e.job.id).collect::<Vec<_>>(),
        [3, 2, 1]
    );
    // Worker 2's victim cursor starts at 0 and finds worker 0 first; with
    // a cost limit it takes fewer than half.
    let limits = Limits {
        batch_jobs: 8,
        batch_cost: 1,
        ..limits
    };
    let claim = s.claim(2, &limits).unwrap();
    assert_eq!((claim.entry.job.id, claim.left), (3, 0));
    // Next it rotates to worker 1.
    assert_eq!(s.claim(2, &limits).unwrap().entry.job.id, 7);
    assert_eq!(s.counts.steals, 3);
}

#[test]
fn batch_limits_count_jobs_and_estimated_cost() {
    let limits = Limits {
        batch_jobs: 10,
        batch_cost: 5,
        ..Limits::default()
    };
    let (mut s, limits) = state(1, 1, limits);
    for (id, cost) in [(0, 4), (1, 1), (2, 1), (3, 1)] {
        s.submit_job(&limits, Task { cost, ..task(id) });
    }
    // 4 + 1 fits the cost limit; a third job would not.
    assert_eq!(s.claim(0, &limits).unwrap().left, 1);
    let (mut s, limits) = state(1, 1, limits);
    s.submit_job(&limits, Task { cost: 9, ..task(0) });
    s.submit_job(&limits, task(1));
    // The first job is always taken, however costly.
    let claim = s.claim(0, &limits).unwrap();
    assert_eq!((claim.entry.job.id, claim.left), (0, 0));
}

#[test]
fn admission_bounds_jobs_and_bytes_and_admits_by_priority() {
    let limits = Limits {
        batch_jobs: 1,
        admitted_jobs: 2,
        admitted_bytes: 10,
        ..Limits::default()
    };
    let (mut s, limits) = state(1, 1, limits);
    let job = |id, bytes, priority| Task {
        bytes,
        priority,
        ..task(id)
    };
    assert_eq!(s.submit_job(&limits, job(0, 6, 0)), 1);
    // 6 + 6 is over the byte bound.
    assert_eq!(s.submit_job(&limits, job(1, 6, 0)), 0);
    // 6 + 1 fits, and fills the job bound.
    assert_eq!(s.submit_job(&limits, job(2, 1, 5)), 1);
    assert_eq!(s.submit_job(&limits, job(3, 0, 9)), 0);
    assert_eq!(
        (s.admitted_jobs, s.admitted_bytes, s.backlog_len),
        (2, 7, 2)
    );
    // The higher priority runs first.
    let claim = s.claim(0, &limits).unwrap();
    assert_eq!(claim.entry.job.id, 2);
    s.running -= 1;
    s.completed.push_back((2, claim.entry.bytes));
    // Consuming its outcome frees its room for the backlog's highest
    // priority, job 3; job 1 still waits.
    assert_eq!(s.consume(&limits).unwrap(), (2, 1));
    assert_eq!(
        (s.admitted_jobs, s.admitted_bytes, s.backlog_len),
        (2, 6, 1)
    );
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 3);
    assert_eq!(s.claim(0, &limits).unwrap().entry.job.id, 0);
    // A job larger than the bound is admitted when nothing else is.
    let (mut s, limits) = state(1, 1, limits);
    assert_eq!(s.submit_job(&limits, job(9, 1000, 0)), 1);
}

#[test]
fn a_spent_quantum_moves_to_the_phase_served_longest_ago() {
    let limits = Limits {
        batch_jobs: 1,
        phase_quantum: 2,
        ..Limits::default()
    };
    let (mut s, limits) = state(1, 3, limits);
    for id in 0..6 {
        s.submit_job(
            &limits,
            Task {
                phase: 0,
                ..task(id)
            },
        );
    }
    s.submit_job(
        &limits,
        Task {
            phase: 1,
            ..task(10)
        },
    );
    s.submit_job(
        &limits,
        Task {
            phase: 2,
            ..task(20)
        },
    );
    let order: Vec<_> = (0..8)
        .map(|_| s.claim(0, &limits).unwrap().entry.job.id)
        .collect();
    // Two from phase 0, then phase 1 and 2 (never served), then phase 0.
    assert_eq!(order, [0, 1, 10, 20, 2, 3, 4, 5]);
}

#[test]
fn urgent_jobs_run_before_queued_work() {
    let order = run(
        &config(1, 1),
        |_| (),
        |_, task: Task, _: &Worker<'_, Task, u64>| task.id,
        |coordinator| {
            coordinator.submit_batch((0..4).map(task));
            coordinator.submit_urgent(task(99));
            let mut order = Vec::new();
            while let Next::Outcome(id) = coordinator.next_outcome().unwrap() {
                order.push(id);
            }
            order
        },
    )
    .unwrap();
    assert_eq!(order, [99, 0, 1, 2, 3]);
}

#[test]
fn no_job_is_lost_or_duplicated_under_claim_and_steal_races() {
    within(120, || {
        for slots in [2, 3, 4, 8, 16] {
            for round in 0..4u64 {
                let graph = Graph {
                    total: 3000,
                    fanout: 3,
                    phases: 3,
                    seed: round * 31 + slots as u64,
                    bytes: 0,
                };
                let mut config = config(slots, 3);
                config.limits.batch_jobs = 1 + round as usize * 3;
                let (runs, outcomes, counts) = graph.run(&config);
                assert!(
                    runs.iter().all(|&n| n == 1),
                    "slots {slots} round {round}: runs {:?}",
                    runs.iter()
                        .enumerate()
                        .filter(|(_, n)| **n != 1)
                        .take(5)
                        .collect::<Vec<_>>()
                );
                assert_eq!(outcomes.len(), 3000);
                assert_eq!(counts.started, 3000);
                assert_eq!(
                    counts.urgent + counts.local + counts.claims + counts.steals,
                    3000,
                    "every job is started by exactly one path"
                );
                assert!(outcomes.iter().all(|ran| (1..slots).contains(&ran.slot)));
            }
        }
    });
}

#[test]
fn every_claimed_batch_member_stays_stealable() {
    let (slots_used, counts) = within(60, || {
        let mut config = config(8, 1);
        config.limits.batch_jobs = 64;
        run(
            &config,
            |_| (),
            |_, task: Task, worker: &Worker<'_, Task, (u64, usize)>| {
                thread::sleep(Duration::from_millis(2));
                (task.id, worker.slot())
            },
            |coordinator| {
                // One batch: the first claimer takes all 64 into its deque.
                coordinator.submit_batch((0..64).map(task));
                let mut slots = Vec::new();
                while let Next::Outcome((_, slot)) = coordinator.next_outcome().unwrap() {
                    slots.push(slot);
                }
                slots.sort_unstable();
                slots.dedup();
                (slots, coordinator.counts())
            },
        )
        .unwrap()
    });
    assert_eq!(counts.claims, 1, "{counts:?}");
    assert!(counts.steals > 0, "{counts:?}");
    assert!(slots_used.len() > 1, "only {slots_used:?} ran jobs");
    assert_eq!(counts.claimed, 64);
}

#[test]
fn a_job_enqueued_as_the_last_worker_parks_is_not_missed() {
    within(120, || {
        for slots in [2, 4] {
            run(
                &config(slots, 1),
                |_| (),
                |_, task: Task, _: &Worker<'_, Task, u64>| task.id,
                |coordinator| {
                    let workers = slots - 1;
                    for id in 0..2000 {
                        // Half the rounds race the submission against the
                        // worker parking after its last outcome; the other
                        // half submit only once every worker is parked.
                        if id % 2 == 0 {
                            while coordinator.parked_workers() < workers {
                                thread::yield_now();
                            }
                        }
                        coordinator.submit(task(id));
                        assert_eq!(coordinator.next_outcome().unwrap(), Next::Outcome(id));
                        assert_eq!(coordinator.next_outcome().unwrap(), Next::Quiescent);
                    }
                    assert!(coordinator.counts().parks >= 1000);
                },
            )
            .unwrap();
        }
    });
}

/// Phase 0 is an endless stream, each job pushing its successor onto its
/// own deque, until the coordinator has seen every phase 1 job finish. With
/// one job admitted at a time the stream's higher priority would also keep
/// phase 1 in the backlog, but for admission aging.
fn starve(slots: usize, admitted_jobs: usize) -> PoolCounts {
    let stop = Arc::new(AtomicBool::new(false));
    let stream = Arc::clone(&stop);
    let mut config = config(slots, 2);
    config.limits.phase_quantum = 8;
    config.limits.admitted_jobs = admitted_jobs;
    run(
        &config,
        |_| (),
        move |_, task: Task, worker: &Worker<'_, Task, Task>| {
            if task.phase == 0 && !stream.load(Ordering::Relaxed) {
                worker.push_local(Task {
                    id: task.id + 1,
                    priority: 10,
                    ..task
                });
            }
            task
        },
        |coordinator| {
            // More streams than workers, so every worker is kept busy.
            coordinator.submit_batch((0..slots as u64).map(|s| Task {
                priority: 10,
                ..task(s * 1_000_000)
            }));
            coordinator.submit_batch((0..20).map(|id| Task {
                phase: 1,
                ..task(id)
            }));
            let mut starved = 20;
            loop {
                match coordinator.next_outcome().unwrap() {
                    Next::Outcome(task) if task.phase == 1 => {
                        starved -= 1;
                        if starved == 0 {
                            stop.store(true, Ordering::Relaxed);
                        }
                    }
                    Next::Outcome(_) => {}
                    Next::Quiescent => break,
                    other => panic!("unexpected {other:?}"),
                }
            }
            assert_eq!(starved, 0);
            coordinator.counts()
        },
    )
    .unwrap()
}

#[test]
fn phase_aging_keeps_a_starved_phase_progressing() {
    let counts = within(60, || starve(1, 4096));
    assert!(counts.phase_switches > 0, "{counts:?}");
    for (slots, admitted) in [(2, 4096), (3, 4096), (8, 4096), (1, 1), (2, 1), (8, 2)] {
        within(60, move || starve(slots, admitted));
    }
}

/// Worker slot state that counts how many were made and dropped.
struct Counted(Arc<AtomicUsize>);

impl Drop for Counted {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}

fn panicking_build(slots: usize) -> (Result<usize, PoolError>, usize, usize, usize) {
    let made = Arc::new(AtomicUsize::new(0));
    let dropped = Arc::new(AtomicUsize::new(0));
    let ran = Arc::new(AtomicUsize::new(0));
    let (m, d, r) = (made.clone(), dropped.clone(), ran.clone());
    let result = run(
        &config(slots, 1),
        move |_| {
            m.fetch_add(1, Ordering::SeqCst);
            Counted(d.clone())
        },
        move |_, task: Task, _: &Worker<'_, Task, u64>| {
            delay(task.id, 7);
            r.fetch_add(1, Ordering::SeqCst);
            if task.id == 137 {
                panic!("job {} failed", task.id);
            }
            task.id
        },
        |coordinator| {
            coordinator.submit_batch((0..2000).map(task));
            let mut seen = 0;
            loop {
                match coordinator.next_outcome() {
                    Ok(Next::Outcome(_)) => seen += 1,
                    Ok(next) => panic!("the build ended without the panic: {next:?}"),
                    Err(panic) => {
                        assert_eq!(panic.message, "job 137 failed");
                        assert_eq!(coordinator.next_outcome().unwrap_err(), panic);
                        return seen;
                    }
                }
            }
        },
    );
    (
        result,
        made.load(Ordering::SeqCst),
        dropped.load(Ordering::SeqCst),
        ran.load(Ordering::SeqCst),
    )
}

#[test]
fn a_panicking_job_stops_the_build_and_joins_every_thread() {
    for slots in [1, 2, 8, 16] {
        let (result, made, dropped, ran) = within(60, move || panicking_build(slots));
        let Err(PoolError::Panic(panic)) = result else {
            panic!("slots {slots}: {result:?}");
        };
        assert_eq!(panic.message, "job 137 failed");
        if slots == 1 {
            assert_eq!(panic.slot, 0);
        } else {
            assert!((1..slots).contains(&panic.slot));
        }
        // Every slot state was dropped: the threads have ended.
        assert!(made >= 1);
        assert_eq!(made, dropped, "slots {slots}");
        assert!(
            ran < 2000,
            "slots {slots}: the build went on after the panic"
        );
    }
}

#[test]
fn a_panicking_slot_constructor_is_a_worker_panic() {
    let result = within(30, || {
        run(
            &config(4, 1),
            |slot| -> () { panic!("slot {slot} cannot start") },
            |_, task: Task, _: &Worker<'_, Task, u64>| task.id,
            |coordinator| {
                coordinator.submit(task(0));
                coordinator.next_outcome().map(|_| ())
            },
        )
    });
    let Err(PoolError::Panic(panic)) = result else {
        panic!("{result:?}");
    };
    assert!(panic.message.ends_with("cannot start"), "{panic}");
}

#[test]
fn coordinator_shutdown_while_workers_are_parked() {
    for slots in [2, 8, 16] {
        let made = within(30, move || {
            let made = Arc::new(AtomicUsize::new(0));
            let m = made.clone();
            run(
                &config(slots, 2),
                move |_| {
                    m.fetch_add(1, Ordering::SeqCst);
                },
                |_, task: Task, _: &Worker<'_, Task, u64>| task.id,
                |coordinator| {
                    coordinator.submit_batch((0..50).map(task));
                    // Leave outcomes unconsumed and return once every worker
                    // waits: the scope must wake them to join.
                    while coordinator.parked_workers() < slots - 1 {
                        thread::yield_now();
                    }
                },
            )
            .unwrap();
            made.load(Ordering::SeqCst)
        });
        assert!(made < slots);
    }
}

#[test]
fn a_failing_coordinator_still_shuts_the_pool_down() {
    let caught = within(30, || {
        panic::catch_unwind(|| {
            run(
                &config(8, 1),
                |_| (),
                |_, task: Task, _: &Worker<'_, Task, u64>| task.id,
                |coordinator| -> () {
                    coordinator.submit_batch((0..20).map(task));
                    while coordinator.parked_workers() < 7 {
                        thread::yield_now();
                    }
                    panic!("coordinator failed");
                },
            )
        })
    });
    let payload = caught.expect_err("the coordinator's panic resumes after the join");
    assert_eq!(panic_message(payload.as_ref()), "coordinator failed");
}

#[test]
fn tiny_capacities_with_saturated_workers_do_not_deadlock() {
    within(120, || {
        for (slots, jobs, bytes) in [(2, 1, 1), (8, 1, 1), (8, 2, 3), (16, 3, 1)] {
            let graph = Graph {
                total: 1500,
                fanout: 4,
                phases: 2,
                seed: slots as u64,
                bytes: 1,
            };
            let mut config = config(slots, 2);
            config.limits = Limits {
                batch_jobs: 2,
                batch_cost: 2,
                admitted_jobs: jobs,
                admitted_bytes: bytes,
                phase_quantum: 2,
            };
            let (runs, _, counts) = graph.run(&config);
            assert!(runs.iter().all(|&n| n == 1));
            assert!(counts.peak_admitted <= jobs, "{counts:?}");
        }
    });
}

#[test]
fn one_slot_runs_the_same_job_graph_inline() {
    let graph = Graph {
        total: 2000,
        fanout: 3,
        phases: 3,
        seed: 5,
        bytes: 0,
    };
    let caller = thread::current().id();
    let (inline, outcomes, counts) = graph.run(&config(1, 3));
    assert!(inline.iter().all(|&n| n == 1));
    assert!(
        outcomes
            .iter()
            .all(|ran| ran.slot == 0 && ran.thread == caller)
    );
    assert_eq!(counts.parks, 0);
    let (pooled, outcomes, _) = graph.run(&config(4, 3));
    assert_eq!(inline, pooled);
    assert!(outcomes.iter().all(|ran| ran.thread != caller));
}

#[test]
fn empty_queues_while_a_job_discovers_work_do_not_end_the_build() {
    for slots in [1, 2, 8] {
        let ids = within(30, move || {
            run(
                &config(slots, 1),
                |_| (),
                |_, task: Task, _: &Worker<'_, Task, u64>| {
                    // While this runs every queue is empty.
                    thread::sleep(Duration::from_millis(if task.id == 0 { 30 } else { 1 }));
                    task.id
                },
                |coordinator| {
                    // A chain: each outcome discovers the next job.
                    coordinator.submit(task(0));
                    let mut ids = Vec::new();
                    loop {
                        match coordinator.next_outcome().unwrap() {
                            Next::Outcome(id) => {
                                ids.push(id);
                                if id < 20 {
                                    coordinator.submit(task(id + 1));
                                }
                            }
                            Next::Quiescent => return ids,
                            other => panic!("{other:?}"),
                        }
                    }
                },
            )
            .unwrap()
        });
        assert_eq!(ids, (0..=20).collect::<Vec<_>>());
    }
}

#[test]
fn held_work_without_a_producer_is_reported_as_stalled() {
    for slots in [1, 4] {
        run(
            &config(slots, 1),
            |_| (),
            |_, task: Task, _: &Worker<'_, Task, u64>| task.id,
            |coordinator| {
                coordinator.hold(Hold::Demand);
                coordinator.submit(task(1));
                assert_eq!(coordinator.next_outcome().unwrap(), Next::Outcome(1));
                assert_eq!(
                    coordinator.next_outcome().unwrap(),
                    Next::Stalled {
                        demands: 1,
                        publications: 0
                    }
                );
                coordinator.hold(Hold::Publication);
                coordinator.release(Hold::Demand);
                assert_eq!(
                    coordinator.next_outcome().unwrap(),
                    Next::Stalled {
                        demands: 0,
                        publications: 1
                    }
                );
                coordinator.release(Hold::Publication);
                assert_eq!(coordinator.next_outcome().unwrap(), Next::Quiescent);
            },
        )
        .unwrap();
    }
}

#[test]
fn cancellation_drops_unstarted_jobs_and_finishes_running_ones() {
    for slots in [1, 4] {
        let (finished, counts) = within(30, move || {
            run(
                &config(slots, 1),
                |_| (),
                |_, task: Task, worker: &Worker<'_, Task, u64>| {
                    let begun = Instant::now();
                    while task.id == 0 && !worker.cancelled() {
                        assert!(begun.elapsed() < Duration::from_secs(20));
                        thread::yield_now();
                    }
                    task.id
                },
                |coordinator| {
                    coordinator.submit_batch((0..100).map(task));
                    let mut finished = 0;
                    if slots == 1 {
                        // Inline, job 0 would spin: cancel before it runs.
                        coordinator.cancel();
                        coordinator.submit(task(1000));
                    }
                    loop {
                        match coordinator.next_outcome().unwrap() {
                            Next::Outcome(id) => {
                                finished += 1;
                                if id == 50 {
                                    coordinator.cancel();
                                    coordinator.submit(task(1000));
                                }
                            }
                            Next::Quiescent => break,
                            other => panic!("{other:?}"),
                        }
                    }
                    (finished, coordinator.counts())
                },
            )
            .unwrap()
        });
        assert!(finished < 101, "slots {slots}");
        assert_eq!(counts.started as usize, finished);
        assert_eq!(counts.cancelled as usize, 100 - finished + 1);
    }
}

/// A job that runs until the build is cancelled or stopping, after
/// marking `started`.
fn poll_until_cancelled(started: &AtomicBool, worker: &Worker<'_, Task, u64>) {
    started.store(true, Ordering::SeqCst);
    let begun = Instant::now();
    while !worker.cancelled() {
        assert!(begun.elapsed() < Duration::from_secs(20), "never cancelled");
        thread::yield_now();
    }
}

#[test]
fn a_worker_panic_cancels_running_jobs() {
    let result = within(30, || {
        let started = Arc::new(AtomicBool::new(false));
        let polled = started.clone();
        run(
            &config(3, 1),
            |_| (),
            move |_, task: Task, worker: &Worker<'_, Task, u64>| {
                if task.id == 0 {
                    poll_until_cancelled(&polled, worker);
                } else {
                    while !polled.load(Ordering::SeqCst) {
                        thread::yield_now();
                    }
                    panic!("job {} failed", task.id);
                }
                task.id
            },
            |coordinator| {
                coordinator.submit(task(0));
                coordinator.submit(task(1));
                // The panic arrives while job 0 still runs; returning must
                // not wait for it forever.
                coordinator.next_outcome().unwrap_err()
            },
        )
    });
    let Err(PoolError::Panic(panic)) = result else {
        panic!("{result:?}");
    };
    assert_eq!(panic.message, "job 1 failed");
}

#[test]
fn an_ending_or_failing_coordinator_cancels_running_jobs() {
    for fail in [false, true] {
        let caught = within(30, move || {
            panic::catch_unwind(move || {
                let started = Arc::new(AtomicBool::new(false));
                let polled = started.clone();
                run(
                    &config(2, 1),
                    |_| (),
                    move |_, task: Task, worker: &Worker<'_, Task, u64>| {
                        poll_until_cancelled(&polled, worker);
                        task.id
                    },
                    |coordinator| {
                        coordinator.submit(task(0));
                        while !started.load(Ordering::SeqCst) {
                            thread::yield_now();
                        }
                        assert!(!fail, "coordinator failed");
                    },
                )
            })
        });
        match caught {
            Ok(result) => assert!(!fail && result.is_ok()),
            Err(payload) => {
                assert!(fail);
                assert_eq!(panic_message(payload.as_ref()), "coordinator failed");
            }
        }
    }
}

#[test]
fn backlog_admission_ages_phases_behind_a_higher_priority() {
    let limits = Limits {
        batch_jobs: 1,
        admitted_jobs: 1,
        phase_quantum: 3,
        ..Limits::default()
    };
    let (mut s, limits) = state(1, 2, limits);
    s.submit_job(
        &limits,
        Task {
            priority: 10,
            ..task(0)
        },
    );
    s.submit_job(
        &limits,
        Task {
            phase: 1,
            ..task(100)
        },
    );
    let mut order = Vec::new();
    for _ in 0..6 {
        let claim = s.claim(0, &limits).unwrap();
        let id = claim.entry.job.id;
        order.push(id);
        // Each phase 0 job queues a higher-priority successor first.
        if id < 100 {
            s.submit_job(
                &limits,
                Task {
                    priority: 10,
                    ..task(id + 1)
                },
            );
        }
        s.running -= 1;
        s.completed.push_back((id, 0));
        s.consume(&limits).unwrap();
    }
    // Phase 1 waited while three stream jobs were admitted, then went
    // ahead of the stream.
    assert_eq!(order, [0, 1, 2, 3, 100, 4]);
    assert_eq!(s.counts.peak_backlog, 2);
}
