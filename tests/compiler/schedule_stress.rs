//! Determinism under perturbed scheduling (docs/compiler-concurrency.md,
//! "As built (light-cljs-342)"): the macro fixtures of `macros.rs`,
//! `compile_time_units.rs` and `deferred.rs`, a mixed fixture with an
//! affinity namespace between pooled ones, and liveness and failure
//! fixtures, built at one, two, four and eight slots under the test-only
//! [`Perturbation`] (randomized delays at job boundaries, random batches
//! and steal victims, a forced assignment, small admission bounds), each build
//! checked against the unperturbed one-slot build: JavaScript, spans,
//! source maps, warnings, errors and logical work counts, with the entry
//! value and initialization order from running it.
//!
//! The run is bounded: three seeds, or `LIGHT_CLJS_STRESS_SEEDS`, and a
//! wall-clock cap per test, `LIGHT_CLJS_STRESS_SECONDS` (30 by default),
//! checked after each seed, which stops further seeds with a note; each
//! fixture's first seed always runs whole. With the default three seeds a
//! test takes 0.1 to 20 s at moderate load and the cap is not reached. A
//! failure prints its seed, slots, mode and the assignment the pool
//! logged; `LIGHT_CLJS_STRESS_SEEDS=12 LIGHT_CLJS_STRESS_SECONDS=600`
//! runs a longer stress.
//!
//! The hook exists in debug builds only (`main.rs` leaves this suite out
//! of release test builds), and QuickJS-only builds, which have no pool,
//! skip each test with a note.
use crate::macro_free_units::{Built, Sources, build_configured, execute};
use light_cljs::MacroHost;
use light_cljs::scheduler::{Limits, Perturbation};
use std::time::{Duration, Instant};

const SLOTS: [usize; 4] = [1, 2, 4, 8];

/// Whether this build has no pool to perturb, saying so.
fn skipped() -> bool {
    if cfg!(feature = "v8-macros") {
        return false;
    }
    eprintln!(
        "skipped: QuickJS-only builds analyze every unit on the calling thread, with no pool to perturb"
    );
    true
}

/// The seeds the stress runs: 1 to 3, or to `LIGHT_CLJS_STRESS_SEEDS`.
fn seeds() -> Vec<u64> {
    let count = std::env::var("LIGHT_CLJS_STRESS_SEEDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(3u64);
    (1..=count.max(1)).collect()
}

/// The wall-clock cap of one test, checked after each seed.
fn budget() -> Duration {
    let seconds = std::env::var("LIGHT_CLJS_STRESS_SECONDS")
        .ok()
        .and_then(|n| n.parse().ok())
        .unwrap_or(30u64);
    Duration::from_secs(seconds)
}

thread_local! {
    /// When this test's first seeded round started: the cap is the test's.
    static STARTED: Instant = Instant::now();
}

/// Run `round` for each seed until the test's cap is reached; the first
/// seed always runs.
fn for_seeds(test: &str, mut round: impl FnMut(u64)) {
    let seeds = seeds();
    let started = STARTED.with(|started| *started);
    for (done, &seed) in seeds.iter().enumerate() {
        round(seed);
        if started.elapsed() > budget() && done + 1 < seeds.len() {
            eprintln!(
                "{test}: time cap of {:?} reached after {} of {} seeds",
                budget(),
                done + 1,
                seeds.len()
            );
            return;
        }
    }
}

/// How one perturbed build schedules.
#[derive(Clone, Copy, Debug)]
enum Mode {
    /// Delays, random batches from random places, random steals.
    Free,
    /// Every job on the worker the seed assigns it, in a random order.
    Fixed,
    /// Free, with one-job batches and one admitted job.
    Tight,
    /// Free, with one-job batches and seven admitted jobs, the most worker
    /// threads a build here has: every worker busy, the rest in the
    /// backlog.
    Saturated,
}

const MODES: [Mode; 4] = [Mode::Free, Mode::Fixed, Mode::Tight, Mode::Saturated];

fn perturbation(base: &Perturbation, seed: u64, mode: Mode) -> Perturbation {
    let admitted = match mode {
        Mode::Tight => Some(1),
        Mode::Saturated => Some(7),
        _ => None,
    };
    Perturbation {
        seed,
        fixed: matches!(mode, Mode::Fixed),
        limits: admitted.map(|admitted_jobs| Limits {
            batch_jobs: 1,
            admitted_jobs,
            ..Default::default()
        }),
        fresh_isolates: base.fresh_isolates,
        panic_in: base.panic_in.clone(),
        ..Default::default()
    }
}

/// A host whose macro memory limit gives eight slots eight isolates of
/// 32 MiB (`isolate_count`): the default 64 MiB gives two, so every slot
/// count above two would run one worker.
fn stress_host() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.memory_limit = 256 << 20;
    host
}

