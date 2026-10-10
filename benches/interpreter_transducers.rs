//! Equivalent pipelines in one binary; parsing/setup and explicit GC stay separate.
use light_cljs::interpreter::{
    AnalyzedForm, Context, ContextOptions, EvalErrorKind, ExceptionCategory, InvokeError,
    MemoryStats, Name, RootedValue, ValueView,
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

macro_rules! fixture {
    ($name:literal) => {
        (
            $name,
            include_str!(concat!(
                "../benchmarks/fixtures/interpreter/transducers/",
                $name,
                ".clj"
            )),
        )
    };
}
const FIXTURES: &[(&str, &str)] = &[
    fixture!("scalar"),
    fixture!("identity"),
    fixture!("reject_all"),
    fixture!("early_take"),
    fixture!("mapcat"),
    fixture!("nested_cat"),
    fixture!("partition_sum"),
    fixture!("partition_vector"),
    fixture!("custom_volatile"),
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
        let (value, allocations, reallocations, bytes, live, peak) = allocations::measure(f);
        (
            value,
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
        let start = Instant::now();
        let value = f();
        (
            value,
            Cost {
                ns: start.elapsed().as_nanos(),
                ..Cost::default()
            },
        )
    }
}
fn row(
    name: &str,
    phase: &str,
    sample: usize,
    iterations: usize,
    cost: Cost,
    m: MemoryStats,
    checksum: usize,
) {
    println!(
        "{name},{phase},{sample},{iterations},{},{},{},{},{},{},{},{},{},{},{},{},{},{},{checksum}",
        cost.ns,
        cost.allocations,
        cost.reallocations,
        cost.bytes,
        cost.live,
        cost.peak,
        m.heap_values,
        m.names,
        m.sources,
        m.executable_units,
        m.namespaces,
        m.vars,
        m.roots,
        m.slots
    );
}

// Full recursive values are checked outside timing, including vector partition
// groups. Timed output handling only black-boxes the value and extracts its
// scalar or outer vector length; it does not copy/print the materialized result.
#[derive(Debug, PartialEq)]
enum Snapshot {
    Integer(i64),
    Vector(Vec<Snapshot>),
}
fn snapshot(value: &RootedValue) -> Snapshot {
    match value.inspect() {
        ValueView::Integer(n) => Snapshot::Integer(n),
        ValueView::Vector(_) => {
            Snapshot::Vector(value.elements().unwrap().iter().map(snapshot).collect())
        }
        other => panic!("unexpected output {other:?}"),
    }
}
fn checksum(value: RootedValue) -> usize {
    match black_box(value).inspect() {
        ValueView::Integer(n) => n as usize,
        ValueView::Vector(n) => n,
        other => panic!("unexpected output {other:?}"),
    }
}
fn analyze(c: &mut Context, text: &str) -> AnalyzedForm {
    let source = c.source("transducers-eval.clj", text);
    let form = c.reader(&source).unwrap().read_form().unwrap().unwrap();
    c.analyze_form(&form).unwrap()
}

struct Work {
    factory: RootedValue,
    xf: RootedValue,
    run_xf: RootedValue,
    run_lazy: RootedValue,
    input: RootedValue,
    rf: RootedValue,
    code_xf: AnalyzedForm,
    code_lazy: AnalyzedForm,
}
impl Work {
    fn operation(&self, c: &mut Context, phase: &str) -> usize {
        match phase {
            "construct_xf" => {
                black_box(c.invoke(&self.factory, &[]).unwrap());
                1
            }
            "apply_xf" => {
                black_box(c.invoke(&self.xf, std::slice::from_ref(&self.rf)).unwrap());
                1
            }
            "construct_invoke_xf" => {
                let xf = c.invoke(&self.factory, &[]).unwrap();
                checksum(c.invoke(&self.run_xf, &[xf, self.input.clone()]).unwrap())
            }
            "invoke_xf" => checksum(
                c.invoke(&self.run_xf, &[self.xf.clone(), self.input.clone()])
                    .unwrap(),
            ),
            "invoke_lazy" => checksum(
                c.invoke(&self.run_lazy, std::slice::from_ref(&self.input))
                    .unwrap(),
            ),
            "eval_xf" => checksum(c.eval_analyzed(&self.code_xf).unwrap()),
            "eval_lazy" => checksum(c.eval_analyzed(&self.code_lazy).unwrap()),
            _ => unreachable!(),
        }
    }
}

fn samples(
    c: &mut Context,
    name: &str,
    phase: &str,
    count: usize,
    iterations: usize,
    expected: usize,
    mut op: impl FnMut(&mut Context) -> usize,
) {
    // Prime the exact collection cadence twice, including all slot and scratch
    // high-water capacity. Returned outputs/reducers are dropped each operation.
    for _ in 0..2 {
        for _ in 0..iterations {
            assert_eq!(op(c), expected);
        }
        c.collect_garbage();
    }
    let warmed = c.memory_stats();
    let gc_phase = format!("gc_after_{phase}");
    for sample in 1..=count {
        let (sum, cost) =
            measure(|| (0..iterations).fold(0usize, |sum, _| sum.wrapping_add(black_box(op(c)))));
        assert_eq!(sum, expected.wrapping_mul(iterations));
        row(name, phase, sample, iterations, cost, c.memory_stats(), sum);
        let (stats, cost) = measure(|| c.collect_garbage());
        assert_eq!(
            stats.after, warmed,
            "{name}/{phase}: warmed live records and slots must stabilize"
        );
        #[cfg(feature = "count-allocations")]
        assert_eq!(
            (cost.allocations, cost.reallocations, cost.bytes),
            (0, 0, 0),
            "warmed GC must reuse scratch"
        );
        row(
            name,
            &gc_phase,
            sample,
            1,
            cost,
            stats.after,
            stats.before.heap_values - stats.after.heap_values,
        );
    }
}

fn workload(name: &str, source: &str, count: usize, iterations: usize, diagnose: bool) {
    let mut c = Context::new();
    let parts = c.eval_string(source).unwrap().elements().unwrap();
    assert_eq!(parts.len(), 5);
    let factory = parts[0].clone();
    let xf = c.invoke(&factory, &[]).unwrap();
    let run_xf = parts[1].clone();
    let run_lazy = parts[2].clone();
    let input = parts[3].clone();
    let expected = snapshot(&parts[4]);
    let expected_checksum = checksum(parts[4].clone());
    // Reusing one xf must start state afresh for every transformed reducer.
    for _ in 0..3 {
        assert_eq!(
            snapshot(&c.invoke(&run_xf, &[xf.clone(), input.clone()]).unwrap()),
            expected,
            "{name}/xf"
        );
        assert_eq!(
            snapshot(&c.invoke(&run_lazy, std::slice::from_ref(&input)).unwrap()),
            expected,
            "{name}/lazy"
        );
    }
    drop(parts);
    c.merge_opts(ContextOptions {
        bindings: vec![
            (Name::new(None, "benchmark-xf"), xf.clone()),
            (Name::new(None, "benchmark-input"), input.clone()),
            (Name::new(None, "benchmark-run-xf"), run_xf.clone()),
            (Name::new(None, "benchmark-run-lazy"), run_lazy.clone()),
        ],
        ..ContextOptions::default()
    })
    .unwrap();
    let code_xf = analyze(&mut c, "(benchmark-run-xf benchmark-xf benchmark-input)");
    let code_lazy = analyze(&mut c, "(benchmark-run-lazy benchmark-input)");
    let rf = c.eval_string("(completing +)").unwrap();
    let work = Work {
        factory,
        xf,
        run_xf,
        run_lazy,
        input,
        rf,
        code_xf,
        code_lazy,
    };
    c.set_diagnostics(diagnose);
    for phase in [
        "construct_xf",
        "apply_xf",
        "construct_invoke_xf",
        "invoke_xf",
        "invoke_lazy",
        "eval_xf",
        "eval_lazy",
    ] {
        let expected = if matches!(phase, "construct_xf" | "apply_xf") {
            1
        } else {
            expected_checksum
        };
        samples(&mut c, name, phase, count, iterations, expected, |c| {
            work.operation(c, phase)
        });
    }
    if diagnose {
        eprintln!(
            "{name}: macro_records={} code_records={}",
            c.macro_expansion_stats().len(),
            c.code_generation_stats().len()
        );
    }
}

fn failed_reclamation(count: usize, iterations: usize) {
    let mut c = Context::new();
    let functions = c
        .eval_string(include_str!(
            "../benchmarks/fixtures/interpreter/transducers/failures.clj"
        ))
        .unwrap()
        .elements()
        .unwrap();
    for (index, function) in functions.iter().enumerate() {
        samples(
            &mut c,
            &format!("failure_{index}"),
            "invoke_failed_xf",
            count,
            iterations,
            1,
            |c| {
                let InvokeError::Execution(error) = black_box(c.invoke(function, &[])).unwrap_err()
                else {
                    panic!("expected execution failure");
                };
                assert_eq!(
                    error.kind,
                    EvalErrorKind::Thrown(ExceptionCategory::ExceptionInfo)
                );
                1
            },
        );
    }
}
fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use a release/bench build".into());
    }
    let (mut count, mut iterations, mut counts, mut diagnose) = (9, 32, false, false);
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => count = args.next().ok_or("missing samples")?.parse()?,
            "--iterations" => iterations = args.next().ok_or("missing iterations")?.parse()?,
            "--allocations" => counts = true,
            "--diagnostics" => diagnose = true,
            "--bench" => {}
            "--help" | "-h" => {
                eprintln!(
                    "interpreter_transducers [--samples N] [--iterations N] [--allocations | --diagnostics]"
                );
                return Ok(());
            }
            _ => return Err(format!("unknown option {arg}").into()),
        }
    }
    if count == 0 || iterations == 0 {
        return Err("samples/iterations must be positive".into());
    }
    if counts != cfg!(feature = "count-allocations") || (counts && diagnose) {
        return Err("allocation runs require count-allocations and --allocations; diagnostics use a separate timing build".into());
    }
    eprintln!(
        "samples={count} iterations={iterations} allocations={counts} diagnostics={diagnose}; GC once per batch; full values validated before timing"
    );
    println!(
        "scenario,phase,sample,iterations,elapsed_ns,allocations,reallocations,requested_bytes,live_requested_bytes,peak_requested_bytes,heap_values,names,sources,executable_units,namespaces,vars,roots,slots,checksum"
    );
    for (name, source) in FIXTURES {
        workload(name, source, count, iterations, diagnose);
    }
    failed_reclamation(count, iterations);
    Ok(())
}
