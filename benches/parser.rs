//! End-to-end syntax construction, including allocation and drop, excluding I/O.
use light_cljs::{Node, NodeId, NodeKind, Parser, SyntaxTree, parse};
use std::{
    env,
    error::Error,
    fs,
    hint::black_box,
    mem::size_of,
    time::{Duration, Instant},
};

#[cfg(feature = "count-allocations")]
#[path = "support/allocations.rs"]
mod allocations;
#[cfg(feature = "count-allocations")]
#[global_allocator]
static ALLOCATOR: allocations::CountingAllocator = allocations::CountingAllocator;

struct Input {
    name: String,
    source: String,
    nodes: usize,
    roots: usize,
}

impl Input {
    fn new(name: String, source: String) -> Result<Self, Box<dyn Error>> {
        let tree = parse(&source).map_err(|error| format!("{name}: {error:?}"))?;
        Ok(Self {
            name,
            source,
            nodes: tree.nodes.len(),
            roots: tree.roots.len(),
        })
    }
}

fn repeated(pattern: &str, bytes: usize) -> String {
    pattern.repeat(bytes.div_ceil(pattern.len()))
}

fn fixtures() -> Result<Vec<Input>, Box<dyn Error>> {
    let small = 256 * 1024;
    let long = format!("({} {})\n", "abcdefgh".repeat(128), "long-name".repeat(128));
    let whitespace = format!("{}(x){}y\n", " \t,\n\r\x0b\x0c".repeat(64), " ".repeat(128));
    [
        ("short-atoms", repeated("(+ 1 2 alpha :beta 42)\n", small)),
        ("long-atoms", repeated(&long, small)),
        ("dense-parens", repeated("(()())()((()))", small)),
        ("whitespace-heavy", repeated(&whitespace, small)),
        ("unicode", repeated("(café λ 你好 🦀 e\u{301})\n", small)),
        ("wide-vector", format!("[{}]", "x 1 :k ".repeat(50_000))),
        (
            "reader-syntax",
            repeated(include_str!("../tests/fixtures/parser_forms.cljs"), small),
        ),
        (
            "large-reader",
            repeated(
                include_str!("../tests/fixtures/parser_forms.cljs"),
                8 * 1024 * 1024,
            ),
        ),
    ]
    .into_iter()
    .map(|(name, source)| Input::new(name.into(), source))
    .collect()
}

#[inline(never)]
fn build(source: &str) -> usize {
    let tree = parse(source).expect("validated benchmark input");
    black_box(&tree);
    // All node storage is observable at the barrier. Allocation and destruction
    // are both included; there is no per-node printing in the measured section.
    tree.nodes.len()
}

fn batch(source: &str, iterations: usize) -> (Duration, usize) {
    let start = Instant::now();
    let mut nodes = 0usize;
    for _ in 0..iterations {
        nodes = nodes.wrapping_add(build(black_box(source)));
    }
    let elapsed = start.elapsed();
    black_box(nodes);
    (elapsed, nodes)
}

fn calibrate(input: &Input, target: Duration) -> usize {
    let mut iterations = 1;
    loop {
        let (elapsed, nodes) = batch(&input.source, iterations);
        assert_eq!(nodes, input.nodes.wrapping_mul(iterations));
        if elapsed >= Duration::from_millis(20) {
            return ((iterations as f64 * target.as_secs_f64() / elapsed.as_secs_f64()).round()
                as usize)
                .clamp(1, 1_000_000);
        }
        iterations *= 2;
    }
}

