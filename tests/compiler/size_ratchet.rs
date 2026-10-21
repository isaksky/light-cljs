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
use crate::temp_dir::TempDir;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::chunks::ChunkedOutput;
use light_cljs::compiler::{analyze_namespace_entry, analyze_namespace_entry_with_provider};
use light_cljs::source_provider::{LoadError, LoadRequest, SourceInput};
use light_cljs::{
    ApplicationOptions, ChunkedRetention, MacroHost, OptimizationLevel, Program, analyze, parse,
    prepare_application,
};
use ratchet_fixtures::{
    FEATURES_CHUNK_CEILINGS, FIXTURES, Fixture, NAMESPACE_FIXTURES, NamespaceFixture,
};
use std::fmt::Write;
use std::fs;
use std::path::{Path, PathBuf};

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

pub(crate) fn thousands(n: usize) -> String {
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
/// first adds at most 230 bytes, and each deftype at most 200: a type is its
/// constructor, method entries and factory, around the runtime's
/// `defineType` (benchmarks/compiler.md, "Per-type output"). Since
/// light-cljs-346 the constructor is a class with a brand of its own and its
/// check, 10 bytes more than the function a record had (220).
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
    assert!(records.iter().all(|&bytes| bytes <= 230), "{records:?}");
    assert!(deftypes.iter().all(|&bytes| bytes <= 200), "{deftypes:?}");
}

/// `protocols` protocols of two one-argument methods, all implemented by one
/// record, with every method called as a value.
fn protocols_program(protocols: usize) -> String {
    let mut source = String::new();
    let mut implementations = String::new();
    let mut methods = Vec::new();
    for n in 1..=protocols {
        writeln!(source, "(defprotocol P{n} (p{n}a [x]) (p{n}b [x]))").unwrap();
        write!(
            implementations,
            " P{n} (p{n}a [_] (+ v {n})) (p{n}b [_] (* v {n}))"
        )
        .unwrap();
        methods.extend([format!("p{n}a"), format!("p{n}b")]);
    }
    writeln!(source, "(defrecord R [v]{implementations})").unwrap();
    write!(
        source,
        "(reduce + 0 (map #(% (->R 2)) [{}]))",
        methods.join(" ")
    )
    .unwrap();
    source
}

/// light-cljs-265: at `Advanced` each protocol of two methods past the first
/// adds at most 350 bytes (upstream `:advanced` adds 488 for the same
/// program; benchmarks/compiler.md, "Per-method protocol dispatchers"), of
/// which each method's own dispatcher is about 53.
#[test]
fn each_two_method_protocol_costs_at_most_its_budget() {
    let size = |protocols| {
        let source = protocols_program(protocols);
        let program = analyze(&source, &parse(&source).unwrap()).unwrap();
        let js = emit(&program, OptimizationLevel::Advanced);
        let expected: usize = (1..=protocols).map(|n| 2 + 3 * n).sum();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n")
        );
        js.len()
    };
    let sizes: Vec<usize> = (1..=4).map(size).collect();
    let added: Vec<usize> = sizes.windows(2).map(|pair| pair[1] - pair[0]).collect();
    println!("bytes per extra two-method protocol: {added:?}");
    assert!(added.iter().all(|&bytes| bytes <= 350), "{added:?}");
}

