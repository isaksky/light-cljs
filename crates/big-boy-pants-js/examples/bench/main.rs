//! Reproducible cost measurements of the standalone runtime (pellet
//! clj-quickjs-15, `docs/design.md` §15). Std only; no benchmark framework.
//!
//! ```sh
//! cargo run --release --example bench              # full run (about a minute when the machine is quiet)
//! cargo run --release --example bench -- --quick   # small sizes, 3 rounds
//! cargo run --release --example bench -- memory work --workers 2 --rounds 5
//! ```
//!
//! Row groups (positional filters; default all): `startup`, `reads`,
//! `park`, `persistent`, `memory`, `work`, `overhead`, `strings`. The workloads and
//! what each measures are in `workloads.rs`; how to read the output, and
//! representative results with their environment, are in
//! `docs/measurement.md`.
//!
//! **Method.** Each timing row runs one warmup round (discarded) and then
//! `--rounds` measured rounds, each in a fresh [`Runtime`] built outside
//! the timed interval; it prints the median and the min..max over the
//! measured rounds, per operation. Setup (compilation, input building,
//! spawning where not the subject, completing cells) is outside every timed
//! interval, and every result is checked against a value computed in Rust.
//! Times are wall-clock ([`Instant`]) on a machine other processes may be
//! using: the load average is printed before and after, and the min..max
//! spread is the variance to read. Nothing here is a pass/fail threshold.
//!
//! **Memory.** A counting global allocator (below) tracks net bytes and
//! allocation counts while counting is switched on (memory rows only). It
//! measures requested bytes, not allocator overhead or RSS; RSS and its
//! high-water mark are read from `/proc/self/status` where available.

mod workloads;

use std::alloc::{GlobalAlloc, Layout, System};
use std::process::ExitCode;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering::Relaxed};
use std::time::{Duration, Instant};

use big_boy_pants_js::Runtime;
use workloads::{ConsumerKind, Fixtures, Probe};

// ---------------------------------------------------------------------------
// Counting allocator.

/// Forwards to the system allocator and, while [`COUNTING`] is set, adds
/// each allocation's requested size to [`LIVE`] (and subtracts frees).
/// Relaxed atomics: the counters are totals, read by the measuring thread
/// after the phase it measures, which ran on that same thread.
struct CountingAlloc;

static COUNTING: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicI64 = AtomicI64::new(0);
static ALLOCS: AtomicU64 = AtomicU64::new(0);

#[global_allocator]
static GLOBAL: CountingAlloc = CountingAlloc;

fn count(delta: i64, new_allocation: bool) {
    if COUNTING.load(Relaxed) {
        LIVE.fetch_add(delta, Relaxed);
        if new_allocation {
            ALLOCS.fetch_add(1, Relaxed);
        }
    }
}

// SAFETY: every method forwards its arguments unchanged to `System`, which
// implements `GlobalAlloc` correctly; the counting touches only atomics and
// never allocates, so it cannot recurse into the allocator.
unsafe impl GlobalAlloc for CountingAlloc {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        // SAFETY: the caller upholds `GlobalAlloc::alloc`'s contract for
        // `layout`, which is all `System.alloc` requires.
        let p = unsafe { System.alloc(layout) };
        if !p.is_null() {
            count(layout.size() as i64, true);
        }
        p
    }

    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        // SAFETY: as for `alloc`.
        let p = unsafe { System.alloc_zeroed(layout) };
        if !p.is_null() {
            count(layout.size() as i64, true);
        }
        p
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        // SAFETY: the caller guarantees `ptr` was allocated by this
        // allocator (hence by `System`) with `layout`.
        unsafe { System.dealloc(ptr, layout) };
        count(-(layout.size() as i64), false);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        // SAFETY: the caller guarantees `ptr`/`layout` came from this
        // allocator (hence from `System`) and that `new_size` is valid.
        let p = unsafe { System.realloc(ptr, layout, new_size) };
        if !p.is_null() {
            count(new_size as i64 - layout.size() as i64, true);
        }
        p
    }
}

fn probe() -> Probe {
    Probe {
        bytes: LIVE.load(Relaxed),
        allocs: ALLOCS.load(Relaxed),
    }
}

// ---------------------------------------------------------------------------
// Options and environment.

struct Options {
    quick: bool,
    rounds: usize,
    workers: usize,
    groups: Vec<String>,
}

const GROUPS: [&str; 8] = [
    "startup",
    "reads",
    "park",
    "persistent",
    "memory",
    "work",
    "overhead",
    "strings",
];

