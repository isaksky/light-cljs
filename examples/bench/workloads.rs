//! The workloads of the measurement harness (pellet clj-quickjs-15,
//! `docs/design.md` §15), shared by `cargo run --release --example bench`
//! (which times them and adds allocator counts) and `tests/measurement.rs`
//! (which runs them small and asserts only their results and the
//! structural memory facts, never times or byte counts).
//!
//! Every workload is host glue on the public embedding API. The JavaScript
//! is the synthetic fixtures next to this file plus `examples/e2e/work.js`
//! (not light-cljs output), compiled once by [`Fixtures::compile`] outside
//! every measured interval. Each workload separates setup (building inputs,
//! completing cells, spawning) from the interval it measures, and checks
//! every task's result against a value computed in Rust, so no measured work
//! can be skipped or discarded.

// Shared by the example and the integration test; each uses a different
// part.
#![allow(dead_code)]

use std::hint::black_box;
use std::time::{Duration, Instant};

use clj_quickjs::exec::{EventKind, ExecutorConfig, OutcomeKind};
use clj_quickjs::frontend::compile_function;
use clj_quickjs::{
    Completion, DependencyId, DependencyKey, Executable, HeapUsage, Need, Primitive, ResultKind,
    Rooted, RunOutcome, Runtime, Task, TaskStats, TaskStatus, ValueView,
};

/// The result kind every fixture needs (`__rt.need(def, 1)`).
pub const KIND: ResultKind = ResultKind::new(1);

/// Bound of every wait on the executor; a workload that does not finish in
/// time returns an error instead of hanging.
pub const TIMEOUT: Duration = Duration::from_secs(60);

/// The compiled fixtures: shared immutable code, compiled once.
pub struct Fixtures {
    pub identity: Executable,
    pub needs: Executable,
    pub baseline: Executable,
    pub vec_nth: Executable,
    pub map_get: Executable,
    pub vec_conj: Executable,
    pub map_assoc: Executable,
    pub consumer: Executable,
    pub work: Executable,
}

impl Fixtures {
    pub fn compile() -> Fixtures {
        let c = |name: &str, src: &str| {
            compile_function(src).unwrap_or_else(|e| panic!("fixture {name}: {e}"))
        };
        Fixtures {
            identity: c("identity.js", include_str!("identity.js")),
            needs: c("needs.js", include_str!("needs.js")),
            baseline: c("loop.js", include_str!("loop.js")),
            vec_nth: c("vec_nth.js", include_str!("vec_nth.js")),
            map_get: c("map_get.js", include_str!("map_get.js")),
            vec_conj: c("vec_conj.js", include_str!("vec_conj.js")),
            map_assoc: c("map_assoc.js", include_str!("map_assoc.js")),
            consumer: c("consumer.js", include_str!("consumer.js")),
            work: c("work.js", include_str!("../e2e/work.js")),
        }
    }
}

/// A rooted number.
pub fn num(rt: &Runtime, n: f64) -> Rooted {
    rt.root(Primitive::Number(n))
}

/// The number a rooted value holds; panics otherwise.
pub fn number(r: &Rooted) -> f64 {
    match r.view() {
        ValueView::Number(n) => n,
        other => panic!("expected a number, got {other:?}"),
    }
}

/// Run `task` on this thread until it completes; panics on any other
/// outcome (the workloads that suspend drive their tasks themselves).
pub fn finish(task: &Task) -> Rooted {
    match task.run().expect("task runs") {
        RunOutcome::Complete(v) => v,
        other => panic!("expected completion, got {other:?}"),
    }
}

/// The key of result [`KIND`] of `def` in the runtime's initial world.
pub fn key(rt: &Runtime, def: u64) -> DependencyKey {
    DependencyKey::new(rt.world(), DependencyId::new(def), KIND)
}

/// Complete `def`'s cell with `value` as its claimed producer.
pub fn publish(rt: &Runtime, def: u64, value: &Rooted) -> clj_quickjs::Wakeups {
    let key = key(rt, def);
    let request = rt.claim(key).expect("cell is unclaimed");
    rt.complete(key, request.producer(), Completion::Ready(value))
        .expect("completion accepted")
}