/// Sources, an entry and how to build it.
struct Fixture {
    sources: Sources,
    entry: &'static str,
    warning_mode: bool,
    blockers: Vec<String>,
    host: fn() -> MacroHost,
}

impl Fixture {
    fn new(entry: &'static str, files: &[(&str, &str)]) -> Fixture {
        let sources = Sources::new();
        for (path, source) in files {
            sources.write(path, source);
        }
        Fixture {
            sources,
            entry,
            warning_mode: false,
            blockers: Vec::new(),
            host: stress_host,
        }
    }

    /// One namespace, `app.main`, with `body` after its `ns` form.
    fn main(body: &str) -> Fixture {
        Fixture::new(
            "app.main",
            &[("app/main.cljs", &format!("(ns app.main)\n{body}"))],
        )
    }

    fn build(&self, slots: usize, perturbation: Option<Perturbation>) -> Built {
        build_configured(
            &self.sources,
            self.entry,
            slots,
            self.warning_mode,
            (self.host)(),
            &self.blockers,
            |recorders| recorders.perturbation = perturbation,
        )
    }
}

/// What must not depend on slots, scheduling or isolate history.
#[derive(Debug, PartialEq)]
struct Observed<'a> {
    /// The JavaScript, or what is compared of the error.
    output: Result<&'a str, String>,
    spans: &'a [(usize, std::ops::Range<usize>)],
    mapped: &'a Option<(String, light_cljs::source_map::SourceMapping)>,
    warnings: &'a [(String, String, usize, std::ops::Range<usize>)],
    macro_calls: usize,
    peak_macro_nodes: usize,
    namespaces: Vec<(&'a str, usize)>,
    /// Units registered, analyzed and prepared. (Units are found
    /// macro-free only with several slots.)
    units: [usize; 3],
}

/// What is compared of an error: all of it, unless a test says otherwise.
type ErrorKey = fn(&str) -> String;

fn whole(error: &str) -> String {
    error.to_owned()
}

fn observed(built: &Built, key: ErrorKey) -> Observed<'_> {
    let units = &built.stats.deferred_units;
    Observed {
        output: built.output.as_deref().map_err(|error| key(error)),
        spans: &built.spans,
        mapped: &built.mapped,
        warnings: &built.warnings,
        macro_calls: built.stats.macro_calls,
        peak_macro_nodes: built.stats.peak_macro_nodes,
        namespaces: (built.stats.namespaces.iter())
            .map(|n| (n.name.as_str(), n.macro_calls))
            .collect(),
        units: [units.registered, units.analyzed, units.prepared],
    }
}

/// The first line where two texts differ, for a failure message.
fn first_difference(expected: &str, actual: &str) -> String {
    let line = expected
        .lines()
        .zip(actual.lines())
        .position(|(a, b)| a != b)
        .unwrap_or_else(|| expected.lines().count().min(actual.lines().count()));
    format!(
        "line {}:\n  expected: {}\n  actual:   {}",
        line + 1,
        expected.lines().nth(line).unwrap_or("<end>"),
        actual.lines().nth(line).unwrap_or("<end>")
    )
}

/// Fail unless `built` observes what `oracle` does, naming the build.
fn check(oracle: &Built, built: &Built, label: &dyn Fn() -> String) {
    check_keyed(oracle, built, label, whole)
}

