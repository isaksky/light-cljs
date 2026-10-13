//! Generated multi-namespace programs with known-dead code, checked by
//! metamorphic properties across optimization levels ("Generated programs" in
//! docs/advanced-optimization.md). The generator is `namespace_program` in
//! tests/support/generated.rs; each of its twelve fixed seeds is one test here
//! (`seed_00` ... `seed_11`).
//!
//! Each seed yields a program P (2-6 namespaces), tagged garbage G that is dead
//! by construction, and escape edges that make some garbage items live. Four
//! variants compile through `analyze_namespace_entry` (the `--entry` path) from
//! a temporary source root at every level and run in one Node process through
//! the shared outcome protocol (tests/support/execution.js): P, P + G,
//! P + G + escapes, and P with its collection literals built (property 5).
//!
//! 1. Correctness: each variant's observable record (trace, returned value or
//!    error category, stdout) is identical at none, prepare and advanced, and
//!    P + G records exactly what P records.
//! 2. Dead code by construction: a chain P, P + G_T1, P + G_T1 + G_T2, ...,
//!    P + G adds one tag's garbage per step in `GarbageTag::ALL` order. Each
//!    step's advanced output is compared with the previous step's, and the
//!    tag's class (see [`Class`]) must equal its row in [`EXPECTED`].
//! 3. Escape preservation: in P + G + escapes, the code of every escaped item
//!    survives at every level, and property 1 holds.
//! 4. Prepare-level regression: `removed_definitions` and `specialized_calls`
//!    of `ApplicationStats` never decrease along the same chain.
//! 5. Constant pool (light-cljs-218): the generator's collection literals,
//!    repeated, nested, with metadata, partially constant, read through
//!    transients, `with-meta`, hashes and printing, can also be rendered with
//!    their first item behind `identity` (`Literals::Built`), which no level
//!    pools. P so rendered records exactly what P records, at every level.
//!
//! Failures print the seed, every namespace's source and the three output
//! sizes. Replay a seed with `LIGHT_CLJS_GENERATED_SEED=<u64>` (it runs in
//! `seed_00`); `LIGHT_CLJS_GENERATED_COUNT=<n>` spreads `n` seeds over the
//! twelve tests for longer local runs:
//! `LIGHT_CLJS_GENERATED_COUNT=240 cargo nextest run --test compiler generated_optimization::`.
//! `--no-capture` prints each seed's garbage table.
use super::generated::{GarbageTag, Literals, NamespaceProgram, SEEDS, namespace_program, seeds};
use super::optimization::Sources;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::{
    ApplicationOptions, ApplicationStats, MacroHost, OptimizationLevel, Program,
    prepare_application,
};
use std::{fmt::Write, fs, process::Command};

/// How much of a tag's garbage advanced output keeps, compared with the same
/// program without that tag's garbage.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Class {
    /// No sentinel survives and the normalized output grows by at most
    /// [`SLACK`] bytes: the property of the design.
    Removed,
    /// No sentinel survives, so the items' code is gone, but more than
    /// [`SLACK`] bytes remain (for example `null;` statements and binding names).
    Residue,
    /// The code of at least one item survives.
    Survives,
}
impl Class {
    fn name(self) -> &'static str {
        match self {
            Self::Removed => "removed",
            Self::Residue => "residue",
            Self::Survives => "survives",
        }
    }
}

/// The fixed constant by which adding a tag's garbage may grow the normalized
/// advanced output and still count as removed.
const SLACK: isize = 0;

/// The expected-survivor list: what advanced output keeps today of each tag's
/// garbage, and what removes the rest. Every seed checks every tag exactly, so
/// a change that removes more fails here until its row is tightened: set the
/// class it now reaches (`Removed` is the goal) and update the note in the same
/// commit. A tag whose items stop sharing one class (some removed, some
/// surviving) should be split in the generator instead of loosened here.
const EXPECTED: [(GarbageTag, Class, &str); 7] = [
    (
        GarbageTag::UnreferencedNamespace,
        Class::Removed,
        "without reflection advanced output registers no namespace and drops `null;` (206)",
    ),
    (
        GarbageTag::UnusedLiteralDef,
        Class::Removed,
        "prepare removes the def; advanced drops its `null;` and lists no binding (206)",
    ),
    (
        GarbageTag::UnusedPureDef,
        Class::Removed,
        "advanced removes the def (213), drops its `null;` and lists no binding (206)",
    ),
    (
        GarbageTag::UnusedDefn,
        Class::Removed,
        "prepare removes the defn; advanced drops its `null;` and lists no binding (206)",
    ),
    (
        GarbageTag::UnusedKeyword,
        Class::Removed,
        "removed with its definition; keyword interning must not resurrect it (208, 218)",
    ),
    (
        GarbageTag::UninstantiatedType,
        Class::Survives,
        "types are always retained until rapid type analysis (211)",
    ),
    (
        GarbageTag::UncalledProtocolMethod,
        Class::Survives,
        "implementations are always installed until rapid type analysis (211)",
    ),
];

