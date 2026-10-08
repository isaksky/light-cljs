//! Run with `cargo bench --bench tokenizer -- --samples 9 --ms 100`.
//! Stdout is raw CSV; progress, configuration, and medians go to stderr.

use std::{
    env,
    error::Error,
    fs,
    hint::black_box,
    mem::size_of,
    time::{Duration, Instant},
};

use light_cljs::{Token, TokenKind, Tokenizer};

struct Input {
    name: String,
    source: String,
    tokens: usize,
}

impl Input {
    fn new(name: String, source: String) -> Self {
        let tokens = Tokenizer::new(&source)
            .map(|token| {
                assert!(
                    !matches!(token.kind, TokenKind::Error(_)),
                    "{name}: {token:?}"
                );
                token
            })
            .count();
        Self {
            name,
            source,
            tokens,
        }
    }
}

// Keep a named function for inspection of the actual optimized scanner.
#[inline(never)]
fn scan(source: &str) -> usize {
    let mut count = 0;
    for token in Tokenizer::new(source) {
        black_box(token);
        count += 1;
    }
    count
}

type Scan = fn(&str) -> usize;
const VARIANTS: &[(&str, Scan)] = &[("full-usize", scan)];

fn repeated(pattern: &str, minimum_bytes: usize) -> String {
    // Repeat whole UTF-8 strings, with generation outside every timed region.
    pattern.repeat(minimum_bytes.div_ceil(pattern.len()))
}

fn fixtures() -> Vec<Input> {
    let small = 256 * 1024;
    let long = format!("({} {})\n", "abcdefgh".repeat(128), "long-name".repeat(128));
    let whitespace = format!("{}(x){}y\n", " \t,\n\r\x0b\x0c".repeat(64), " ".repeat(128));
    let mixed =
        format!("(defn calculate (alpha café λ) (+ 123 456) (nested (x y z)))\n{long}{whitespace}");
    [
        ("short-atoms", repeated("(+ 1 2 alpha :beta 42)\n", small)),
        ("long-atoms", repeated(&long, small)),
        ("dense-parens", repeated("(()())()((()))", small)),
        ("whitespace-heavy", repeated(&whitespace, small)),
        ("unicode", repeated("(café λ 你好 🦀 e\u{301})\n", small)),
        ("large-mixed", repeated(&mixed, 16 * 1024 * 1024)),
        (
            "reader-syntax",
            repeated(include_str!("../tests/fixtures/reader_forms.cljs"), small),
        ),
        (
            "large-reader",
            repeated(
                include_str!("../tests/fixtures/reader_forms.cljs"),
                16 * 1024 * 1024,
            ),
        ),
    ]
    .into_iter()
    .map(|(name, source)| Input::new(name.into(), source))
    .collect()
}

fn batch(scan: Scan, source: &str, iterations: usize) -> (Duration, usize) {
    let start = Instant::now();
    let mut count = 0usize;
    for _ in 0..iterations {
        // Make the input opaque on each traversal as well as consuming every
        // entire token in scan(). Include iterator and barrier costs, not I/O.
        count = count.wrapping_add(scan(black_box(source)));
    }
    let elapsed = start.elapsed();
    black_box(count);
    (elapsed, count)
}

fn calibrate(scan: Scan, input: &Input, target: Duration) -> usize {
    let mut iterations = 1;
    loop {
        let (elapsed, count) = batch(scan, &input.source, iterations);
        assert_eq!(count, input.tokens.wrapping_mul(iterations));
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
    variant: usize,
    round: usize,
    iterations: usize,
    seconds: f64,
}

fn main() -> Result<(), Box<dyn Error>> {
    if cfg!(debug_assertions) {
        return Err("benchmark requires an optimized build: cargo bench --bench tokenizer".into());
    }
    let mut samples = 9;
    let mut millis = 100;
    let mut files = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--samples" => samples = args.next().ok_or("missing --samples value")?.parse()?,
            "--ms" => millis = args.next().ok_or("missing --ms value")?.parse()?,
            // Cargo may supply this to harness=false targets.
            "--bench" => {}
            "--help" | "-h" => {
                eprintln!(
                    "cargo bench --bench tokenizer -- [--samples N] [--ms N] [FILE ...]\nBuilt-in fixtures plus optional UTF-8 files; stdout is CSV."
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

    let mut inputs = fixtures();
    for file in files {
        let source = fs::read_to_string(&file)?;
        inputs.push(Input::new(file, source));
    }
    eprintln!(
        "Token: {} bytes; Tokenizer: {} bytes",
        size_of::<Token>(),
        size_of::<Tokenizer<'_>>()
    );
    eprintln!(
        "{samples} samples, target {millis} ms/sample, >=20 ms calibration per input/variant; single thread, warm resident inputs"
    );
    eprintln!(
        "No I/O, input generation, or allocation in timed scans; black_box per token and per traversal"
    );

    let mut results = Vec::new();
    for (input_index, input) in inputs.iter().enumerate() {
        eprintln!(
            "{}: {} bytes, {} tokens",
            input.name,
            input.source.len(),
            input.tokens
        );
        let iterations: Vec<_> = VARIANTS
            .iter()
            .map(|(_, scan)| {
                assert_eq!(scan(&input.source), input.tokens);
                calibrate(*scan, input, Duration::from_millis(millis))
            })
            .collect();
        for round in 0..samples {
            // Rotate order when comparing variants to reduce ordering bias.
            for offset in 0..VARIANTS.len() {
                let variant = (round + offset) % VARIANTS.len();
                let n = iterations[variant];
                let (elapsed, count) = batch(VARIANTS[variant].1, &input.source, n);
                assert_eq!(count, input.tokens.wrapping_mul(n));
                results.push(Sample {
                    input: input_index,
                    variant,
                    round,
                    iterations: n,
                    seconds: elapsed.as_secs_f64(),
                });
            }
        }
    }

    // All reporting happens after measurement.
    println!(
        "variant,input,bytes,tokens,iterations,sample,seconds,bytes_per_second,tokens_per_second"
    );
    for sample in &results {
        let input = &inputs[sample.input];
        let traversals_per_second = sample.iterations as f64 / sample.seconds;
        println!(
            "{},\"{}\",{},{},{},{},{:.9},{:.3},{:.3}",
            VARIANTS[sample.variant].0,
            input.name.replace('"', "\"\""),
            input.source.len(),
            input.tokens,
            sample.iterations,
            sample.round + 1,
            sample.seconds,
            input.source.len() as f64 * traversals_per_second,
            input.tokens as f64 * traversals_per_second
        );
    }
    for (input_index, input) in inputs.iter().enumerate() {
        for (variant, (name, _)) in VARIANTS.iter().enumerate() {
            let mut rates: Vec<_> = results
                .iter()
                .filter(|s| s.input == input_index && s.variant == variant)
                .map(|s| s.iterations as f64 / s.seconds)
                .collect();
            rates.sort_by(f64::total_cmp);
            let median = if rates.len() % 2 == 0 {
                (rates[rates.len() / 2 - 1] + rates[rates.len() / 2]) / 2.0
            } else {
                rates[rates.len() / 2]
            };
            eprintln!(
                "{name:16} {:18} {:9.1} MB/s {:9.1} Mtokens/s (range {:.1}..{:.1} MB/s)",
                input.name,
                median * input.source.len() as f64 / 1e6,
                median * input.tokens as f64 / 1e6,
                rates[0] * input.source.len() as f64 / 1e6,
                rates[rates.len() - 1] * input.source.len() as f64 / 1e6
            );
        }
    }
    Ok(())
}