/// The chunked build of `program` at `Advanced` and its size report.
fn advanced_chunks(program: &Program) -> (ChunkedOutput, ChunkedRetention) {
    let prepared = prepare_application(
        program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let (output, report) = prepared.emit_chunks_reported();
    (
        output,
        report.expect("an `Advanced` build of several chunks has a size report"),
    )
}

/// light-cljs-280 (docs/code-splitting.md, "Phase 3"): at `advanced` each
/// file of the `features` fixture's chunked build stays under its ceiling in
/// [`FEATURES_CHUNK_CEILINGS`], and the build writes exactly those files.
#[test]
fn features_chunk_sizes_stay_under_their_ceilings() {
    let fixture = NAMESPACE_FIXTURES
        .iter()
        .find(|fixture| fixture.name == "features")
        .unwrap();
    let (output, report) = advanced_chunks(&namespace_program(fixture));
    let mut table = format!("{:<28} {:>20}\n", "chunk", "bytes / ceiling");
    let mut exceeded = Vec::new();
    for (file, chunk) in output.files.iter().zip(&report.chunks) {
        assert_eq!(chunk.bytes, file.source.len(), "{}", file.name);
        let ceiling = FEATURES_CHUNK_CEILINGS
            .iter()
            .find(|(name, _)| *name == file.name)
            .map(|&(_, ceiling)| ceiling);
        let size = file.source.len();
        let mark = if ceiling.is_none_or(|ceiling| size > ceiling) {
            exceeded.push(file.name.clone());
            '!'
        } else {
            ' '
        };
        let cell = format!(
            "{} / {}",
            thousands(size),
            ceiling.map_or("none".to_owned(), thousands)
        );
        writeln!(table, "{:<28} {cell:>19}{mark}", file.name).unwrap();
    }
    println!("{table}");
    assert!(
        exceeded.is_empty(),
        "a chunk of the features example grew past its ratchet ceiling or has none: {}; \
         shrink it or raise the ceiling with a reason (marked !):\n{table}",
        exceeded.join(", ")
    );
    assert_eq!(
        output
            .files
            .iter()
            .map(|file| file.name.as_str())
            .collect::<Vec<_>>(),
        FEATURES_CHUNK_CEILINGS
            .iter()
            .map(|&(name, _)| name)
            .collect::<Vec<_>>()
    );
}

/// The bytes of startup's chunk loading in examples/features at `advanced`,
/// which the Goal's startup target does not count by the user's decision of
/// 2026-10-20 (docs/code-splitting.md, "Open questions", "Startup's chunk
/// loading"): the runtime's `Loadable` type (255), the `lazy$` helper with
/// its binding (249; `chunks::terse_chunks_write_their_runtime_units_at_module_level`
/// bounds it), the two sites' `lazy$` calls and import thunks (97 more than
/// the `nil`s the application without its features has), and
/// `light.lazy/Loadable`'s definition (11), measured by light-cljs-353 on
/// master `8221e3b`. The application without its features cannot need any
/// of it, so the startup check takes it out before comparing.
const FEATURES_STARTUP_LOADER: usize = 612;

/// How many bytes the startup chunk of examples/features may exceed the
/// application without its features by at `advanced`, not counting the
/// units startup holds only for the features or [`FEATURES_STARTUP_LOADER`].
/// Measured 172 on master `030e618`: the export lines through which the
/// features read startup's runtime (96) and the longer names of a program
/// that holds its features. Rounded up to the next 100 bytes, under the
/// Goal's 200 bytes per `loadable` (400 here). The starting slack of
/// light-cljs-280, the larger of 10% and 1,000 bytes, was 1,500 when chunk
/// loading still counted (1,487 bytes over, before light-cljs-353).
const FEATURES_STARTUP_SLACK: usize = 200;

/// The analyzed program of entry `app.main` under the source root `root`.
fn root_program(root: &Path) -> Program {
    analyze_namespace_entry(
        "app.main",
        &[root.to_path_buf()],
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{}: {error}", root.display()))
}

/// `text` with each `light.lazy/loadable` form, `(lazy/loadable ...)` or
/// `(light.lazy/loadable ...)`, replaced by `nil`.
fn without_loadables(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = ["(lazy/loadable ", "(light.lazy/loadable "]
        .iter()
        .filter_map(|form| rest.find(form))
        .min()
    {
        out.push_str(&rest[..start]);
        let mut depth = 0;
        let end = rest[start..]
            .char_indices()
            .find_map(|(at, c)| {
                match c {
                    '(' => depth += 1,
                    ')' => depth -= 1,
                    _ => {}
                }
                (depth == 0).then_some(start + at + 1)
            })
            .expect("a closed loadable form");
        out.push_str("nil");
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

/// Write the sources under `from` into `to` without the files `removed`,
/// and with each loadable form `nil`, as `bb benchmarks/features.bb`
/// generates its variants.
fn copy_without(from: &Path, to: &Path, removed: &[PathBuf]) {
    for entry in fs::read_dir(from).unwrap() {
        let path = entry.unwrap().path();
        let target = to.join(path.file_name().unwrap());
        if path.is_dir() {
            fs::create_dir_all(&target).unwrap();
            copy_without(&path, &target, removed);
        } else if path
            .extension()
            .is_some_and(|extension| extension == "cljs")
            && !removed.contains(&path)
        {
            let text = fs::read_to_string(&path).unwrap();
            fs::write(&target, without_loadables(&text)).unwrap();
        }
    }
}

/// light-cljs-280 (docs/code-splitting.md, "Phase 3", tests): the startup
/// chunk of examples/features at `advanced` is the application without its
/// features, generated from the sources with its loadables `nil` and the
/// feature namespaces absent, within [`FEATURES_STARTUP_SLACK`], once the
/// units startup holds only for the features are taken out, those the size
/// report attributes to another chunk's roots (runtime units that share a
/// binding with a unit startup needs), and the chunk loading the Goal does not
/// count, [`FEATURES_STARTUP_LOADER`].
#[test]
fn features_startup_is_the_application_without_its_features_within_its_slack() {
    let example = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/features");
    let program = root_program(&example);
    let removed = program
        .lazy_namespaces
        .iter()
        .map(|namespace| {
            example.join(format!(
                "{}.cljs",
                namespace.replace('.', "/").replace('-', "_")
            ))
        })
        .collect::<Vec<_>>();
    assert_eq!(removed.len(), 2, "{removed:?}");
    let dir = TempDir::new("features-without-features");
    copy_without(&example, &dir, &removed);
    let lower = root_program(&dir);
    assert!(lower.lazy_namespaces.is_empty());
    let lower = prepare_application(&lower, ApplicationOptions::new(OptimizationLevel::Advanced))
        .emit_esm()
        .len();
    let (output, report) = advanced_chunks(&program);
    let startup = output.startup().source.len();
    let held = report.chunks[0]
        .units
        .iter()
        .filter(|unit| unit.root.is_some_and(|(chunk, _)| chunk != 0))
        .map(|unit| unit.bytes)
        .sum::<usize>();
    let counted = startup - held - FEATURES_STARTUP_LOADER;
    println!(
        "startup {startup} bytes, {held} of them units only the features need and \
         {FEATURES_STARTUP_LOADER} chunk loading: {counted}; without its features {lower}; \
         slack {FEATURES_STARTUP_SLACK}"
    );
    assert!(
        counted <= lower + FEATURES_STARTUP_SLACK,
        "startup is {startup} bytes, {held} of them units only the features need and \
         {FEATURES_STARTUP_LOADER} chunk loading, so {counted} against {lower} for the \
         application without its features: {} over, slack {FEATURES_STARTUP_SLACK}",
        counted.saturating_sub(lower)
    );
}
