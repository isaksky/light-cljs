//! Separate evaluation and explicit reclamation cost on a bounded cyclic workload.
use light_cljs::interpreter::{Context, MemoryStats, ValueView};
use std::{env, hint::black_box, process::Command, time::Instant};

#[cfg(feature = "count-allocations")]
#[path = "support/allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

const PROGRAM: &str =
    "(def current (let [a (atom nil)] (letfn [(f [] (if @a 42 (f)))] (reset! a f) f))) (current)";
const ITERATIONS: usize = 256;
const SAMPLES: usize = 7;

#[derive(Clone, Copy, Default)]
struct Cost {
    ns: u128,
    allocations: usize,
    reallocations: usize,
    bytes: usize,
}

impl Cost {
    fn add(&mut self, other: Self) {
        self.ns += other.ns;
        self.allocations += other.allocations;
        self.reallocations += other.reallocations;
        self.bytes += other.bytes;
    }
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Cost) {
    let start = Instant::now();
    #[cfg(feature = "count-allocations")]
    let (result, allocations, reallocations, bytes) = allocations::measure(f);
    #[cfg(not(feature = "count-allocations"))]
    let (result, allocations, reallocations, bytes) = (f(), 0, 0, 0);
    (
        result,
        Cost {
            ns: start.elapsed().as_nanos(),
            allocations,
            reallocations,
            bytes,
        },
    )
}

fn evaluate(c: &mut Context) {
    assert_eq!(
        black_box(c.eval_string(black_box(PROGRAM)).unwrap()).inspect(),
        ValueView::Integer(42)
    );
}

fn live(mut stats: MemoryStats) -> MemoryStats {
    stats.slots = 0;
    stats
}

fn sample(interval: usize) -> (Cost, Cost, MemoryStats) {
    let mut c = Context::new();
    c.eval_string("(ns-map 'clojure.core)").unwrap();
    // Fill then reuse every vacancy and the root/mark/sweep scratch capacities.
    for _ in 0..2 {
        for _ in 0..interval {
            evaluate(&mut c);
        }
        c.collect_garbage();
    }
    let expected = c.memory_stats();
    let mut evaluation = Cost::default();
    let mut collection = Cost::default();
    for n in 1..=ITERATIONS {
        let (_, cost) = measure(|| evaluate(&mut c));
        evaluation.add(cost);
        if n % interval == 0 {
            let (stats, cost) = measure(|| c.collect_garbage());
            collection.add(cost);
            assert_eq!(
                stats.after, expected,
                "live records and vacant capacity must stabilize"
            );
        }
    }
    #[cfg(feature = "count-allocations")]
    assert_eq!(
        (
            collection.allocations,
            collection.reallocations,
            collection.bytes
        ),
        (0, 0, 0),
        "warmed collection must reuse scratch instead of allocating per collection node"
    );
    (evaluation, collection, expected)
}

fn median(mut values: Vec<u128>) -> u128 {
    values.sort_unstable();
    values[values.len() / 2]
}

fn main() {
    if env::args().any(|arg| arg == "--fresh-process") {
        evaluate(&mut Context::new());
        return;
    }
    let mut fresh_context = Vec::new();
    let mut fresh_process = Vec::new();
    for _ in 0..SAMPLES {
        let start = Instant::now();
        evaluate(&mut Context::new());
        fresh_context.push(start.elapsed().as_nanos());
        let start = Instant::now();
        assert!(
            Command::new(env::current_exe().unwrap())
                .arg("--fresh-process")
                .status()
                .unwrap()
                .success()
        );
        fresh_process.push(start.elapsed().as_nanos());
    }
    println!(
        "samples={SAMPLES} evaluations/sample={ITERATIONS} fresh_context_ns={} fresh_process_ns={} instrumented={}",
        median(fresh_context),
        median(fresh_process),
        cfg!(feature = "count-allocations")
    );
    let mut equivalent = None;
    for interval in [1, 16, 256] {
        let samples: Vec<_> = (0..SAMPLES).map(|_| sample(interval)).collect();
        let stats = samples[0].2;
        if let Some(expected) = equivalent {
            assert_eq!(
                live(stats),
                expected,
                "collection cadence preserves retained records"
            );
        } else {
            equivalent = Some(live(stats));
        }
        println!(
            "collect_every={interval} eval_ns/op={} gc_ns/collection={} gc_ns/op={} eval_allocs/op={} eval_reallocs/op={} eval_bytes/op={} gc_allocs/sample={} gc_reallocs/sample={} gc_bytes/sample={} live={:?}",
            median(
                samples
                    .iter()
                    .map(|s| s.0.ns / ITERATIONS as u128)
                    .collect()
            ),
            median(
                samples
                    .iter()
                    .map(|s| s.1.ns / (ITERATIONS / interval) as u128)
                    .collect()
            ),
            median(
                samples
                    .iter()
                    .map(|s| s.1.ns / ITERATIONS as u128)
                    .collect()
            ),
            samples[0].0.allocations / ITERATIONS,
            samples[0].0.reallocations / ITERATIONS,
            samples[0].0.bytes / ITERATIONS,
            samples[0].1.allocations,
            samples[0].1.reallocations,
            samples[0].1.bytes,
            stats
        );
    }
}