fn check_keyed(oracle: &Built, built: &Built, label: &dyn Fn() -> String, key: ErrorKey) {
    let (expected, actual) = (observed(oracle, key), observed(built, key));
    if expected == actual {
        return;
    }
    let detail = match (&oracle.output, &built.output) {
        (Ok(a), Ok(b)) if a != b => first_difference(a, b),
        (Err(a), Err(b)) if key(a) != key(b) => {
            format!("error\n  expected: {a}\n  actual:   {b}")
        }
        (a, b) if a.is_ok() != b.is_ok() => format!("expected {a:?}\nactual {b:?}"),
        _ if expected.spans != actual.spans => {
            let at = (expected.spans.iter().zip(actual.spans))
                .position(|(a, b)| a != b)
                .unwrap_or(expected.spans.len().min(actual.spans.len()));
            format!(
                "expression {at}: span {:?} != {:?}",
                expected.spans.get(at),
                actual.spans.get(at)
            )
        }
        _ if expected.mapped != actual.mapped => "source maps differ".to_owned(),
        _ if expected.warnings != actual.warnings => {
            format!(
                "warnings\n  expected: {:?}\n  actual:   {:?}",
                expected.warnings, actual.warnings
            )
        }
        _ => format!(
            "counts\n  expected: {:?} {:?} {} {}\n  actual:   {:?} {:?} {} {}",
            expected.units,
            expected.namespaces,
            expected.macro_calls,
            expected.peak_macro_nodes,
            actual.units,
            actual.namespaces,
            actual.macro_calls,
            actual.peak_macro_nodes
        ),
    };
    panic!("{} differs from the one-slot build: {detail}", label());
}

fn describe(slots: usize, mode: Mode, p: &Perturbation) -> String {
    format!(
        "seed {} at {slots} slots ({mode:?}{}), assignment {:?}",
        p.seed,
        if p.fresh_isolates {
            ", fresh isolates"
        } else {
            ""
        },
        p.assignment()
    )
}

/// Build `fixture` at every slot count, every mode and every seed, each
/// with `base`'s compiler hooks, against the one-slot build with them and
/// no scheduling perturbation; return that build.
fn stress_with(test: &str, fixture: &Fixture, base: Perturbation) -> Built {
    stress_keyed(test, fixture, base, whole)
}

/// [`stress_with`], comparing what `key` takes of an error.
fn stress_keyed(test: &str, fixture: &Fixture, base: Perturbation, key: ErrorKey) -> Built {
    let oracle = fixture.build(1, Some(base.clone()));
    for_seeds(test, |seed| {
        for slots in SLOTS {
            for mode in MODES {
                let p = perturbation(&base, seed, mode);
                let built = fixture.build(slots, Some(p.clone()));
                check_keyed(&oracle, &built, &|| describe(slots, mode, &p), key);
                note_workers(&p);
            }
        }
    });
    oracle
}

