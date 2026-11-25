//! Run with `cargo run --example evaluator`. Uses V8 when available.
use light_cljs::{
    analyze,
    evaluator::{Evaluator, EvaluatorOptions},
    parse,
};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let source = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "(apply + [20 22])".into());
    let syntax = parse(&source)?;
    let program = analyze(&source, &syntax)?;
    let evaluator = Evaluator::new(EvaluatorOptions::default())?;
    println!("{}", evaluator.print(evaluator.evaluate(&program)?)?);
    Ok(())
}