fn expected(tag: GarbageTag) -> (Class, &'static str) {
    let (_, class, note) = EXPECTED.iter().find(|row| row.0 == tag).unwrap();
    (*class, note)
}

const VARIANTS: [&str; 3] = ["program", "garbage", "escapes"];

/// One variant's sources, and its emitted JavaScript and preparation
/// statistics per level in `OptimizationLevel::ALL` order.
struct Compiled {
    variant: &'static str,
    sources: Vec<(String, String)>,
    js: [String; 3],
    stats: [ApplicationStats; 3],
}

/// Load the program's entry namespace and its dependencies from a temporary
/// source root, as `--compile --entry` does.
fn analyze(program: &NamespaceProgram, sources: &[(String, String)]) -> Program {
    let root = Sources::new();
    for (path, source) in sources {
        root.write(path, source);
    }
    analyze_namespace_entry(
        &program.entry,
        std::slice::from_ref(&root.0),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| {
        panic!(
            "seed {}: generated program does not compile: {error}\n{}",
            program.seed,
            listing(sources)
        )
    })
}

fn compile(
    program: &NamespaceProgram,
    variant: &'static str,
    sources: Vec<(String, String)>,
) -> Compiled {
    let analyzed = analyze(program, &sources);
    let prepared = OptimizationLevel::ALL
        .map(|level| prepare_application(&analyzed, ApplicationOptions::new(level)));
    Compiled {
        variant,
        sources,
        js: prepared.each_ref().map(|prepared| prepared.emit_js()),
        stats: prepared.each_ref().map(|prepared| prepared.stats().clone()),
    }
}

fn listing(sources: &[(String, String)]) -> String {
    sources
        .iter()
        .map(|(path, source)| format!(";; {path}\n{source}"))
        .collect()
}

/// Seed, sizes and sources for a failure message.
fn context(program: &NamespaceProgram, compiled: &Compiled) -> String {
    let mut text = format!(
        "seed {} (replay with LIGHT_CLJS_GENERATED_SEED={}), variant {}, emitted bytes:",
        program.seed, program.seed, compiled.variant
    );
    for (level, js) in OptimizationLevel::ALL.iter().zip(&compiled.js) {
        write!(text, " {level} {}", js.len()).unwrap();
    }
    text.push('\n');
    text.push_str(&listing(&compiled.sources));
    text
}