thread_local! {
    /// The most unit-pool workers one build of this test ran units on.
    static WIDEST: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn note_workers(p: &Perturbation) {
    let workers = p.assignment().len();
    WIDEST.with(|widest| widest.set(widest.get().max(workers)));
}

/// Fail unless some build of this test ran units on several workers, so
/// that its cross-worker cases did happen.
fn assert_several_workers(test: &str) {
    let widest = WIDEST.with(|widest| widest.get());
    assert!(widest >= 2, "{test}: no build ran units on two workers");
}

fn stress(test: &str, fixture: &Fixture) -> Built {
    stress_with(test, fixture, Perturbation::default())
}

/// The program's value, as `console.log` prints it.
fn value(built: &Built) -> String {
    let js = built.output.as_ref().expect("a program");
    execute(&format!("console.log({js});"))
}

#[test]
fn single_namespace_macro_fixtures_are_the_same_under_any_schedule() {
    if skipped() {
        return;
    }
    use crate::macros::*;
    for (name, source, expected) in [
        ("source order", SOURCE_ORDER_GENERATIONS, Some("true")),
        (
            "generated declarations",
            GENERATED_DECLARATIONS,
            Some("true"),
        ),
        ("nested demands", NESTED_DEMANDS, Some("true")),
        ("snapshots", SNAPSHOT_GENERATIONS, Some("true")),
        ("mutual recursion", MUTUAL_RECURSION, Some("true")),
        ("cached failure", FAILED_GENERATION, None),
        ("defonce", DEFONCE_PENDING, Some("true")),
        ("saved functions", SAVED_FUNCTION_CODE, Some("true")),
        (
            "extension frontier",
            FRONTIER_REPLAY,
            Some("[[1 10] [2 10] 2 10]"),
        ),
    ] {
        let fixture = Fixture::main(source);
        let oracle = stress(name, &fixture);
        if let Some(expected) = expected {
            assert_eq!(value(&oracle), format!("{expected}\n"), "{name}");
        }
    }
}

#[test]
fn namespace_macro_fixtures_keep_initialization_order_under_any_schedule() {
    if skipped() {
        return;
    }
    let helpers = Fixture::new("app", crate::macros::TRANSITIVE_HELPERS);
    let oracle = stress("transitive helpers", &helpers);
    assert_eq!(
        execute(oracle.output.as_ref().unwrap()),
        "leaf\nleft\nright\n42 42 42 7 1\n"
    );
    let deferred = Fixture::new("main", crate::deferred::DEFERRED_NAMESPACES);
    let oracle = stress("deferred namespaces", &deferred);
    assert_eq!(execute(oracle.output.as_ref().unwrap()), "false 9 7 9\n");
}

#[test]
fn compile_time_unit_and_deferred_value_fixtures_are_the_same_under_any_schedule() {
    if skipped() {
        return;
    }
    let mut body = String::new();
    let cases = crate::compile_time_units::REFLECTION_PROGRAMS
        .iter()
        .chain(crate::compile_time_units::COMPILE_TIME_EXPANSIONS);
    for (k, (source, _)) in cases.enumerate() {
        body.push_str(&format!(
            "(defn r{k} [] (try {source} (catch :default e (str \"threw \" (.-message e)))))\n"
        ));
    }
    body.push_str("(js/console.log (str *ns*) (r0) (r3))");
    let oracle = stress("compile-time units", &Fixture::main(&body));
    assert_eq!(
        execute(oracle.output.as_ref().unwrap()),
        "app.main app.main true\n"
    );
    let mut body = String::new();
    let effects = crate::deferred::DEFERRED_EFFECTS;
    for (k, (source, _)) in effects.iter().enumerate() {
        body.push_str(&format!("(def r{k} {source})\n"));
    }
    body.push_str("(js/console.log (js/JSON.stringify (array r0 r1 r2)))");
    let oracle = stress("deferred values", &Fixture::main(&body));
    let expected = effects.iter().map(|(_, json)| *json).collect::<Vec<_>>();
    assert_eq!(
        execute(oracle.output.as_ref().unwrap()),
        format!("[{}]\n", expected.join(","))
    );
}

/// Pure macros and helpers used by pooled namespaces around the counter.
const PURE: &str = r#"(ns app.pure)
(defn helper [x] (inc x))
(defmacro sq [x] `(* ~x ~x))
(defmacro bump [x] (helper x))
(defmacro named [] (let [s (gensym "v")] `(let [~s 1] (+ ~s 1))))"#;

fn safe_units(name: &str, requires: &str, count: usize) -> String {
    let mut body = format!("(ns {name} (:require [app.pure :refer [sq bump named]]{requires}))\n");
    for k in 0..count {
        body.push_str(&format!(
            "(defn f{k} [n] (+ (sq n) (bump {k}) (named)))\n(def v{k} (f{k} {k}))\n"
        ));
    }
    body.push_str(&format!(
        "(js/console.log \"{name}\" (+ {}))\n",
        (0..count)
            .map(|k| format!("v{k}"))
            .collect::<Vec<_>>()
            .join(" ")
    ));
    body
}

#[test]
fn safe_units_around_an_affinity_namespace_are_the_same_under_any_schedule() {
    if skipped() {
        return;
    }
    let mut files = crate::macros::COUNTER.to_vec();
    let first = safe_units("app.first", "", 12);
    let second = safe_units("app.second", " [app.first]", 12);
    files.push(("app/pure.cljs", PURE));
    files.push(("app/first.cljs", &first));
    files.push(("app/second.cljs", &second));
    files.push((
        "app/main.cljs",
        "(ns app.main (:require [app.first] [main] [app.second]))",
    ));
    let mut fixture = Fixture::new("app.main", &files);
    // `macros` is the counter's provider: `dep` and `main` keep the main
    // isolate, so `main` still prints `1 2`; the pure namespaces before
    // and after them go to the pool.
    fixture.blockers = vec!["macros".into()];
    let oracle = stress("mixed", &fixture);
    let sum = |count: usize| (0..count).map(|k| k * k + k + 1 + 2).sum::<usize>();
    assert_eq!(
        execute(oracle.output.as_ref().unwrap()),
        format!("app.first {}\n1 2\napp.second {}\n", sum(12), sum(12))
    );
    let pooled = fixture.build(4, None).stats.deferred_units;
    assert!(pooled.pooled >= 48, "{pooled:?}");
    assert_several_workers("mixed");
}