fn options() -> Result<Options, String> {
    let mut o = Options {
        quick: false,
        rounds: 0,
        workers: 2,
        groups: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        let mut value = |name: &str| -> Result<usize, String> {
            args.next()
                .and_then(|v| v.parse().ok())
                .filter(|&v| v > 0)
                .ok_or(format!("{name} needs a positive integer"))
        };
        match a.as_str() {
            "--quick" => o.quick = true,
            "--rounds" => o.rounds = value("--rounds")?,
            "--workers" => o.workers = value("--workers")?,
            g if GROUPS.contains(&g) => o.groups.push(g.to_string()),
            other => {
                return Err(format!(
                    "unknown argument {other:?}; usage: bench [--quick] [--rounds N] \
                     [--workers N] [{}]...",
                    GROUPS.join("|")
                ));
            }
        }
    }
    if o.rounds == 0 {
        o.rounds = if o.quick { 3 } else { 5 };
    }
    Ok(o)
}

impl Options {
    fn wants(&self, group: &str) -> bool {
        self.groups.is_empty() || self.groups.iter().any(|g| g == group)
    }

    /// `full` normally, `quick` with `--quick`.
    fn size<T>(&self, full: T, quick: T) -> T {
        if self.quick { quick } else { full }
    }
}

fn proc_field(path: &str, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .find(|l| l.starts_with(key))
        .and_then(|l| l.split_once(':'))
        .map(|(_, v)| v.trim().to_string())
}

fn load_average() -> String {
    std::fs::read_to_string("/proc/loadavg")
        .map(|s| s.split_whitespace().take(3).collect::<Vec<_>>().join(" "))
        .unwrap_or_else(|_| "n/a".into())
}

fn rss() -> String {
    match (
        proc_field("/proc/self/status", "VmRSS"),
        proc_field("/proc/self/status", "VmHWM"),
    ) {
        (Some(rss), Some(hwm)) => format!("RSS {rss}, high-water {hwm}"),
        _ => "RSS n/a".into(),
    }
}

fn environment(o: &Options) {
    println!("big-boy-pants-js measurement harness (examples/bench)");
    println!(
        "  build profile: {}",
        if cfg!(debug_assertions) {
            "debug (debug_assertions on): not representative; use --release"
        } else {
            "release (debug_assertions off)"
        }
    );
    println!(
        "  target: {} {}; available parallelism: {}; cpu: {}",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, |n| n.get()),
        proc_field("/proc/cpuinfo", "model name").unwrap_or_else(|| "n/a".into()),
    );
    println!(
        "  mode: {}; rounds: 1 warmup + {} measured per row; executor workers: {}",
        if o.quick { "quick" } else { "full" },
        o.rounds,
        o.workers
    );
    println!("  load average at start: {}", load_average());
    println!("  {}", rss());
    println!(
        "  times: median [min .. max] over measured rounds, wall clock, single embedder \
         thread unless a row says workers"
    );
}

// ---------------------------------------------------------------------------
// Rounds and rows.

/// Run `f` once as warmup (result discarded), then `--rounds` times, and
/// return the measured rounds' results.
fn rounds<T>(o: &Options, mut f: impl FnMut() -> T) -> Vec<T> {
    std::hint::black_box(f());
    (0..o.rounds).map(|_| f()).collect()
}

/// One column of figures out of measured rounds.
fn col<T>(rs: &[T], f: impl Fn(&T) -> f64) -> Vec<f64> {
    rs.iter().map(f).collect()
}

fn summary(mut xs: Vec<f64>) -> (f64, f64, f64) {
    xs.sort_by(f64::total_cmp);
    let median = if xs.len() % 2 == 1 {
        xs[xs.len() / 2]
    } else {
        (xs[xs.len() / 2 - 1] + xs[xs.len() / 2]) / 2.0
    };
    (median, xs[0], xs[xs.len() - 1])
}

fn row(name: &str, param: &str, xs: Vec<f64>, unit: &str) {
    let (median, min, max) = summary(xs);
    println!("  {name:<30} {param:<22} {median:>10.1} [{min:.1} .. {max:.1}] {unit}");
}

fn per(d: Duration, n: u64) -> f64 {
    d.as_nanos() as f64 / n as f64
}

fn header(title: &str) {
    println!("\n{title}");
}

// ---------------------------------------------------------------------------
// Groups.

