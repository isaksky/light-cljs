//! Raw release samples. The Python driver saves environment, binaries and summaries.
use light_cljs::{
    interpreter::{Context, Form, LoadRequest, MemoryStats, Name, SourceInput, ValueView},
    parser::parse,
};
#[cfg(not(feature = "count-allocations"))]
use std::time::Instant;
use std::{env, error::Error, hint::black_box};

#[cfg(feature = "count-allocations")]
#[path = "support/interpreter_allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

const FIXTURES: &[(&str, &str, i64)] = &[
    (
        "closures_loop",
        include_str!("../benchmarks/fixtures/interpreter/closures_loop.clj"),
        134400,
    ),
    (
        "collections",
        include_str!("../benchmarks/fixtures/interpreter/collections.clj"),
        2763520,
    ),
    (
        "lazy",
        include_str!("../benchmarks/fixtures/interpreter/lazy.clj"),
        2796160,
    ),
    (
        "into_lazy",
        "(reduce + (into [] (map inc (range 128))))",
        8256,
    ),
    (
        "into_transducer",
        "(reduce + (into [] (fn [rf] (fn ([a] (rf a)) ([a x] (rf a (inc x))))) (range 128)))",
        8256,
    ),
    (
        "bounded_lazy",
        "(reduce + (into [] (take 64 (map inc (filter odd? (range 256))))))",
        4160,
    ),
    (
        "bounded_transducer",
        "(reduce + (into [] (comp (filter odd?) (map inc) (take 64)) (range 256)))",
        4160,
    ),
    (
        "bounded_eager",
        "(reduce + (into [] (take 64 (mapv inc (filterv odd? (range 256))))))",
        4160,
    ),
    (
        "mapcat_lazy_complete",
        "(reduce + (mapcat (fn [x] [x x]) (range 128)))",
        16256,
    ),
    (
        "mapcat_xf_complete",
        "(transduce (mapcat (fn [x] [x x])) (completing +) 0 (range 128))",
        16256,
    ),
    (
        "cat_lazy_complete",
        "(reduce + (apply concat (map (fn [x] [x x]) (range 128))))",
        16256,
    ),
    (
        "cat_xf_complete",
        "(transduce cat (completing +) 0 (map (fn [x] [x x]) (range 128)))",
        16256,
    ),
    (
        "mapcat_lazy_early",
        "(reduce + (take 64 (mapcat (fn [x] [x x]) (range 128))))",
        992,
    ),
    (
        "mapcat_xf_early",
        "(transduce (comp (mapcat (fn [x] [x x])) (take 64)) (completing +) 0 (range 128))",
        992,
    ),
    (
        "cat_lazy_early",
        "(reduce + (take 64 (apply concat (map (fn [x] [x x]) (range 128)))))",
        992,
    ),
    (
        "partition_lazy",
        "(reduce + (map #(reduce + %) (into [] (map vec (partition-all 3 (range 128))))))",
        8128,
    ),
    (
        "partition_transducer",
        "(reduce + (map #(reduce + %) (into [] (partition-all 3) (range 128))))",
        8128,
    ),
    (
        "cat_xf_early",
        "(transduce (comp cat (take 64)) (completing +) 0 (map (fn [x] [x x]) (range 128)))",
        992,
    ),
];
const EMBEDDING: &str = include_str!("../benchmarks/fixtures/interpreter/embedding.clj");
const MAIN: &str = include_str!("../benchmarks/fixtures/interpreter/main.clj");
const MODULES: &[(&str, &str, &str)] = &[
    (
        "report.data",
        "report/data.cljc",
        include_str!("../benchmarks/fixtures/interpreter/report/data.cljc"),
    ),
    (
        "report.metrics",
        "report/metrics.cljc",
        include_str!("../benchmarks/fixtures/interpreter/report/metrics.cljc"),
    ),
    (
        "report.main",
        "report/main.clj",
        include_str!("../benchmarks/fixtures/interpreter/report/main.clj"),
    ),
];

#[derive(Default)]
struct Cost {
    ns: u128,
    allocations: usize,
    reallocations: usize,
    bytes: usize,
    live: usize,
    peak: usize,
}