/// Several workers demand one helper's unit at once: one analyzes it and
/// the others wait for that analysis and share it.
#[test]
fn duplicate_demands_for_one_unit_from_several_workers_share_one_analysis() {
    if skipped() {
        return;
    }
    let mut body = String::from(
        "(defn shared-helper [x] (loop [i 0 acc x] (if (< i 2000) (recur (inc i) acc) (* 2 acc))))\n\
         (defmacro twice [x] (shared-helper x))\n",
    );
    for k in 0..24 {
        body.push_str(&format!("(defn f{k} [] (twice {k}))\n"));
    }
    body.push_str("(js/console.log (f0) (f23))");
    let fixture = Fixture::main(&body);
    let oracle = stress("duplicate demands", &fixture);
    assert_eq!(execute(oracle.output.as_ref().unwrap()), "0 46\n");
    assert_several_workers("duplicate demands");
}

/// An expansion limit crossed inside a helper that a unit's macro demanded
/// fails with the one-slot error, the helper's wrapped in the macro's,
/// whether the demanding unit's worker analyzed the helper, another worker
/// did as a job of its own, or one demanding it later did (light-cljs-377).
#[test]
fn limits_crossed_inside_demanded_helpers_fail_the_same_way_under_any_schedule() {
    if skipped() {
        return;
    }
    fn limited<const EXPANSIONS: usize>() -> MacroHost {
        let mut host = stress_host();
        host.limits.expansions = EXPANSIONS;
        host
    }
    // Limits that `parallel_diagnostics` finds crossed in the first, the
    // fourth and the last helper.
    for host in [
        limited::<29> as fn() -> MacroHost,
        limited::<47>,
        limited::<66>,
    ] {
        let sources = Sources::new();
        crate::parallel_diagnostics::helper_demands(&sources);
        let fixture = Fixture {
            sources,
            entry: "app.h",
            warning_mode: false,
            blockers: Vec::new(),
            host,
        };
        let oracle = stress("helper limits", &fixture);
        let error = oracle.output.unwrap_err();
        assert!(
            error.contains(": macro app.h/outer") && error.contains(": app.h/h"),
            "{error}"
        );
    }
    assert_several_workers("helper limits");
}

/// Warnings from units on several workers, and ordinary errors of two
/// failing units where the later fails first: the one-slot warnings in
/// their order and the earlier unit's error under any schedule.
#[test]
fn warnings_and_ordinary_errors_are_the_same_under_any_schedule() {
    if skipped() {
        return;
    }
    let macros = "(ns app.m) (defmacro call-missing [n] (list (symbol (str \"missing-\" n)) n)) (defn two [a b] [a b])";
    let mut files = vec![("app/m.cljs".to_owned(), macros.to_owned())];
    let mut requires = String::new();
    for n in 0..2 {
        let mut body = format!("(ns app.w{n} (:require [app.m :refer [call-missing two]]))\n");
        for k in 0..6 {
            body.push_str(&format!(
                "(defn f{k} [] [(call-missing {k}) (two 1) (+ 1 {k})])\n(def v{k} (when (f{k}) undeclared-{n}-{k}))\n"
            ));
        }
        files.push((format!("app/w{n}.cljs"), body));
        requires.push_str(&format!(" [app.w{n}]"));
    }
    files.push((
        "app/main.cljs".to_owned(),
        format!("(ns app.main (:require{requires})) (js/console.log app.w1/v1)"),
    ));
    let files: Vec<_> = files
        .iter()
        .map(|(p, s)| (p.as_str(), s.as_str()))
        .collect();
    let mut warned = Fixture::new("app.main", &files);
    warned.warning_mode = true;
    let oracle = stress("warnings", &warned);
    assert!(oracle.output.is_ok(), "{:?}", oracle.output);
    // Two warnings per `k` in each namespace, as parallel_diagnostics has.
    assert_eq!(oracle.warnings.len(), 2 * 6 * 2, "{:?}", oracle.warnings);
    assert_several_workers("warnings");
    let failing = Fixture::new(
        "app.e",
        &[
            (
                "app/m.cljs",
                "(ns app.m) (defmacro fast-fail [] (throw (js/Error. \"later failure\"))) \
                 (defmacro slow-fail [] (let [end (+ (js/Date.now) 40)] (loop [] (when (< (js/Date.now) end) (recur)))) \
                 (throw (js/Error. \"earlier failure\")))",
            ),
            (
                "app/e.cljs",
                "(ns app.e (:require [app.m :refer [slow-fail fast-fail]]))\n\
                 (defn f0 [] 0)\n(defn f1 [] (slow-fail))\n(defn f2 [] 2)\n(defn f3 [] (fast-fail))\n\
                 (defn f4 [] (fast-fail))\n(js/console.log (f0))",
            ),
        ],
    );
    let oracle = stress("ordinary errors", &failing);
    let error = oracle.output.unwrap_err();
    assert!(
        error.contains("earlier failure") && error.contains("app/e.cljs:3:"),
        "{error}"
    );
}