fn startup(o: &Options, fx: &Fixtures) {
    header("task startup: spawn N tasks of identity.js, then run each to completion");
    let n = o.size(20_000, 2_000);
    let rs = rounds(o, || workloads::startup(&Runtime::new(), fx, n));
    let param = format!("n={n}");
    let n = n as u64;
    row(
        "startup.spawn",
        &param,
        col(&rs, |r| per(r.spawn, n)),
        "ns/task",
    );
    row(
        "startup.first_run_to_complete",
        &param,
        col(&rs, |r| per(r.run, n)),
        "ns/task",
    );
}

fn reads(o: &Options, fx: &Fixtures) {
    header("already-ready dependency reads (cell completed before the timed run)");
    let n = o.size(1_000_000, 100_000);
    let rs = rounds(o, || {
        let rt = Runtime::new();
        let js = workloads::ready_reads(&rt, fx, 1, n);
        let rust = workloads::rust_ready_reads(&rt, fx, 2, n);
        (js, rust)
    });
    let param = format!("reads={n}");
    let needs = |r: &(workloads::ReadyReads, Duration)| per(r.0.needs, n);
    let base = |r: &(workloads::ReadyReads, Duration)| per(r.0.baseline, n);
    row(
        "reads.js_need_loop",
        &param,
        col(&rs, needs),
        "ns/iteration",
    );
    row(
        "reads.js_baseline_loop",
        &param,
        col(&rs, base),
        "ns/iteration",
    );
    row(
        "reads.js_need_minus_baseline",
        &param,
        col(&rs, |r| needs(r) - base(r)),
        "ns/read",
    );
    row(
        "reads.rust_runguard_need",
        &param,
        col(&rs, |r| per(r.1, n)),
        "ns/read",
    );
}

fn park(o: &Options, fx: &Fixtures) {
    header("suspension and wake-up: one task needs N pending cells in turn, host completes each");
    let n = o.size(100_000, 10_000);
    let rs = rounds(o, || workloads::park_cycles(&Runtime::new(), fx, 1, n));
    let param = format!("cycles={n}");
    row(
        "park.full_cycle",
        &param,
        col(&rs, |r| per(r.total, r.cycles)),
        "ns/cycle",
    );
    row(
        "park.resume_need_park",
        &param,
        col(&rs, |r| per(r.resume_to_park, r.cycles)),
        "ns/cycle",
    );
    row(
        "park.complete_and_wake",
        &param,
        col(&rs, |r| per(r.complete_and_wake, r.cycles)),
        "ns/cycle",
    );
}

fn persistent(o: &Options, fx: &Fixtures) {
    header("native persistent operations from JavaScript (per loop iteration)");
    let ops = o.size(1_000_000, 100_000);
    // Updates keep every intermediate version until a collection, so they
    // run fewer times than lookups to bound the process's memory.
    let updates = o.size(100_000, 20_000);
    let sizes: &[u64] = if o.quick {
        &[1_024]
    } else {
        &[32, 1_024, 32_768]
    };
    for &size in sizes {
        let rs = rounds(o, || {
            workloads::persistent(&Runtime::new(), fx, size, ops, updates)
        });
        let base = |r: &workloads::Persistent| per(r.baseline, ops);
        let param = format!("size={size} ops={ops}");
        let nth = |r: &workloads::Persistent| per(r.vec_nth, ops);
        let get = |r: &workloads::Persistent| per(r.map_get, ops);
        row(
            "persistent.vec_nth_loop",
            &param,
            col(&rs, nth),
            "ns/iteration",
        );
        row(
            "persistent.vec_nth_minus_baseline",
            &param,
            col(&rs, |r| nth(r) - base(r)),
            "ns/op",
        );
        row(
            "persistent.map_get_loop",
            &param,
            col(&rs, get),
            "ns/iteration",
        );
        row(
            "persistent.map_get_minus_baseline",
            &param,
            col(&rs, |r| get(r) - base(r)),
            "ns/op",
        );
        row(
            "persistent.baseline_loop",
            &param,
            col(&rs, base),
            "ns/iteration",
        );
        // The update rows build from empty: the same work at every size.
        if size == sizes[0] {
            let param = format!("from empty, ops={updates}");
            row(
                "persistent.vec_conj_loop",
                &param,
                col(&rs, |r| per(r.vec_conj, updates)),
                "ns/iteration",
            );
            row(
                "persistent.map_assoc_loop",
                &param,
                col(&rs, |r| per(r.map_assoc, updates)),
                "ns/iteration",
            );
            row(
                "persistent.update_garbage_objects",
                &param,
                col(&rs, |r| r.update_objects as f64),
                "objects left until a collection",
            );
            row(
                "persistent.update_reclaimed",
                &param,
                col(&rs, |r| r.update_reclaimed as f64),
                "objects reclaimed by it",
            );
        }
    }
    // One counted pass (allocator counting on; not timed).
    LIVE.store(0, Relaxed);
    COUNTING.store(true, Relaxed);
    let [conj, assoc, collected] = workloads::update_bytes(fx, updates, &probe);
    COUNTING.store(false, Relaxed);
    println!(
        "  counted pass, ops={updates} from empty: vec_conj {:.1} B/op ({:.2} allocs/op), \
         map_assoc {:.1} B/op ({:.2} allocs/op) still allocated at the end of each run; \
         the collection after both changed allocated bytes by {}",
        conj.bytes as f64 / updates as f64,
        conj.allocs as f64 / updates as f64,
        assoc.bytes as f64 / updates as f64,
        assoc.allocs as f64 / updates as f64,
        collected.bytes,
    );
    println!("  {}", rss());
}