fn measure<T>(f: impl FnOnce() -> T) -> (T, Cost) {
    #[cfg(feature = "count-allocations")]
    {
        let (result, allocations, reallocations, bytes, live, peak) = allocations::measure(f);
        (
            result,
            Cost {
                ns: 0,
                allocations,
                reallocations,
                bytes,
                live,
                peak,
            },
        )
    }
    #[cfg(not(feature = "count-allocations"))]
    {
        let started = Instant::now();
        let result = f();
        (
            result,
            Cost {
                ns: started.elapsed().as_nanos(),
                ..Cost::default()
            },
        )
    }
}

fn batch(iterations: usize, mut f: impl FnMut() -> usize) -> (usize, Cost) {
    measure(|| (0..iterations).fold(0usize, |sum, _| sum.wrapping_add(black_box(f()))))
}

fn integer(value: light_cljs::interpreter::RootedValue) -> usize {
    let ValueView::Integer(value) = black_box(value).inspect() else {
        panic!("integer result")
    };
    value as usize
}

fn read(c: &Context, source: &light_cljs::interpreter::Source) -> Form {
    let mut reader = c.reader(source).unwrap();
    let form = reader.read_form().unwrap().unwrap();
    assert!(reader.read_form().unwrap().is_none());
    form
}

fn context() -> Context {
    let c = Context::new();
    black_box(c.current_namespace());
    c
}

fn provider(c: &mut Context) {
    c.set_source_provider(|request: &LoadRequest| {
        Ok(MODULES
            .iter()
            .find(|(name, _, _)| request.namespace == Name::new(None, name))
            .map(|(_, path, text)| SourceInput::new(*path, *text)))
    });
}

fn row(
    scenario: &str,
    phase: &str,
    sample: usize,
    iterations: usize,
    cost: Cost,
    memory: MemoryStats,
    checksum: usize,
) {
    println!(
        "{scenario},{phase},{sample},{iterations},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{checksum}",
        cost.ns,
        cost.allocations,
        cost.reallocations,
        cost.bytes,
        cost.live,
        cost.peak,
        memory.heap_values,
        memory.names,
        memory.sources,
        memory.executable_units,
        memory.namespaces,
        memory.vars,
        memory.roots,
        memory.slots
    );
}

fn diagnostics(c: &Context) {
    for stats in c.macro_expansion_stats() {
        let name = stats
            .name
            .namespace()
            .map(|ns| format!("{}/", String::from_utf16_lossy(ns)))
            .unwrap_or_default()
            + &String::from_utf16_lossy(stats.name.name());
        eprintln!(
            "macro name={} definition={:?} expansions={} generated_forms={} elapsed_ns={}",
            name,
            stats.definition,
            stats.expansions,
            stats.generated_forms,
            stats.elapsed.as_nanos()
        );
    }
    for stats in c.code_generation_stats() {
        eprintln!(
            "code location={:?} origin={:?} generations={} repeated={} expressions={} functions={} elapsed_ns={}",
            stats.location,
            stats.origin,
            stats.generations,
            stats.generations.saturating_sub(1),
            stats.generated_expressions,
            stats.generated_functions,
            stats.elapsed.as_nanos()
        );
    }
}