/// Units on two workers each demand a helper whose analysis demands the
/// other's, while the coordinator waits in its drain: the build fails with
/// the cycle at every slot count and schedule, at the first unit's
/// expansion. Which unit the cycle is entered from is the schedule's
/// choice, and the nested message records that path; that part is an open
/// question (docs/compiler-concurrency.md, "Open questions").
#[test]
fn a_cross_worker_cycle_fails_the_build_at_the_same_unit_under_any_schedule() {
    if skipped() {
        return;
    }
    let fixture = Fixture::main(
        "(declare a b)\n\
         (defmacro ma [] (a))\n\
         (defmacro mb [] (b))\n\
         (defn a [] (mb))\n\
         (defn b [] (ma))\n\
         (defn x [] (ma))\n\
         (defn y [] (mb))\n\
         (js/console.log (x) (y))",
    );
    // The error up to the failing expansion, and the units on the cycle.
    fn cycle(error: &str) -> String {
        let (outer, rest) = error
            .split_once(": macro app.main/mb: ")
            .unwrap_or((error, ""));
        let path = rest
            .split_once("compile dependency cycle: ")
            .map_or("", |(_, path)| path.split(';').next().unwrap_or(""));
        // A cycle met on one thread's stack is printed with the demands
        // that led to it; the cycle starts where its last unit first is.
        let path: Vec<_> = path.split(" -> ").collect();
        let last = path.last().copied().unwrap_or_default();
        let from = path.iter().position(|unit| *unit == last).unwrap_or(0);
        let mut units = path[from..].to_vec();
        units.sort();
        units.dedup();
        format!("{outer} / cycle of {units:?}")
    }
    let oracle = stress_keyed("cycle", &fixture, Perturbation::default(), cycle);
    let error = oracle.output.unwrap_err();
    assert!(
        error.contains("app/main.cljs:5:12: macro expansion: while expanding mb")
            && error.contains("compile dependency cycle: app.main/a -> app.main/b -> app.main/a"),
        "{error}"
    );
    assert_several_workers("cycle");
}

/// A Rust panic inside `__lightCompileUnit`, the callback a running
/// macro's nested demand enters, fails the unit whose macro ran, in
/// whichever isolate ran it: the build fails with the one-slot error at
/// every slot count and schedule, and the isolate is retired, not reused.
#[test]
fn a_panic_in_a_nested_demand_callback_fails_the_build_the_same_way_and_retires_the_isolate() {
    if skipped() {
        return;
    }
    let mut body = String::from(
        "(defn helper [x] (inc x))\n\
         (defmacro bumped [x] (helper x))\n",
    );
    for k in 0..10 {
        body.push_str(&format!("(defn f{k} [] (+ 1 {k}))\n"));
    }
    body.push_str("(defn g [] (bumped 1))\n");
    for k in 10..14 {
        body.push_str(&format!("(defn f{k} [] (+ 1 {k}))\n"));
    }
    body.push_str("(js/console.log (g))");
    let fixture = Fixture::main(&body);
    let base = Perturbation {
        panic_in: Some("app.main/helper".into()),
        ..Default::default()
    };
    let oracle = stress_with("callback panic", &fixture, base.clone());
    let error = oracle.output.unwrap_err();
    assert!(
        error.contains("app/main.cljs:14:12: macro expansion: while expanding bumped")
            && error.contains("compiler unit service panicked"),
        "{error}"
    );
    // With one worker taking every unit in the seed's order, a unit before
    // `g` runs after its isolate failed, in a new one.
    retired_for_some_seed(&fixture, &base);
}

