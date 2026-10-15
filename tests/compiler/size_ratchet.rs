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
//! Every fixture also pins `ApplicationStats::removed_definitions` per level, so
//! a change in what preparation removes shows up next to the sizes.
//!
//! The same fixtures run in Node at every level and must print the same output
//! as `OptimizationLevel::None`, the oracle.
#[path = "../support/ratchet_fixtures.rs"]
pub(super) mod ratchet_fixtures;

use super::optimization::execute;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry_with_provider;
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use ratchet_fixtures::{FIXTURES, Fixture, NAMESPACE_FIXTURES, NamespaceFixture};
use std::fmt::Write;

fn program(fixture: &Fixture) -> Program {
    let source = fixture.source;
    analyze(source, &parse(source).unwrap())
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.name))
}

/// The program of `fixture`, from its entry over its in-memory source root.
fn namespace_program(fixture: &NamespaceFixture) -> Program {
    let files = fixture.files;
    let provider = |request: &LoadRequest| -> Result<Option<SourceInput>, LoadError> {
        let namespace = String::from_utf16_lossy(request.namespace.name());
        let path = format!("{}.cljs", namespace.replace('.', "/").replace('-', "_"));
        Ok(files
            .iter()
            .find(|(file, _)| *file == path)
            .map(|(file, source)| SourceInput::new(*file, *source)))
    };
    analyze_namespace_entry_with_provider(
        fixture.entry,
        &provider,
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{}: {error}", fixture.name))
}

/// A program the ratchet pins, from [`FIXTURES`] or [`NAMESPACE_FIXTURES`].
struct Pinned {
    name: &'static str,
    program: Program,
    ceilings: [usize; 3],
    removed: [usize; 3],
    printed: &'static str,
}

/// Every pinned program, the fixtures of one source text first.
fn pinned() -> Vec<Pinned> {
    FIXTURES
        .iter()
        .map(|fixture| Pinned {
            name: fixture.name,
            program: program(fixture),
            ceilings: fixture.ceilings,
            removed: fixture.removed,
            printed: fixture.printed,
        })
        .chain(NAMESPACE_FIXTURES.iter().map(|fixture| Pinned {
            name: fixture.name,
            program: namespace_program(fixture),
            ceilings: fixture.ceilings,
            removed: fixture.removed,
            printed: fixture.printed,
        }))
        .collect()
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
    for fixture in pinned() {
        write!(table, "{:<16}", fixture.name).unwrap();
        for (level, ceiling) in OptimizationLevel::ALL.into_iter().zip(fixture.ceilings) {
            let size = emit(&fixture.program, level).len();
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
fn removed_definitions_match_each_fixture() {
    for fixture in pinned() {
        let removed = OptimizationLevel::ALL.map(|level| {
            prepare_application(&fixture.program, ApplicationOptions::new(level))
                .stats()
                .removed_definitions
        });
        assert_eq!(
            removed, fixture.removed,
            "{}: removed definitions at none, prepare, advanced",
            fixture.name
        );
    }
}

#[test]
fn fixtures_print_identical_results_at_every_level() {
    for fixture in pinned() {
        let outputs = OptimizationLevel::ALL.map(|level| printed(&emit(&fixture.program, level)));
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

/// The Phase 1 exit criterion of docs/code-splitting.md (light-cljs-272): at
/// `advanced`, `features` costs under 200 bytes per `light.lazy/loadable` site
/// more than `features-direct`, the same example reading its two targets
/// directly, the `Loadable` type and `light.lazy/load` included.
#[test]
fn features_loadable_sites_cost_under_200_bytes_each_at_advanced() {
    let size = |name: &str| {
        let fixture = NAMESPACE_FIXTURES
            .iter()
            .find(|fixture| fixture.name == name)
            .unwrap();
        emit(&namespace_program(fixture), OptimizationLevel::Advanced).len()
    };
    let (loadables, direct) = (size("features"), size("features-direct"));
    let sites = 2;
    assert!(
        loadables < direct + 200 * sites,
        "features is {loadables} bytes at advanced, features-direct {direct}: {} bytes a site",
        loadables.saturating_sub(direct) / sites
    );
}

/// The `eight-records` fixture with its first `records` records and its first
/// `deftypes` deftypes.
fn types_program(records: usize, deftypes: usize) -> String {
    const BODIES: [&str; 8] = [
        "(+ a b)", "(- a b)", "(* a b)", "(+ b a)", "(- b a)", "(* b a)", "(+ a a)", "(* b b)",
    ];
    let mut source = String::from("(defprotocol Shape (area [s]))\n");
    for (index, body) in BODIES.iter().enumerate().take(records) {
        writeln!(
            source,
            "(defrecord R{} [a b] Shape (area [_] {body}))",
            index + 1
        )
        .unwrap();
    }
    for (index, body) in BODIES.iter().rev().enumerate().take(deftypes) {
        writeln!(
            source,
            "(deftype T{} [a b] Shape (area [_] {body}))",
            index + 1
        )
        .unwrap();
    }
    let values: Vec<String> = (1..=records)
        .map(|n| format!("(->R{n} {n} {})", n + 1))
        .chain((1..=deftypes).map(|n| format!("(T{n}. {} {})", n + 8, n + 9)))
        .collect();
    write!(
        source,
        "(reduce (fn [t s] (+ t (area s) (.-a s))) 0 [{}])",
        values.join(" ")
    )
    .unwrap();
    source
}

/// light-cljs-264: at `Advanced` each record of `eight-records` past the
/// first adds at most 220 bytes, and each deftype at most 200: a type is its
/// constructor, method entries and factory, around the runtime's
/// `defineType` (benchmarks/compiler.md, "Per-type output").
#[test]
fn each_type_of_eight_records_costs_at_most_its_budget() {
    let fixture = FIXTURES
        .iter()
        .find(|fixture| fixture.name == "eight-records")
        .unwrap();
    assert_eq!(types_program(8, 8), fixture.source);
    let size = |records, deftypes| {
        let source = types_program(records, deftypes);
        let program = analyze(&source, &parse(&source).unwrap()).unwrap();
        emit(&program, OptimizationLevel::Advanced).len()
    };
    let added = |sizes: Vec<usize>| -> Vec<usize> {
        sizes.windows(2).map(|pair| pair[1] - pair[0]).collect()
    };
    let records = added((1..=8).map(|records| size(records, 8)).collect());
    let deftypes = added((1..=8).map(|deftypes| size(8, deftypes)).collect());
    println!("bytes per extra record: {records:?}\nbytes per extra deftype: {deftypes:?}");
    assert!(records.iter().all(|&bytes| bytes <= 220), "{records:?}");
    assert!(deftypes.iter().all(|&bytes| bytes <= 200), "{deftypes:?}");
}