fn workload(
    name: &str,
    text: &str,
    expected: i64,
    samples: usize,
    iterations: usize,
    diagnose: bool,
) {
    let mut c = context();
    let source = c.source(format!("{name}.clj"), text);
    let tree = parse(text).unwrap();
    assert_eq!(tree.roots.len(), 1);
    let form = read(&c, &source);
    let code = c.analyze_form(&form).unwrap();
    // Prime every phase and registry/scratch before sampling. Code/source/form
    // roots survive all collections; temporary results do not.
    assert_eq!(integer(c.eval_analyzed(&code).unwrap()), expected as usize);
    c.collect_garbage();
    c.set_diagnostics(diagnose);
    for sample in 1..=samples {
        for phase in [
            "parse",
            "lower",
            "read",
            "analyze",
            "eval",
            "read_analyze_eval",
        ] {
            c.collect_garbage();
            let (checksum, cost) = batch(iterations, || match phase {
                "parse" => black_box(parse(black_box(text)).unwrap()).nodes.len(),
                "lower" => black_box(c.lower_form(&source, &tree, tree.roots[0]).unwrap())
                    .nodes()
                    .len(),
                "read" => black_box(read(&c, &source)).nodes().len(),
                "analyze" => black_box(c.analyze_form(&form).unwrap()).expression_count(),
                "eval" => integer(c.eval_analyzed(&code).unwrap()),
                _ => integer(c.eval_source(&source).unwrap()),
            });
            let expected = match phase {
                "parse" => tree.nodes.len(),
                "lower" | "read" => form.nodes().len(),
                "analyze" => code.expression_count(),
                _ => expected as usize,
            };
            assert_eq!(checksum, expected.wrapping_mul(iterations));
            row(
                name,
                phase,
                sample,
                iterations,
                cost,
                c.memory_stats(),
                checksum,
            );
            let (stats, cost) = measure(|| c.collect_garbage());
            row(
                name,
                &format!("gc_after_{phase}"),
                sample,
                1,
                cost,
                stats.after,
                stats.before.heap_values - stats.after.heap_values,
            );
        }
    }
    if diagnose {
        diagnostics(&c);
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use a release/bench build".into());
    }
    let (mut samples, mut iterations) = (9, 32);
    let (mut counts, mut diagnose) = (false, false);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => samples = args.next().ok_or("missing samples")?.parse()?,
            "--iterations" => iterations = args.next().ok_or("missing iterations")?.parse()?,
            "--allocations" => counts = true,
            "--diagnostics" => diagnose = true,
            "--bench" => {}
            "--help" | "-h" => {
                eprintln!(
                    "interpreter [--samples N] [--iterations N] [--allocations | --diagnostics]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if samples == 0 || iterations == 0 {
        return Err("samples/iterations must be positive".into());
    }
    if counts != cfg!(feature = "count-allocations") || (counts && diagnose) {
        return Err("allocation runs require count-allocations and --allocations; diagnostics use a separate timing build".into());
    }
    eprintln!(
        "samples={samples} iterations={iterations} allocations={counts} diagnostics={diagnose}; no filesystem I/O in resident phases"
    );
    println!(
        "scenario,phase,sample,iterations,elapsed_ns,allocations,reallocations,requested_bytes,live_requested_bytes,peak_requested_bytes,heap_values,names,sources,executable_units,namespaces,vars,roots,slots,checksum"
    );
    for _ in 0..iterations {
        black_box(Context::new());
        black_box(context());
    }
    for sample in 1..=samples {
        for phase in ["constructor_drop", "namespace_init_drop"] {
            let (_, cost) = batch(iterations, || {
                let c = if phase == "constructor_drop" {
                    Context::new()
                } else {
                    context()
                };
                black_box(c);
                1
            });
            row(
                "context",
                phase,
                sample,
                iterations,
                cost,
                MemoryStats::default(),
                iterations,
            );
        }
    }
    for (name, text, expected) in FIXTURES {
        workload(name, text, *expected, samples, iterations, diagnose);
    }
    let mut c = context();
    let function = c.eval_string(EMBEDDING).unwrap();
    let args = [c.integer(64)];
    assert_eq!(integer(c.invoke(&function, &args).unwrap()), 85355);
    c.collect_garbage();
    c.set_diagnostics(diagnose);
    for sample in 1..=samples {
        let (checksum, cost) = batch(iterations, || integer(c.invoke(&function, &args).unwrap()));
        assert_eq!(checksum, 85355 * iterations);
        row(
            "embedding",
            "invoke",
            sample,
            iterations,
            cost,
            c.memory_stats(),
            checksum,
        );
        let (stats, cost) = measure(|| c.collect_garbage());
        row(
            "embedding",
            "gc_after_invoke",
            sample,
            1,
            cost,
            stats.after,
            stats.before.heap_values - stats.after.heap_values,
        );
    }
    if diagnose {
        diagnostics(&c);
    }
    drop(c);
    for sample in 1..=samples {
        let mut c = context();
        provider(&mut c);
        c.set_diagnostics(diagnose);
        let source = c.source("main.clj", MAIN);
        let (result, cost) = measure(|| c.eval_source(&source).unwrap());
        assert_eq!(integer(result), 62182);
        row(
            "modules",
            "first_load",
            sample,
            1,
            cost,
            c.memory_stats(),
            62182,
        );
        c.collect_garbage();
        let (checksum, cost) = batch(iterations, || integer(c.eval_source(&source).unwrap()));
        assert_eq!(checksum, 62182 * iterations);
        row(
            "modules",
            "cached_require",
            sample,
            iterations,
            cost,
            c.memory_stats(),
            checksum,
        );
        let (stats, cost) = measure(|| c.collect_garbage());
        row(
            "modules",
            "gc_after_require",
            sample,
            1,
            cost,
            stats.after,
            stats.before.heap_values - stats.after.heap_values,
        );
        if diagnose && sample == 1 {
            diagnostics(&c);
        }
    }
    Ok(())
}