/// `sum(i % c for i in 0..n)`: the result of `loop.js`, `vec_nth.js` and
/// `map_get.js` over a collection whose element `i` is `i`.
pub fn cyclic_sum(c: u64, n: u64) -> f64 {
    let (full, rest) = (n / c, n % c);
    (full * c * (c - 1) / 2 + rest * rest.saturating_sub(1) / 2) as f64
}

// ---------------------------------------------------------------------------
// Task startup.

/// `n` tasks of `identity.js`: spawning them, then running each to
/// completion on this thread.
pub struct Startup {
    pub tasks: usize,
    pub spawn: Duration,
    pub run: Duration,
}

pub fn startup(rt: &Runtime, fx: &Fixtures, n: usize) -> Startup {
    let arg = num(rt, 7.0);
    let mut tasks = Vec::with_capacity(n);
    let t0 = Instant::now();
    for _ in 0..n {
        tasks.push(rt.spawn(&fx.identity, &[&arg]).expect("spawn"));
    }
    let spawn = t0.elapsed();
    let t1 = Instant::now();
    let mut sum = 0.0;
    for task in &tasks {
        sum += number(&finish(task));
    }
    let run = t1.elapsed();
    assert_eq!(black_box(sum), 7.0 * n as f64);
    Startup {
        tasks: n,
        spawn,
        run,
    }
}

// ---------------------------------------------------------------------------
// Already-ready dependency reads.

/// One task of `needs.js` reading a completed cell `n` times, and one task
/// of `loop.js` with the same trip count (the loop's own cost). Only the
/// runs are timed; the cell is completed and the tasks spawned before.
pub struct ReadyReads {
    pub reads: u64,
    pub needs: Duration,
    pub baseline: Duration,
}

/// `def` must be a definition whose cell is not used elsewhere in `rt`.
pub fn ready_reads(rt: &Runtime, fx: &Fixtures, def: u64, n: u64) -> ReadyReads {
    let one = num(rt, 1.0);
    let wakeups = publish(rt, def, &one);
    assert!(wakeups.task_ids().is_empty());
    let task = rt
        .spawn(
            &fx.needs,
            &[&num(rt, def as f64), &num(rt, 0.0), &num(rt, n as f64)],
        )
        .expect("spawn");
    let base = rt
        .spawn(&fx.baseline, &[&num(rt, 1e15), &num(rt, n as f64)])
        .expect("spawn");

    let t0 = Instant::now();
    let read = finish(&task);
    let needs = t0.elapsed();
    let t1 = Instant::now();
    let looped = finish(&base);
    let baseline = t1.elapsed();

    assert_eq!(number(&read), n as f64);
    assert_eq!(number(&looped), cyclic_sum(1_000_000_000_000_000, n));
    assert_eq!(task.dependency_log().edges(), &[key(rt, def)]);
    ReadyReads {
        reads: n,
        needs,
        baseline,
    }
}

/// `n` ready reads of a completed cell from Rust through
/// [`clj_quickjs::RunGuard::need`] (no VM dispatch; each read returns a new
/// persistent root, dropped at once). `def` as for [`ready_reads`].
pub fn rust_ready_reads(rt: &Runtime, fx: &Fixtures, def: u64, n: u64) -> Duration {
    let one = num(rt, 1.0);
    let _ = publish(rt, def, &one);
    let task = rt.spawn(&fx.identity, &[&one]).expect("spawn");
    let mut guard = task.try_begin_run().expect("claim");
    let mut sum = 0.0;
    let t0 = Instant::now();
    for _ in 0..n {
        match guard.need(DependencyId::new(def), KIND) {
            Ok(Need::Ready(v)) => sum += number(&v),
            other => panic!("expected a ready read, got {other:?}"),
        }
    }
    let elapsed = t0.elapsed();
    assert_eq!(black_box(sum), n as f64);
    match guard.run() {
        RunOutcome::Complete(_) => {}
        other => panic!("expected completion, got {other:?}"),
    }
    elapsed
}

// ---------------------------------------------------------------------------
// Suspension and wake-up.

/// One task of `needs.js` needing `n` distinct pending cells in turn, driven
/// on this thread: each cycle resumes the task, which needs the next cell
/// and parks (`Waiting`); the host then completes that cell, which delivers
/// the wake. The two phases are timed separately per cycle.
pub struct ParkCycles {
    pub cycles: u64,
    /// Resume + run to the next need + subscribe + park, summed.
    pub resume_to_park: Duration,
    /// Claimed producer taken, cell completed, wake delivered, summed.
    pub complete_and_wake: Duration,
    /// Whole driver loop, including the final run to completion.
    pub total: Duration,
}

