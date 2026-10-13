//! Output size ratchet and cross-level differential execution for a fixed set of
//! small programs (docs/advanced-optimization.md, Phase 0).
//!
//! Every fixture has one byte ceiling per optimization level for the emitted
//! JavaScript expression (`prepare_application(..).emit_js()`, bytes before gzip).
//! The ratchet fails when any output grows past its ceiling. The fixtures and
//! their ceilings live in tests/support/ratchet_fixtures.rs, which says how to
//! move a ceiling; benches/compiler.rs `--levels` measures startup for the same
//! set. Print the current table with
//! `cargo nextest run --test compiler size_ratchet:: --no-capture`.
//!
//! The same fixtures run in Node at every level and must print the same output
//! as `OptimizationLevel::None`, the oracle.
#[path = "../support/ratchet_fixtures.rs"]
mod ratchet_fixtures;

use super::optimization::execute;
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use ratchet_fixtures::{FIXTURES, Fixture};
use std::fmt::Write;

fn program(fixture: &Fixture) -> Program {
    let source = fixture.source;
    analyze(source, &parse(source).unwrap())
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.name))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

/// Run `js` in Node and `console.log` its value, except that a function prints
/// as `#function`: Node would list its compiler-owned ABI properties, whose
/// names a level may change.
fn printed(js: &str) -> String {
    execute(&format!(
        "const value = {js};\nconsole.log(typeof value === \"function\" ? \"#function\" : value);"
    ))
}

fn thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

#[test]
fn emitted_sizes_stay_under_their_ceilings() {
    let mut table = format!("{:<16}", "bytes / ceiling");
    for level in OptimizationLevel::ALL {
        write!(table, " {:>20}", level.name()).unwrap();
    }
    table.push('\n');
    let mut exceeded = Vec::new();
    for fixture in FIXTURES {
        let program = program(fixture);
        write!(table, "{:<16}", fixture.name).unwrap();
        for (level, ceiling) in OptimizationLevel::ALL.into_iter().zip(fixture.ceilings) {
            let size = emit(&program, level).len();
            let mark = if size > ceiling {
                exceeded.push(format!("{} at {level}", fixture.name));
                '!'
            } else {
                ' '
            };
            let cell = format!("{} / {}", thousands(size), thousands(ceiling));
            write!(table, " {cell:>19}{mark}").unwrap();
        }
        table.push('\n');
    }
    println!("{table}");
    assert!(
        exceeded.is_empty(),
        "emitted JavaScript grew past its ratchet ceiling for {}; \
         shrink the output or raise the ceiling with a reason (marked !):\n{table}",
        exceeded.join(", ")
    );
}

#[test]
fn fixtures_print_identical_results_at_every_level() {
    for fixture in FIXTURES {
        let program = program(fixture);
        let outputs = OptimizationLevel::ALL.map(|level| printed(&emit(&program, level)));
        assert_eq!(
            outputs[0], fixture.printed,
            "{} at none (the oracle)",
            fixture.name
        );
        for (level, output) in OptimizationLevel::ALL.into_iter().zip(&outputs).skip(1) {
            assert_eq!(output, &outputs[0], "{} at {level}", fixture.name);
        }
    }
}
