//! `--callees`: the cost of each call shape of light-cljs-212 per call, in Node
//! and in the QuickJS engine light-cljs embeds. callees.js holds the shapes and
//! the method; benchmarks/compiler.md ("Direct call shapes") has the results.
//! It prints a Markdown table of the median nanoseconds per call over the timed
//! rounds, with the minimum in parentheses.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use std::collections::BTreeMap;
use std::error::Error;
use std::process::Command;
use std::time::Duration;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

const BENCHMARK: &str = include_str!("callees.js");

pub struct Config {
    /// Timed rounds; each times every shape and target once.
    pub rounds: usize,
    /// Untimed rounds before them.
    pub warmups: usize,
    /// Calls per sample.
    pub calls: usize,
}

/// Nanoseconds per call of every sample, by `shape/target`.
type Samples = BTreeMap<String, Vec<f64>>;

fn parse(json: &str) -> Result<Samples> {
    // `{"call/single":[1.5,2],...}`: keys never hold quotes or brackets.
    let mut samples = Samples::new();
    for entry in json
        .trim()
        .trim_start_matches('{')
        .trim_end_matches('}')
        .split("],")
    {
        let (key, values) = entry.split_once(":[").ok_or("malformed samples")?;
        let values = values
            .trim_end_matches(']')
            .split(',')
            .map(str::parse::<f64>)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        samples.insert(key.trim_matches('"').to_owned(), values);
    }
    Ok(samples)
}

fn call(config: &Config) -> String {
    format!(
        "({BENCHMARK})({}, {}, {})",
        config.rounds, config.warmups, config.calls
    )
}

fn node(config: &Config) -> Result<Samples> {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", &format!("console.log({})", call(config))])
        .output()?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).into_owned().into());
    }
    parse(&String::from_utf8(output.stdout)?)
}

fn quickjs(config: &Config) -> Result<Samples> {
    let engine = Evaluator::new(EvaluatorOptions {
        memory_limit: 1 << 30,
        poll_budget: u64::MAX,
        timeout: Duration::from_secs(24 * 60 * 60),
        ..EvaluatorOptions::default()
    })?;
    let value = engine.evaluate_js(&call(config))?;
    parse(&engine.string(value)?)
}

fn median(values: &[f64]) -> f64 {
    let mut sorted = values.to_vec();
    sorted.sort_by(f64::total_cmp);
    sorted[sorted.len() / 2]
}

fn minimum(values: &[f64]) -> f64 {
    values.iter().copied().fold(f64::INFINITY, f64::min)
}

pub fn run(config: &Config) -> Result<()> {
    let engines = [("Node", node(config)?), ("QuickJS", quickjs(config)?)];
    println!(
        "{} calls per sample, {} timed rounds after {} untimed; median ns per call (minimum)\n",
        config.calls, config.rounds, config.warmups
    );
    println!("| Shape | Target | Node | QuickJS |\n| --- | --- | ---: | ---: |");
    for key in engines[0].1.keys() {
        let (shape, target) = key.split_once('/').unwrap_or((key, ""));
        let cells: Vec<String> = engines
            .iter()
            .map(|(_, samples)| {
                let values = &samples[key];
                format!("{:.1} ({:.1})", median(values), minimum(values))
            })
            .collect();
        println!("| {shape} | {target} | {} | {} |", cells[0], cells[1]);
    }
    Ok(())
}