/// Uses definitions `first .. first + n` of `rt`'s initial world.
pub fn park_cycles(rt: &Runtime, fx: &Fixtures, first: u64, n: u64) -> ParkCycles {
    let one = num(rt, 1.0);
    let task = rt
        .spawn(
            &fx.needs,
            &[&num(rt, first as f64), &num(rt, 1.0), &num(rt, n as f64)],
        )
        .expect("spawn");
    let mut resume_to_park = Duration::ZERO;
    let mut complete_and_wake = Duration::ZERO;
    let mut cycles = 0;
    let start = Instant::now();
    let result = loop {
        let t0 = Instant::now();
        let outcome = task.run().expect("task runs");
        let t1 = Instant::now();
        resume_to_park += t1 - t0;
        match outcome {
            RunOutcome::Waiting(key) => {
                // The task's own need claimed the producer request.
                let request = task.take_producer_requests().pop().expect("claimed");
                let wakeups = rt
                    .complete(key, request.producer(), Completion::Ready(&one))
                    .expect("completion accepted");
                let runnable = wakeups.into_runnable();
                complete_and_wake += t1.elapsed();
                assert_eq!(runnable.len(), 1, "the parked task is woken once");
                cycles += 1;
            }
            RunOutcome::Complete(v) => break v,
            other => panic!("unexpected outcome {other:?}"),
        }
    };
    let total = start.elapsed();
    assert_eq!(cycles, n);
    assert_eq!(number(&result), n as f64);
    ParkCycles {
        cycles,
        resume_to_park,
        complete_and_wake,
        total,
    }
}

// ---------------------------------------------------------------------------
// Native persistent operations.

/// Lookups and updates of persistent collections from JavaScript: `ops`
/// reads cycling over a vector and a map of `size` elements (built before
/// timing by the update fixtures), `loop.js` with the same trip count, and
/// `updates` appends and associations from empty (fewer than `ops`: every
/// intermediate version stays allocated until the collection after them).
pub struct Persistent {
    pub size: u64,
    pub ops: u64,
    pub updates: u64,
    pub vec_nth: Duration,
    pub map_get: Duration,
    pub vec_conj: Duration,
    pub map_assoc: Duration,
    pub baseline: Duration,
    /// Heap objects the two update runs left behind: every intermediate
    /// version stays allocated until a collection is requested (the
    /// collector runs only on request).
    pub update_objects: usize,
    /// Objects the collection after the update runs reclaimed.
    pub update_reclaimed: usize,
}

pub fn persistent(rt: &Runtime, fx: &Fixtures, size: u64, ops: u64, updates: u64) -> Persistent {
    let size_arg = num(rt, size as f64);
    let ops_arg = num(rt, ops as f64);
    let updates_arg = num(rt, updates as f64);
    // Setup: the collections the lookups read.
    let vector = finish(&rt.spawn(&fx.vec_conj, &[&size_arg]).expect("spawn"));
    let map = finish(&rt.spawn(&fx.map_assoc, &[&size_arg]).expect("spawn"));

    let timed = |exe: &Executable, args: &[&Rooted]| {
        let task = rt.spawn(exe, args).expect("spawn");
        let t0 = Instant::now();
        let result = finish(&task);
        (t0.elapsed(), result)
    };
    let expected = cyclic_sum(size, ops);
    let (vec_nth, r) = timed(&fx.vec_nth, &[&vector, &ops_arg]);
    assert_eq!(number(&r), expected);
    let (map_get, r) = timed(&fx.map_get, &[&map, &ops_arg]);
    assert_eq!(number(&r), expected);
    let (baseline, r) = timed(&fx.baseline, &[&size_arg, &ops_arg]);
    assert_eq!(number(&r), expected);
    let before_updates = rt.heap_stats().objects;
    let (vec_conj, r) = timed(&fx.vec_conj, &[&updates_arg]);
    rt.scope(|s| {
        let v = s.local(&r).expect("live");
        assert_eq!(s.count(v).expect("a vector"), updates as usize);
    });
    let (map_assoc, r) = timed(&fx.map_assoc, &[&updates_arg]);
    rt.scope(|s| {
        let m = s.local(&r).expect("live");
        assert_eq!(s.count(m).expect("a map"), updates as usize);
    });
    let update_objects = rt.heap_stats().objects - before_updates;
    let update_reclaimed = rt.collect().expect("no mutator is active").reclaimed;
    Persistent {
        size,
        ops,
        updates,
        vec_nth,
        map_get,
        vec_conj,
        map_assoc,
        baseline,
        update_objects,
        update_reclaimed,
    }
}