/// Whether, with one worker taking every unit in a seed's random order,
/// some seed made a second isolate: a unit ran after the first failed.
fn retired_for_some_seed(fixture: &Fixture, base: &Perturbation) {
    let isolates: Vec<_> = (1..=12)
        .map(|seed| {
            let p = perturbation(base, seed, Mode::Fixed);
            let built = fixture.build(2, Some(p.clone()));
            assert!(built.output.is_err(), "seed {seed}");
            p.isolates.load(std::sync::atomic::Ordering::Relaxed)
        })
        .collect();
    assert!(
        isolates.iter().any(|&n| n >= 2),
        "no seed ran a unit after the failed isolate: {isolates:?}"
    );
}

/// A macro that holds `mb` MiB of JavaScript heap while it runs.
const HOG: &str = "(ns app.m) (defmacro hog [mb] (let [keep #js []] (dotimes [_ mb] (.push keep (.fill (js/Array. 131072) 1.5))) (.-length keep)))";

/// An isolate that runs out of memory is retired: the unit fails as with
/// one slot, and a unit after it gets a new isolate.
#[test]
fn an_isolate_out_of_memory_is_retired_and_the_build_fails_the_same_way() {
    if skipped() {
        return;
    }
    let mut body = String::from("(ns app.h (:require [app.m :refer [hog]]))\n");
    for k in 0..12 {
        let size = if k == 8 { 320 } else { 1 };
        body.push_str(&format!("(defn h{k} [] (hog {size}))\n"));
    }
    let mut fixture = Fixture::new("app.h", &[("app/m.cljs", HOG), ("app/h.cljs", &body)]);
    fixture.host = || {
        let mut host = MacroHost::new();
        host.evaluator_options.memory_limit = 256 << 20;
        host
    };
    let oracle = stress("out of memory", &fixture);
    let error = oracle.output.unwrap_err();
    assert!(
        error.contains("app/h.cljs:10:") && error.contains("memory limit"),
        "{error}"
    );
    retired_for_some_seed(&fixture, &Perturbation::default());
}

/// A unit that spins until the build's deadline is terminated by the
/// interrupt scheduler, and the build fails with the deadline under any
/// schedule. The deadline is the build's: which unit and form it is
/// reported at depends on scheduling and is not promised ("Semantics and
/// determinism", Limits). Under a random order a unit before the spinning
/// one may start after the deadline and fail with it first.
#[test]
fn an_isolate_past_the_deadline_fails_the_build_under_any_schedule() {
    if skipped() {
        return;
    }
    let fixture = {
        let mut fixture = Fixture::main(
            "(defmacro spin [] (loop [n 0] (recur (inc n))))\n\
             (defn before [] 1)\n\
             (def a (spin))\n\
             (defn b [] 2)\n\
             (defn c [] 3)",
        );
        fixture.host = || {
            let mut host = stress_host();
            host.evaluator_options.timeout = Duration::from_millis(300);
            host
        };
        fixture
    };
    let serial = fixture.build(1, None).output.unwrap_err();
    assert!(
        serial.contains("app/main.cljs:4:8") && serial.contains("deadline exceeded"),
        "{serial}"
    );
    // One seed in two modes: every build waits for the deadline.
    for slots in SLOTS {
        for mode in [Mode::Free, Mode::Fixed] {
            let p = perturbation(&Perturbation::default(), 1, mode);
            let error = fixture.build(slots, Some(p.clone())).output.unwrap_err();
            assert!(
                error.contains("deadline exceeded"),
                "{}: {error}",
                describe(slots, mode, &p)
            );
        }
    }
}

/// Source roots whose provider panics when asked for `app.boom`, from the
/// parse queue's workers.
struct PanickingRoots(light_cljs::source_provider::FileSourceProvider);

impl light_cljs::source_provider::SourceProvider for PanickingRoots {
    fn load(
        &self,
        request: &light_cljs::source_provider::LoadRequest,
    ) -> Result<
        Option<light_cljs::source_provider::SourceInput>,
        light_cljs::source_provider::LoadError,
    > {
        if request.namespace.name() == "app.boom".encode_utf16().collect::<Vec<_>>() {
            panic!("injected panic loading app.boom");
        }
        self.0.load(request)
    }
    fn concurrent(
        &self,
    ) -> Option<&(dyn light_cljs::source_provider::SourceProvider + Send + Sync)> {
        Some(self)
    }
}