struct Sample {
    input: usize,
    round: usize,
    iterations: usize,
    seconds: f64,
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("use cargo bench --bench parser for an optimized build".into());
    }
    let (mut samples, mut millis) = (9, 100);
    let mut count_allocations = false;
    let mut files = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => samples = args.next().ok_or("missing --samples value")?.parse()?,
            "--ms" => millis = args.next().ok_or("missing --ms value")?.parse()?,
            "--allocations" => count_allocations = true,
            "--bench" => {}
            "--help" | "-h" => {
                eprintln!(
                    "cargo bench --bench parser -- [--samples N] [--ms N] [FILE ...]\nAllocation counts: add --features count-allocations before --, and --allocations after it."
                );
                return Ok(());
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}").into()),
            _ => files.push(arg),
        }
    }
    if samples == 0 || millis == 0 {
        return Err("--samples and --ms must be positive".into());
    }
    if count_allocations != cfg!(feature = "count-allocations") {
        return Err("--allocations requires --features count-allocations; timing runs require the feature to be OFF".into());
    }
    let mut inputs = fixtures()?;
    for path in files {
        let source = fs::read_to_string(&path)?;
        inputs.push(Input::new(path, source)?);
    }
    eprintln!(
        "Node: {} bytes; NodeKind: {}; NodeId: {}; Parser: {}; SyntaxTree: {}",
        size_of::<Node>(),
        size_of::<NodeKind>(),
        size_of::<NodeId>(),
        size_of::<Parser<'_>>(),
        size_of::<SyntaxTree>()
    );

    #[cfg(feature = "count-allocations")]
    {
        println!("input,bytes,nodes,roots,allocations,reallocations,requested_bytes");
        for input in &inputs {
            let (tree, allocs, reallocs, bytes) =
                allocations::measure(|| parse(black_box(&input.source)).unwrap());
            black_box(&tree);
            assert_eq!(tree.nodes.len(), input.nodes);
            println!(
                "\"{}\",{},{},{},{},{},{}",
                input.name.replace('"', "\"\""),
                input.source.len(),
                input.nodes,
                input.roots,
                allocs,
                reallocs,
                bytes
            );
        }
        Ok(())
    }

    #[cfg(not(feature = "count-allocations"))]
    timing(&inputs, samples, millis)
}

#[cfg_attr(feature = "count-allocations", allow(dead_code))]
fn timing(inputs: &[Input], samples: usize, millis: u64) -> Result<(), Box<dyn Error>> {
    eprintln!(
        "{samples} samples; target {millis} ms/sample; >=20 ms calibration; single thread; warm resident source"
    );
    eprintln!(
        "Includes parse, allocations, tree barrier and drop; excludes I/O, generation, line index and reporting; no counting allocator"
    );
    let mut results = Vec::new();
    for (index, input) in inputs.iter().enumerate() {
        let iterations = calibrate(input, Duration::from_millis(millis));
        for round in 0..samples {
            let (elapsed, nodes) = batch(&input.source, iterations);
            assert_eq!(nodes, input.nodes.wrapping_mul(iterations));
            results.push(Sample {
                input: index,
                round,
                iterations,
                seconds: elapsed.as_secs_f64(),
            });
        }
    }
    println!("input,bytes,nodes,roots,iterations,sample,seconds,bytes_per_second,nodes_per_second");
    for sample in &results {
        let input = &inputs[sample.input];
        let rate = sample.iterations as f64 / sample.seconds;
        println!(
            "\"{}\",{},{},{},{},{},{:.9},{:.3},{:.3}",
            input.name.replace('"', "\"\""),
            input.source.len(),
            input.nodes,
            input.roots,
            sample.iterations,
            sample.round + 1,
            sample.seconds,
            rate * input.source.len() as f64,
            rate * input.nodes as f64
        );
    }
    for (index, input) in inputs.iter().enumerate() {
        let mut rates: Vec<_> = results
            .iter()
            .filter(|s| s.input == index)
            .map(|s| s.iterations as f64 / s.seconds)
            .collect();
        rates.sort_by(f64::total_cmp);
        let median = if rates.len().is_multiple_of(2) {
            (rates[rates.len() / 2 - 1] + rates[rates.len() / 2]) / 2.0
        } else {
            rates[rates.len() / 2]
        };
        eprintln!(
            "{:20} {} bytes, {} nodes, {:.1} MB/s, {:.1} Mnodes/s (range {:.1}..{:.1} MB/s)",
            input.name,
            input.source.len(),
            input.nodes,
            median * input.source.len() as f64 / 1e6,
            median * input.nodes as f64 / 1e6,
            rates[0] * input.source.len() as f64 / 1e6,
            rates[rates.len() - 1] * input.source.len() as f64 / 1e6
        );
    }
    Ok(())
}