/// Net bytes and allocations of `ops` appends (`vec_conj.js`) and `ops`
/// associations (`map_assoc.js`) from empty, measured by `probe` across
/// each run (every intermediate version is still allocated at its end), and
/// across the collection after both (negative: given back).
pub fn update_bytes(fx: &Fixtures, ops: u64, probe: &dyn Fn() -> Probe) -> [Probe; 3] {
    let rt = Runtime::new();
    let ops_arg = num(&rt, ops as f64);
    let conj = rt.spawn(&fx.vec_conj, &[&ops_arg]).expect("spawn");
    let assoc = rt.spawn(&fx.map_assoc, &[&ops_arg]).expect("spawn");
    let p0 = probe();
    let v = finish(&conj);
    let p1 = probe();
    let m = finish(&assoc);
    let p2 = probe();
    drop((v, m, conj, assoc));
    rt.collect().expect("no mutator is active");
    let p3 = probe();
    [p1 - p0, p2 - p1, p3 - p2]
}

// ---------------------------------------------------------------------------
// Shared input and retained memory versus consumer count.

/// Bytes (from the harness's counting allocator, or 0 without one) and
/// allocation count at one moment.
#[derive(Clone, Copy, Debug, Default)]
pub struct Probe {
    pub bytes: i64,
    pub allocs: u64,
}

impl std::ops::Sub for Probe {
    type Output = Probe;
    fn sub(self, rhs: Probe) -> Probe {
        Probe {
            bytes: self.bytes - rhs.bytes,
            allocs: self.allocs - rhs.allocs,
        }
    }
}

/// The input form: a vector of `width` entries, each
/// `[i, {0: i, 1: 2i}, [i, i + 1, i + 2]]`. Three heap objects per entry
/// plus the outer vector.
pub fn build_input(rt: &Runtime, width: usize) -> Rooted {
    rt.scope(|s| {
        let n = |x: usize| s.primitive(Primitive::Number(x as f64));
        let entries: Vec<_> = (0..width)
            .map(|i| {
                let map = s.new_map(&[(n(0), n(i)), (n(1), n(2 * i))]).expect("map");
                let vec = s.new_vector(&[n(i), n(i + 1), n(i + 2)]).expect("vector");
                s.new_vector(&[n(i), map, vec]).expect("vector")
            })
            .collect();
        s.persist(s.new_vector(&entries).expect("vector"))
    })
}

/// Heap objects of [`build_input`].
pub fn input_objects(width: usize) -> usize {
    1 + 3 * width
}

/// Which consumer fixture a [`Memory`] measurement parks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConsumerKind {
    /// `consumer.js`: parks at a need in its only frame, holding a
    /// transient builder.
    Frame,
    /// `consumer_fold.js`: parks inside the callback of an `__rt.reduce`
    /// over the input, holding a native continuation and a callback frame.
    NativeFold,
}

impl ConsumerKind {
    pub fn name(self) -> &'static str {
        match self {
            ConsumerKind::Frame => "consumer.js",
            ConsumerKind::NativeFold => "consumer_fold.js",
        }
    }

    fn source(self) -> &'static str {
        match self {
            ConsumerKind::Frame => include_str!("consumer.js"),
            ConsumerKind::NativeFold => include_str!("consumer_fold.js"),
        }
    }
}

