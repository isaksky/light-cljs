//! Output size ratchet and cross-level differential execution for a fixed set of
//! small programs (docs/advanced-optimization.md, Phase 0).
//!
//! Every fixture has one byte ceiling per optimization level for the emitted
//! JavaScript expression (`prepare_application(..).emit_js()`, bytes before gzip).
//! The ratchet fails when any output grows past its ceiling. Ceilings only move
//! down: a change that shrinks output lowers them to the new size rounded up to
//! the next 100 bytes (next 1,000 from 10,000 bytes up) and records the sizes in
//! benchmarks/compiler.md. Raising a ceiling needs a reason in the commit message.
//! Print the current table with
//! `cargo nextest run --test compiler size_ratchet:: --no-capture`.
//!
//! The same fixtures run in Node at every level and must print the same output
//! as `OptimizationLevel::None`, the oracle. Add fixtures here when a later
//! optimization needs a size or behavior pinned; keep the five upstream rows.
use super::optimization::execute;
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use std::fmt::Write;

struct Fixture {
    name: &'static str,
    source: &'static str,
    /// Byte ceilings in `OptimizationLevel::ALL` order: none, prepare, advanced.
    ceilings: [usize; 3],
    /// Program output followed by the final value, as the `printed` helper
    /// shows them.
    printed: &'static str,
}

/// The first five rows are the Targets table of the design document, mirroring
/// upstream's `build_api_tests.clj` size ratchets (`trivial/core*.cljs`).
const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "console-log-sum",
        source: "(js/console.log (+ 1 2))",
        ceilings: [351_000, 351_000, 351_000],
        printed: "3\nundefined\n",
    },
    Fixture {
        name: "println-hi",
        source: r#"(enable-console-print!) (println "hi")"#,
        ceilings: [351_000, 351_000, 351_000],
        printed: "hi\nnull\n",
    },
    Fixture {
        name: "vector-printed",
        source: "(enable-console-print!) (println [1 2 3])",
        ceilings: [351_000, 351_000, 351_000],
        printed: "[1 2 3]\nnull\n",
    },
    Fixture {
        name: "map-printed",
        source: "(enable-console-print!) (println {})",
        ceilings: [351_000, 351_000, 351_000],
        printed: "{}\nnull\n",
    },
    Fixture {
        name: "seq-pipeline",
        source: "(js/console.log (->> (map inc (range 10)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity) into-array))",
        ceilings: [352_000, 352_000, 352_000],
        printed: "[ 6, 8 ]\nundefined\n",
    },
    Fixture {
        name: "add",
        source: "(+ 1 2)",
        ceilings: [351_000, 351_000, 351_000],
        printed: "3\n",
    },
    Fixture {
        name: "rest-fn",
        source: "(fn [& xs] xs)",
        ceilings: [352_000, 352_000, 352_000],
        printed: "#function\n",
    },
    Fixture {
        name: "keyword-case",
        source: "(defn kind [k] (case k :a 1 :b 2 3))
                 (+ (kind :a) (* 10 (kind :b)) (* 100 (kind :c)))",
        ceilings: [352_000, 352_000, 352_000],
        printed: "321\n",
    },
    Fixture {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (area (->Rect 3 4))",
        ceilings: [352_000, 352_000, 352_000],
        printed: "12\n",
    },
];

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