fn memory(o: &Options) {
    header(
        "memory: N consumers parked on one pending cell, sharing one input form and one \
         executable (counting allocator; deterministic, one round per row)",
    );
    let width = 1_000;
    let counts: &[usize] = if o.quick {
        &[1, 10, 100]
    } else {
        &[1, 10, 100, 1_000, 10_000]
    };
    for kind in [ConsumerKind::Frame, ConsumerKind::NativeFold] {
        memory_table(kind, width, counts);
    }
    println!("  {}", rss());
}

fn memory_table(kind: ConsumerKind, width: usize, counts: &[usize]) {
    println!(
        "  {}: {}",
        kind.name(),
        match kind {
            ConsumerKind::Frame => "parks at a need in its only frame, holding a builder",
            ConsumerKind::NativeFold =>
                "parks in a __rt.reduce callback: a native continuation plus a callback frame",
        }
    );
    println!(
        "  {:>9} {:>11} {:>8} {:>10} {:>11} {:>11} {:>9} {:>9} {:>10} {:>9} {:>11} {:>10} {:>9}",
        "consumers",
        "input_B",
        "input_ob",
        "code_B",
        "spawn_B/t",
        "park_B/t",
        "allocs/t",
        "heap_ob",
        "heap_ob/t",
        "vm_est_B",
        "identical",
        "after_drop",
        "kept_B/t"
    );
    for &n in counts {
        LIVE.store(0, Relaxed);
        COUNTING.store(true, Relaxed);
        let m = workloads::memory(kind, n, width, &probe);
        COUNTING.store(false, Relaxed);
        let per_task = |p: Probe| p.bytes as f64 / n as f64;
        println!(
            "  {:>9} {:>11} {:>8} {:>10} {:>11.1} {:>11.1} {:>9.2} {:>9} {:>10.2} {:>9} {:>11} {:>10} {:>9.1}",
            n,
            m.input.bytes,
            m.input_objects,
            m.code.bytes,
            per_task(m.spawned),
            per_task(m.parked),
            (m.spawned.allocs + m.parked.allocs) as f64 / n as f64,
            m.objects_parked,
            (m.objects_parked - m.input_objects) as f64 / n as f64,
            m.task_parked.shallow_bytes,
            format!("{}/{}", m.identical, n),
            m.objects_after_drop,
            per_task(m.retained_after_drop),
        );
        if n == *counts.last().unwrap() {
            let u = m.usage_parked;
            println!(
                "  at {n} consumers: heap objects by kind {:?}; known-shareable {}; table slots \
                 {} ({} free); shallow table+object bytes {} (excludes trie bodies, code, task \
                 state); host-owned Vec<Task> {} B (excluded above); first consumer {:?}",
                u.objects,
                u.known_shareable,
                u.slots,
                u.free_slots,
                u.shallow_bytes(),
                m.host_handles_bytes,
                m.task_parked,
            );
        }
        assert_eq!(m.waiting, n, "every consumer parked");
    }
}

fn work(o: &Options) -> Result<(), String> {
    header(&format!(
        "useful work while consumers wait: {} executor workers run work.js tasks alone, then \
         while consumers are parked on one pending cell",
        o.workers
    ));
    let consumers = o.size(1_000, 100);
    let work_tasks = o.size(64, 16);
    let squares = 2_000;
    let rs = rounds(o, || {
        workloads::work_while_waiting(o.workers, consumers, work_tasks, squares)
    })
    .into_iter()
    .collect::<Result<Vec<_>, _>>()?;
    let us_per_task = |d: Duration| per(d, work_tasks as u64) / 1_000.0;
    let param = format!("tasks={work_tasks} n={squares}");
    row(
        "work.alone",
        &param,
        col(&rs, |r| us_per_task(r.baseline)),
        "us/work task (wall / tasks)",
    );
    let param = format!("same, +{consumers} parked");
    row(
        "work.while_consumers_parked",
        &param,
        col(&rs, |r| us_per_task(r.while_waiting)),
        "us/work task (wall / tasks)",
    );
    row(
        "work.consumers_still_parked",
        &param,
        col(&rs, |r| r.still_waiting as f64),
        "consumers after the work",
    );
    row(
        "work.publish_to_all_resumed",
        &param,
        col(&rs, |r| per(r.wake_all, consumers as u64)),
        "ns/consumer (wall / consumers)",
    );
    Ok(())
}