fn json_string(value: &str) -> String {
    let mut out = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => write!(out, "\\u{:04x}", c as u32).unwrap(),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn case_id(variant: &str, level: OptimizationLevel) -> String {
    format!("{variant}-{level}")
}

/// Run every variant at every level in one Node process with the shared
/// outcome protocol and return its stdout: one JSON record per case.
fn run(compiled: &[&Compiled]) -> Result<String, String> {
    let mut script = String::from("globalThis.lightCases = [\n");
    for variant in compiled {
        for js in &variant.js {
            // Generated code observes through js/lightObserve; bind it to this
            // case's observer before the program's namespaces load.
            script.push_str("(observe) => { globalThis.lightObserve = observe; return ");
            script.push_str(js);
            script.push_str("; },\n");
        }
    }
    script.push_str("];\nglobalThis.lightCaseMetadata = [\n");
    for variant in compiled {
        for level in OptimizationLevel::ALL {
            let id = json_string(&case_id(variant.variant, level));
            writeln!(script, "[{id}, {id}],").unwrap();
        }
    }
    script.push_str("];\n");
    script.push_str(include_str!("../support/execution.js"));
    let dir = Sources::new();
    let path = dir.0.join("cases.js");
    fs::write(&path, script).unwrap();
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .arg(&path)
        .output()
        .expect("generated optimization tests require Node");
    if !output.status.success() {
        return Err(format!(
            "outcome harness failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }
    Ok(String::from_utf8(output.stdout).unwrap())
}

/// The observable part of each case's record, in `compiled` and level order.
fn records<'a>(output: &'a str, compiled: &[&Compiled]) -> Result<Vec<&'a str>, String> {
    let lines: Vec<_> = output.lines().collect();
    let ids: Vec<_> = compiled
        .iter()
        .flat_map(|variant| OptimizationLevel::ALL.map(|level| case_id(variant.variant, level)))
        .collect();
    if lines.len() != ids.len() {
        return Err(format!(
            "expected {} outcome records, got {}:\n{output}",
            ids.len(),
            lines.len()
        ));
    }
    ids.iter()
        .zip(lines)
        .map(|(id, line)| {
            line.strip_prefix(&format!("{{\"id\":{},", json_string(id)))
                .ok_or_else(|| format!("missing or reordered record for {id}: {line}"))
        })
        .collect()
}

/// Property 1 for one variant: the record at every level equals `None`'s, and
/// the outcome is the one the generator intended.
fn agree(records: &[&str], throws: bool) -> Result<(), String> {
    let outcome = if throws {
        "\"outcome\":{\"status\":\"threw\",\"error\":{\"category\":\"error\"}},"
    } else {
        "\"outcome\":{\"status\":\"returned\","
    };
    if !records[0].starts_with(outcome) {
        return Err(format!(
            "unexpected outcome at none, expected {outcome}\n{}",
            records[0]
        ));
    }
    for (level, record) in OptimizationLevel::ALL.iter().zip(records).skip(1) {
        if *record != records[0] {
            return Err(format!(
                "observable record at {level} differs from none:\nnone: {}\n{level}: {record}",
                records[0]
            ));
        }
    }
    Ok(())
}

/// The composite entries (collections, `with-meta`, `#uuid`) of the
/// `Advanced` constant table in `js`: every table entry that is not a keyword
/// or symbol.
fn pool_composites(js: &str) -> usize {
    js.lines()
        .filter(|line| {
            ["  const k", "  let c"].iter().any(|prefix| {
                line.strip_prefix(prefix)
                    .is_some_and(|rest| rest.starts_with(|c: char| c.is_ascii_digit()))
            }) && !line.contains("= rt$keyword(")
                && !line.contains("= rt$symbol(")
        })
        .count()
}

/// Bytes of `js` without the digits inside identifiers, so outputs that differ
/// only in compiler numbering (`g12`, `l3`, `r4_0`, `__light2_`) have the same
/// size. Digits of numeric literals count.
fn normalized_len(js: &str) -> usize {
    let mut length = 0;
    let mut identifier = false;
    for byte in js.bytes() {
        if byte.is_ascii_digit() {
            if !identifier {
                length += 1;
            }
        } else {
            identifier = byte.is_ascii_alphabetic() || byte == b'_' || byte == b'$';
            length += 1;
        }
    }
    length
}

/// Property 2 for one tag: which of its sentinels `with_garbage` keeps, and its
/// normalized size over `without`.
struct TagRow {
    tag: GarbageTag,
    items: usize,
    survivors: Vec<String>,
    bytes: isize,
    class: Class,
}

fn classify(
    program: &NamespaceProgram,
    tag: GarbageTag,
    without: &str,
    with_garbage: &str,
) -> TagRow {
    let items: Vec<_> = program
        .garbage
        .iter()
        .filter(|item| item.tag == tag)
        .collect();
    let survivors: Vec<_> = items
        .iter()
        .filter(|item| with_garbage.contains(&item.sentinel))
        .map(|item| item.sentinel.clone())
        .collect();
    let bytes = normalized_len(with_garbage) as isize - normalized_len(without) as isize;
    let class = if !survivors.is_empty() {
        Class::Survives
    } else if bytes > SLACK {
        Class::Residue
    } else {
        Class::Removed
    };
    TagRow {
        tag,
        items: items.len(),
        survivors,
        bytes,
        class,
    }
}

fn table(program: &NamespaceProgram, rows: &[TagRow], prefixes: &[ApplicationStats]) -> String {
    let escaped = program.garbage.iter().filter(|item| item.escape.is_some());
    let mut text = format!(
        "seed {}: entry {}, {} garbage items, escaped:",
        program.seed,
        program.entry,
        program.garbage.len()
    );
    for item in escaped {
        write!(
            text,
            " {} ({}, {})",
            item.sentinel,
            item.tag.name(),
            item.escape.unwrap().name()
        )
        .unwrap();
    }
    writeln!(
        text,
        "\n{:<26} {:>5} {:>8} {:<9} {:<9} survived",
        "garbage tag", "items", "+bytes", "class", "expected"
    )
    .unwrap();
    for row in rows {
        writeln!(
            text,
            "{:<26} {:>5} {:>+8} {:<9} {:<9} {}",
            row.tag.name(),
            row.items,
            row.bytes,
            row.class.name(),
            expected(row.tag).0.name(),
            if row.survivors.is_empty() {
                "-".into()
            } else {
                row.survivors.join(" ")
            }
        )
        .unwrap();
    }
    let removed: Vec<_> = prefixes
        .iter()
        .map(|stats| stats.removed_definitions.to_string())
        .collect();
    let specialized: Vec<_> = prefixes
        .iter()
        .map(|stats| stats.specialized_calls.to_string())
        .collect();
    writeln!(
        text,
        "prepare from P through each tag's garbage: removed definitions {}; specialized calls {}",
        removed.join(" "),
        specialized.join(" ")
    )
    .unwrap();
    text
}

fn check_seed(seed: u64) {
    let program = namespace_program(seed);
    let compiled: Vec<_> = VARIANTS
        .iter()
        .map(|&variant| {
            let garbage = variant != "program";
            let sources = program.sources(&|_| garbage, variant == "escapes");
            compile(&program, variant, sources)
        })
        .collect();
    // P with every collection literal built at its site, for property 5.
    let built = compile(
        &program,
        "built",
        program.sources_with(&|_| false, false, Literals::Built),
    );
    let all: Vec<_> = compiled.iter().chain([&built]).collect();
    let [p, pg, escapes] = [&compiled[0], &compiled[1], &compiled[2]];

    // Properties 1, 3 and 5: one Node run for every variant and level.
    let output =
        run(&all).unwrap_or_else(|error| panic!("{error}\n{}", context(&program, escapes)));
    let records = records(&output, &all)
        .unwrap_or_else(|error| panic!("{error}\n{}", context(&program, escapes)));
    for (variant, records) in all.iter().zip(records.chunks(3)) {
        agree(records, program.throws)
            .unwrap_or_else(|error| panic!("property 1: {error}\n{}", context(&program, variant)));
    }
    assert_eq!(
        records[0],
        records[3],
        "property 1: dead garbage changed the observable record\n{}",
        context(&program, pg)
    );
    for (level, (pooled, built_record)) in OptimizationLevel::ALL
        .iter()
        .zip(records[..3].iter().zip(&records[9..]))
    {
        assert_eq!(
            pooled,
            built_record,
            "property 5: constant literals behave differently from built ones at {level}\n{}\n{}",
            context(&program, p),
            context(&program, &built)
        );
    }
    assert!(
        pool_composites(&p.js[2]) >= pool_composites(&built.js[2]),
        "property 5: the built rendering pools more than the constant one\n{}",
        context(&program, &built)
    );
    for item in program.garbage.iter().filter(|item| item.escape.is_some()) {
        for (level, js) in OptimizationLevel::ALL.iter().zip(&escapes.js) {
            assert!(
                js.contains(&item.sentinel),
                "property 3: {} ({}, reachable through {}) was removed at {level}\n{}",
                item.sentinel,
                item.tag.name(),
                item.escape.unwrap().name(),
                context(&program, escapes)
            );
        }
    }

    // Properties 2 and 4 along the chain P, P + G_T1, P + G_T1 + G_T2, ...,
    // P + G, which adds one tag's garbage per step in `GarbageTag::ALL` order.
    // Each step's advanced output is compared with the previous step's.
    let mut previous = p.js[2].clone();
    let mut prefixes = vec![p.stats[1].clone()];
    let mut rows = Vec::new();
    let mut included = Vec::new();
    for tag in GarbageTag::ALL {
        if program.garbage.iter().all(|item| item.tag != tag) {
            continue;
        }
        included.push(tag);
        let (advanced, stats) = if program
            .garbage
            .iter()
            .all(|item| included.contains(&item.tag))
        {
            (pg.js[2].clone(), pg.stats[1].clone())
        } else {
            let sources = program.sources(
                &|index| included.contains(&program.garbage[index].tag),
                false,
            );
            let analyzed = analyze(&program, &sources);
            let prepare = |level| prepare_application(&analyzed, ApplicationOptions::new(level));
            (
                prepare(OptimizationLevel::Advanced).emit_js(),
                prepare(OptimizationLevel::Prepare).stats().clone(),
            )
        };
        rows.push(classify(&program, tag, &previous, &advanced));
        prefixes.push(stats);
        previous = advanced;
    }

    let report = table(&program, &rows, &prefixes);
    println!(
        "{report}constant pool composites at advanced: {} constant, {} built",
        pool_composites(&p.js[2]),
        pool_composites(&built.js[2])
    );
    for pair in prefixes.windows(2) {
        assert!(
            pair[1].removed_definitions >= pair[0].removed_definitions
                && pair[1].specialized_calls >= pair[0].specialized_calls,
            "property 4: prepare statistics decreased as garbage grew\n{report}\n{}",
            context(&program, pg)
        );
    }
    let mismatches: Vec<_> = rows
        .iter()
        .filter_map(|row| {
            let (class, note) = expected(row.tag);
            (row.class != class).then(|| {
                let advice = if row.class < class {
                    "more is removed now: tighten its EXPECTED row"
                } else {
                    "more survives than before: a regression"
                };
                format!(
                    "{} ({}): expected {} ({note}), got {}; {advice}",
                    row.tag.name(),
                    row.tag.reason(),
                    class.name(),
                    row.class.name()
                )
            })
        })
        .collect();
    assert!(
        mismatches.is_empty(),
        "property 2: garbage classes differ from EXPECTED:\n{}\n{report}\n{}",
        mismatches.join("\n"),
        context(&program, pg)
    );
}

/// Run the selected seeds at positions `index`, `index + 12`, ...
fn check_seeds(index: usize) {
    for seed in seeds().into_iter().skip(index).step_by(SEEDS.len()) {
        check_seed(seed);
    }
}

row_tests!(check_seeds;
    seed_00(0), seed_01(1), seed_02(2), seed_03(3), seed_04(4), seed_05(5),
    seed_06(6), seed_07(7), seed_08(8), seed_09(9), seed_10(10), seed_11(11),
);

#[test]
fn checks_reject_changed_records_surviving_garbage_and_residue() {
    let program = namespace_program(SEEDS[0]);
    let compiled = compile(&program, "program", program.sources(&|_| false, false));
    let all = [&compiled];
    let output = run(&all).unwrap();
    let observed = records(&output, &all).unwrap();
    agree(&observed, program.throws).unwrap();
    // A changed trace, outcome or stdout at one level, or a lost record, fails.
    let mut changed = observed.clone();
    let tampered = changed[2].replacen("\"trace\":[", "\"trace\":[\"extra\",", 1);
    changed[2] = &tampered;
    assert!(agree(&changed, program.throws).is_err());
    let mut changed = observed.clone();
    let tampered = changed[1].replacen("\"stdout\":\"", "\"stdout\":\"x", 1);
    changed[1] = &tampered;
    assert!(agree(&changed, program.throws).is_err());
    assert!(agree(&observed, !program.throws).is_err());
    let truncated: String = output
        .lines()
        .take(2)
        .map(|line| format!("{line}\n"))
        .collect();
    assert!(records(&truncated, &all).is_err());
    // A surviving sentinel and leftover bytes change the class.
    let item = &program.garbage[0];
    let advanced = &compiled.js[2];
    assert_eq!(
        classify(&program, item.tag, advanced, advanced).class,
        Class::Removed
    );
    let survived = format!("{advanced}\"{}\";", item.sentinel);
    let row = classify(&program, item.tag, advanced, &survived);
    assert_eq!((row.class, row.survivors.len()), (Class::Survives, 1));
    let residue = format!("{advanced}null;");
    assert_eq!(
        classify(&program, item.tag, advanced, &residue).class,
        Class::Residue
    );
    // Renumbered compiler names alone are not residue.
    assert_eq!(
        normalized_len("g1 = l22 + 3;"),
        normalized_len("g10 = l3 + 3;")
    );
    assert_ne!(normalized_len("g1 = 3;"), normalized_len("g1 = 30;"));
}

#[test]
fn garbage_never_changes_the_program_text_and_every_tag_occurs() {
    for seed in SEEDS {
        let program = namespace_program(seed);
        let without = program.sources(&|_| false, false);
        assert_eq!(without, namespace_program(seed).sources(&|_| false, false));
        // Escape edges need their garbage, and no garbage text leaks into P.
        assert_eq!(without, program.sources(&|_| false, true));
        // Every program reads collection literals, which property 5 renders
        // both ways.
        assert_ne!(
            without,
            program.sources_with(&|_| false, false, Literals::Built),
            "seed {seed}: no collection literal"
        );
        for (_, source) in &without {
            assert!(!source.contains("GARBAGE-") && !source.contains("dead"));
        }
        let everything = listing(&program.sources(&|_| true, true));
        for tag in GarbageTag::ALL {
            assert!(
                program.garbage.iter().any(|item| item.tag == tag),
                "seed {seed}: {tag:?}"
            );
        }
        for item in &program.garbage {
            assert!(
                everything.contains(&item.sentinel),
                "seed {seed}: {}",
                item.sentinel
            );
        }
        assert!(program.garbage.iter().any(|item| item.escape.is_some()));
    }
}