/// A panic in a provider during source discovery, on a parse worker or
/// inline with one slot, fails the build with the same error under any
/// schedule; nothing waits for the job it stopped.
#[test]
fn a_panic_during_discovery_fails_the_build_the_same_way_under_any_schedule() {
    if skipped() {
        return;
    }
    let sources = Sources::new();
    sources
        .write("app/a.cljs", "(ns app.a (:require [app.c] [app.boom]))")
        .write("app/b.cljs", "(ns app.b (:require [app.c]))")
        .write("app/c.cljs", "(ns app.c)")
        .write("app/boom.cljs", "(ns app.boom)")
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.b] [app.a] [app.c]))",
        );
    let provider = PanickingRoots(light_cljs::source_provider::FileSourceProvider::new(vec![
        std::fs::canonicalize(sources.root()).unwrap(),
    ]));
    let build = |slots: usize, perturbation: Option<Perturbation>| {
        light_cljs::compiler::analyze_namespace_entry_recorded_with_provider(
            "app.main",
            &provider,
            &mut MacroHost::new(),
            Default::default(),
            &mut Vec::new(),
            light_cljs::compiler::EntryRecorders {
                slots: light_cljs::scheduler::Slots::new(slots).unwrap(),
                perturbation,
                ..Default::default()
            },
        )
        .map(|_| ())
        .map_err(|error| error.to_string())
    };
    let oracle = build(1, None);
    let error = oracle.clone().unwrap_err();
    assert!(
        error.contains("a compiler worker panicked: injected panic loading app.boom"),
        "{error}"
    );
    for_seeds("discovery panic", |seed| {
        for slots in SLOTS {
            for mode in MODES {
                let p = perturbation(&Perturbation::default(), seed, mode);
                assert_eq!(
                    build(slots, Some(p.clone())),
                    oracle,
                    "{}",
                    describe(slots, mode, &p)
                );
            }
        }
    });
}

/// Persistent worker isolates carry the macros, helpers and JIT state of
/// the units they ran before. Results, diagnostics and logical work counts
/// must not depend on that history: each fixture is built with a fresh
/// isolate per unit, with persistent isolates under several fixed
/// assignments, and with one worker running every unit in the seed's
/// order, all against the one-slot build. A unit whose result differs is
/// a bug, not a flake: the failure names the first difference and both
/// assignments.
#[test]
fn persistent_worker_isolates_match_a_fresh_isolate_per_unit_after_any_history() {
    if skipped() {
        return;
    }
    use crate::macros::*;
    let mut fixtures: Vec<(&str, Fixture)> = vec![
        ("nested demands", Fixture::main(NESTED_DEMANDS)),
        ("snapshots", Fixture::main(SNAPSHOT_GENERATIONS)),
        ("saved functions", Fixture::main(SAVED_FUNCTION_CODE)),
        ("extension frontier", Fixture::main(FRONTIER_REPLAY)),
        (
            "transitive helpers",
            Fixture::new("app", TRANSITIVE_HELPERS),
        ),
    ];
    let mut inventory = String::from(
        "(defmacro seen [] (contains? (:light-cljs/macros &env) 'app.main/later))\n\
         (defn before [] (seen))\n",
    );
    inventory.push_str("(defmacro later [] 1)\n");
    for k in 0..8 {
        inventory.push_str(&format!("(defn after{k} [] [(seen) (later)])\n"));
    }
    inventory.push_str("(js/console.log (before) (after0) (after7))");
    fixtures.push(("macro inventory", Fixture::main(&inventory)));
    for (name, fixture) in &fixtures {
        let oracle = fixture.build(1, None);
        for_seeds(name, |seed| {
            let fresh_base = Perturbation {
                fresh_isolates: true,
                ..Default::default()
            };
            let fresh = perturbation(&fresh_base, seed, Mode::Fixed);
            let fresh_built = fixture.build(4, Some(fresh.clone()));
            check(&oracle, &fresh_built, &|| {
                format!("{name}: {}", describe(4, Mode::Fixed, &fresh))
            });
            for (slots, persistent_seed) in [(4, seed), (4, seed + 100), (2, seed)] {
                let persistent =
                    perturbation(&Perturbation::default(), persistent_seed, Mode::Fixed);
                let built = fixture.build(slots, Some(persistent.clone()));
                check(&fresh_built, &built, &|| {
                    format!(
                        "{name}: persistent isolates, {}, against fresh isolates, {}",
                        describe(slots, Mode::Fixed, &persistent),
                        describe(4, Mode::Fixed, &fresh)
                    )
                });
                note_workers(&persistent);
            }
        });
    }
    assert_several_workers("worker history");
}