fn overhead(o: &Options, fx: &Fixtures) {
    header("instrumentation overhead");
    let n = o.size(20_000, 2_000);
    let startup = |counting: bool| {
        COUNTING.store(counting, Relaxed);
        let xs = rounds(o, || {
            let r = workloads::startup(&Runtime::new(), fx, n);
            per(r.spawn + r.run, n as u64)
        });
        COUNTING.store(false, Relaxed);
        xs
    };
    let param = format!("n={n}");
    row(
        "overhead.startup_counting_off",
        &param,
        startup(false),
        "ns/task",
    );
    row(
        "overhead.startup_counting_on",
        &param,
        startup(true),
        "ns/task",
    );

    let instants = rounds(o, || {
        let k = 1_000_000u64;
        let t0 = Instant::now();
        for _ in 0..k {
            std::hint::black_box(Instant::now());
        }
        per(t0.elapsed(), k)
    });
    row("overhead.instant_now", "", instants, "ns/call");

    // Snapshot costs over the memory rows' input form.
    let rt = Runtime::new();
    let form = workloads::build_input(&rt, 1_000);
    let one = workloads::num(&rt, 1.0);
    let task = rt.spawn(&fx.consumer, &[&form, &one, &one]).expect("spawn");
    let usage = rounds(o, || {
        let k = 1_000u64;
        let t0 = Instant::now();
        for _ in 0..k {
            std::hint::black_box(rt.heap_usage());
        }
        per(t0.elapsed(), k)
    });
    let param = format!("{} objects", rt.heap_stats().objects);
    row("overhead.heap_usage_snapshot", &param, usage, "ns/call");
    let stats = rounds(o, || {
        let k = 100_000u64;
        let t0 = Instant::now();
        for _ in 0..k {
            std::hint::black_box(task.stats());
        }
        per(t0.elapsed(), k)
    });
    row(
        "overhead.task_stats_snapshot",
        "unstarted task",
        stats,
        "ns/call",
    );
}

fn main() -> ExitCode {
    let o = match options() {
        Ok(o) => o,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::FAILURE;
        }
    };
    environment(&o);
    let fx = Fixtures::compile();
    if o.wants("strings") {
        strings(&o);
    }
    if o.wants("startup") {
        startup(&o, &fx);
    }
    if o.wants("reads") {
        reads(&o, &fx);
    }
    if o.wants("park") {
        park(&o, &fx);
    }
    if o.wants("persistent") {
        persistent(&o, &fx);
    }
    if o.wants("memory") {
        memory(&o);
    }
    if o.wants("work")
        && let Err(e) = work(&o)
    {
        eprintln!("work rows failed: {e}");
        return ExitCode::FAILURE;
    }
    if o.wants("overhead") {
        overhead(&o, &fx);
    }
    println!("\n  load average at end: {}", load_average());
    ExitCode::SUCCESS
}

fn strings(o: &Options) {
    header("string concatenation, first flatten, then indexed reads");
    let build = big_boy_pants_js::frontend::compile_function(workloads::STRING_CONCAT).unwrap();
    let read = big_boy_pants_js::frontend::compile_function(workloads::STRING_READ).unwrap();
    for n in if o.quick {
        [1_000, 2_000, 10_000]
    } else {
        [10_000, 20_000, 100_000]
    } {
        for (name, direction) in [("append", 1.0), ("prepend", -1.0)] {
            let rs = rounds(o, || workloads::string_concat(&build, &read, n, direction));
            let param = format!("{name} n={n}");
            for (stage, times) in [
                ("build", col(&rs, |r| per(r.build, n as u64))),
                ("flatten", col(&rs, |r| per(r.flatten, n as u64))),
                ("read", col(&rs, |r| per(r.read, n as u64))),
            ] {
                row(&format!("strings.{stage}"), &param, times, "ns/unit");
            }
        }
    }
}