/// Memory of `consumers` tasks of one consumer fixture sharing one input
/// form and one executable, each parked at its need. Every `Probe` field is
/// a difference of the probe across one phase.
pub struct Memory {
    pub kind: ConsumerKind,
    pub consumers: usize,
    pub width: usize,
    /// Building and rooting the input form.
    pub input: Probe,
    pub input_objects: usize,
    /// Compiling the consumer fixture (shared code).
    pub code: Probe,
    /// Host-owned storage the harness allocates for the task handles (a
    /// `Vec<Task>` of this many bytes), allocated before `spawned`.
    pub host_handles_bytes: usize,
    /// Spawning every consumer.
    pub spawned: Probe,
    /// Running every consumer to its park (after `spawned`).
    pub parked: Probe,
    /// Heap objects while every consumer is parked.
    pub objects_parked: usize,
    pub usage_parked: HeapUsage,
    /// The first consumer's snapshot while parked.
    pub task_parked: TaskStats,
    /// Consumers that were `Waiting` before the publication.
    pub waiting: usize,
    /// Consumers whose result is the input's entry with its identity.
    pub identical: usize,
    /// Heap objects after dropping the tasks and results and collecting,
    /// while the input stays rooted.
    pub objects_after_drop: usize,
    /// Probe change from before spawning to after that collection: runtime
    /// storage the tasks did not give back (object-table and registry
    /// capacity). Host storage (the handle vector, verification roots) is
    /// allocated before and kept until after this interval, so it is
    /// excluded.
    pub retained_after_drop: Probe,
}

/// Measure [`Memory`] in a fresh runtime. `probe` reads the harness's
/// allocator counters (tests pass one returning zeros). The consumer fixture
/// is compiled inside the measured code phase so its bytes are counted.
pub fn memory(
    kind: ConsumerKind,
    consumers: usize,
    width: usize,
    probe: &dyn Fn() -> Probe,
) -> Memory {
    const DEF: u64 = 1;
    let rt = Runtime::new();
    let def = num(&rt, DEF as f64);
    let one = num(&rt, 1.0);
    let indices: Vec<Rooted> = (0..consumers)
        .map(|i| num(&rt, (i % width) as f64))
        .collect();

    let p0 = probe();
    let form = build_input(&rt, width);
    let p1 = probe();
    let input_objects = rt.heap_stats().objects;
    let code =
        compile_function(kind.source()).unwrap_or_else(|e| panic!("fixture {}: {e}", kind.name()));
    let p2 = probe();

    // Host storage, allocated before the task phases and kept until after
    // the retention probe: the handle vector, the roots the results are
    // checked against, and a free root slot for each result as it is taken.
    let mut tasks: Vec<Task> = Vec::with_capacity(consumers);
    let host_handles_bytes = tasks.capacity() * size_of::<Task>();
    let expected: Vec<Rooted> = rt.scope(|s| {
        let f = s.local(&form).expect("live");
        (0..width)
            .map(|i| s.persist(s.nth(f, i).expect("entry")))
            .collect()
    });
    drop(num(&rt, 0.0));
    let p3 = probe();
    for index in &indices {
        tasks.push(rt.spawn(&code, &[&form, &def, index]).expect("spawn"));
    }
    let p4 = probe();
    for task in &tasks {
        match task.run().expect("task runs") {
            RunOutcome::Waiting(_) => {}
            other => panic!("consumer should wait, got {other:?}"),
        }
    }
    let p5 = probe();
    let objects_parked = rt.heap_stats().objects;
    let usage_parked = rt.heap_usage();
    let task_parked = tasks[0].stats().expect("parked task has VM state");
    let waiting = tasks
        .iter()
        .filter(|t| t.status() == TaskStatus::Waiting)
        .count();
    // The first need claimed the producer; the host publishes as it.
    let request = tasks[0].take_producer_requests().pop().expect("claimed");
    let wakeups = rt
        .complete(key(&rt, DEF), request.producer(), Completion::Ready(&one))
        .expect("completion accepted");
    assert_eq!(wakeups.task_ids().len(), consumers);
    drop((wakeups, request));

    let mut identical = 0;
    for (i, task) in tasks.iter().enumerate() {
        let result = finish(task);
        identical += usize::from(result.same_object(&expected[i % width]));
    }
    // Drop the task handles but keep the vector's allocation (host storage).
    tasks.clear();
    rt.collect().expect("no mutator is active");
    let objects_after_drop = rt.heap_stats().objects;
    let p6 = probe();
    black_box((&form, &expected, &tasks));
    Memory {
        kind,
        consumers,
        width,
        input: p1 - p0,
        input_objects,
        code: p2 - p1,
        host_handles_bytes,
        spawned: p4 - p3,
        parked: p5 - p4,
        objects_parked,
        usage_parked,
        task_parked,
        waiting,
        identical,
        objects_after_drop,
        retained_after_drop: p6 - p3,
    }
}

// ---------------------------------------------------------------------------
// Useful work while consumers wait.

/// The reference executor with `workers` workers runs `work_tasks` tasks of
/// `work.js` twice: alone (baseline), then while `consumers` tasks of
/// `needs.js` are parked on one pending cell. Then the host publishes and
/// every consumer completes.
pub struct WorkWhileWaiting {
    pub workers: usize,
    pub consumers: usize,
    pub work_tasks: usize,
    pub squares: u64,
    /// Submitting the work tasks until the last result is taken, no
    /// consumers present.
    pub baseline: Duration,
    /// The same with every consumer parked.
    pub while_waiting: Duration,
    /// Consumers still `Waiting` when the work finished (none ran).
    pub still_waiting: usize,
    /// From the completion (wakes handed to the executor) until the last
    /// consumer's result is taken.
    pub wake_all: Duration,
}

/// Runs in a fresh runtime; the consumers need definition 1.
pub fn work_while_waiting(
    workers: usize,
    consumers: usize,
    work_tasks: usize,
    squares: u64,
) -> Result<WorkWhileWaiting, String> {
    const DEF: u64 = 1;
    let rt = Runtime::new();
    let fx_work = compile_function(include_str!("../e2e/work.js")).expect("work.js");
    let fx_needs = compile_function(include_str!("needs.js")).expect("needs.js");
    let exec = ExecutorConfig::new()
        .workers(workers)
        .start(&rt)
        .map_err(|e| format!("executor: {e}"))?;
    let n = num(&rt, squares as f64);
    let expected = (squares * (squares + 1) * (2 * squares + 1) / 6) as f64;

    let run_work = |label: &str| -> Result<Duration, String> {
        let tasks: Vec<Task> = (0..work_tasks)
            .map(|_| rt.spawn(&fx_work, &[&n]).expect("spawn"))
            .collect();
        let ids: Vec<_> = tasks.iter().map(Task::id).collect();
        let t0 = Instant::now();
        for task in tasks {
            exec.submit(task).map_err(|_| "foreign task".to_string())?;
        }
        for id in ids {
            match exec.take_outcome(id, TIMEOUT) {
                Some(RunOutcome::Complete(v)) if number(&v) == expected => {}
                other => return Err(format!("{label} work task {id}: {other:?}")),
            }
        }
        Ok(t0.elapsed())
    };

    let baseline = run_work("baseline")?;

    // The host claims the cell so no consumer's need does.
    let key = key(&rt, DEF);
    let request = rt.claim(key).map_err(|e| format!("claim: {e:?}"))?;
    let args = [&num(&rt, DEF as f64), &num(&rt, 0.0), &num(&rt, 1.0)];
    let waiters: Vec<Task> = (0..consumers)
        .map(|_| rt.spawn(&fx_needs, &args).expect("spawn"))
        .collect();
    for task in &waiters {
        exec.submit(task.clone())
            .map_err(|_| "foreign task".to_string())?;
    }
    let parked = |events: &[clj_quickjs::exec::Event]| {
        events
            .iter()
            .filter(|e| matches!(e.kind, EventKind::Released(OutcomeKind::Waiting(k)) if k == key))
            .count()
            >= consumers
    };
    if !exec.wait_for(TIMEOUT, parked) {
        return Err("consumers did not all park".into());
    }

    let while_waiting = run_work("while waiting")?;
    let still_waiting = waiters
        .iter()
        .filter(|t| t.status() == TaskStatus::Waiting)
        .count();

    let one = num(&rt, 1.0);
    let t0 = Instant::now();
    let wakeups = rt
        .complete(key, request.producer(), Completion::Ready(&one))
        .map_err(|e| format!("complete: {e:?}"))?;
    exec.wake(wakeups).map_err(|_| "foreign task".to_string())?;
    for task in &waiters {
        match exec.take_outcome(task.id(), TIMEOUT) {
            Some(RunOutcome::Complete(v)) if number(&v) == 1.0 => {}
            other => return Err(format!("consumer {}: {other:?}", task.id())),
        }
    }
    let wake_all = t0.elapsed();
    exec.shutdown(TIMEOUT)
        .map_err(|_| "executor did not shut down".to_string())?;
    Ok(WorkWhileWaiting {
        workers,
        consumers,
        work_tasks,
        squares,
        baseline,
        while_waiting,
        still_waiting,
        wake_all,
    })
}
