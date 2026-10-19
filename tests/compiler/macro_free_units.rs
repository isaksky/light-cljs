//! Macro-free deferred units on the compiler's pool (docs/compiler-concurrency.md,
//! "As built (light-cljs-338)"): which units are found macro-free when they
//! are registered, and that a build with several slots, whose workers
//! analyze those units with no compile-time engine, emits the program,
//! warnings and errors of a build with one.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning, ExprKind};
use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
use light_cljs::scheduler::Slots;
use light_cljs::{CompileStats, MacroHost, emit_js};
use std::{fs, process::Command};

struct Sources(TempDir);

impl Sources {
    fn new() -> Self {
        Self(TempDir::new("cljs-macro-free"))
    }
    fn write(&self, path: &str, source: &str) -> &Self {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        self
    }
}

/// What a build made: its JavaScript or error, its warnings with their
/// spans and its stats.
struct Built {
    output: Result<String, String>,
    warnings: Vec<(String, String, usize, std::ops::Range<usize>)>,
    stats: CompileStats,
}

fn build(sources: &Sources, entry: &str, slots: usize, warning_mode: bool) -> Built {
    build_with(sources, entry, slots, warning_mode, MacroHost::new())
}

fn build_with(
    sources: &Sources,
    entry: &str,
    slots: usize,
    warning_mode: bool,
    mut host: MacroHost,
) -> Built {
    let mut warnings: Vec<AnalysisWarning> = Vec::new();
    let mut stats = CompileStats::new();
    let analyzed = analyze_namespace_entry_recorded(
        entry,
        std::slice::from_ref(sources.0.path()),
        &mut host,
        AnalysisOptions {
            warning_mode,
            ..Default::default()
        },
        &mut warnings,
        EntryRecorders {
            stats: Some(&mut stats),
            slots: Slots::new(slots).unwrap(),
            ..Default::default()
        },
    );
    Built {
        output: analyzed
            .map(|(program, _)| emit_js(&program))
            .map_err(|error| error.to_string()),
        warnings: warnings
            .into_iter()
            .map(|w| (w.kind.to_owned(), w.message, w.source_index, w.span))
            .collect(),
        stats,
    }
}

/// `count` macro-free units where several slots dispatch them: in native V8
/// builds; QuickJS-only builds analyze every unit on the calling thread.
fn pooled_count(count: usize) -> usize {
    if cfg!(feature = "v8-macros") {
        count
    } else {
        0
    }
}

fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", js])
        .output()
        .expect("compiler integration requires Node");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

/// Build `entry` at one, two and eight slots, several rounds each, and
/// check every build against the one-slot build; return that build.
fn same_at_every_slot_count(sources: &Sources, entry: &str, warning_mode: bool) -> Built {
    let serial = build(sources, entry, 1, warning_mode);
    for slots in [2, 8] {
        for _ in 0..3 {
            let pooled = build(sources, entry, slots, warning_mode);
            assert_eq!(pooled.output, serial.output, "{slots} slots");
            assert_eq!(pooled.warnings, serial.warnings, "{slots} slots");
            assert_eq!(
                pooled.stats.macro_calls, serial.stats.macro_calls,
                "{slots} slots"
            );
            assert_eq!(
                pooled.stats.peak_macro_nodes, serial.stats.peak_macro_nodes,
                "{slots} slots"
            );
        }
    }
    serial
}

const MACROS: &str = "(ns app.m) (defmacro mac [] 1) (defn twice [x] (* 2 x))";

#[test]
fn units_are_macro_free_only_when_no_form_can_reach_the_macro_host() {
    let sources = Sources::new();
    sources.write("app/m.cljs", MACROS).write(
        "app/a.cljs",
        r#"(ns app.a (:require [app.m :as am :refer [mac]]))
           (def a 1)
           (def b (fn* [x] x))
           (defn c [x] (let [y x] (loop [z y] z)))
           (def d (when true 1))
           (def e (mac))
           (def g (am/twice 1))
           (def h #(inc %))
           (defn i [] (def j 1))
           (def k (delay 1))
           (def l 'when)
           (defn helper [x] x)
           (def m (helper 1))
           (defmacro later [x] x)
           (def n (later 2))
           (def ^{:test (fn [] 1)} o 1)
           (js/console.log a (b 2) (c 3) d e g (h 4) (str l) n)"#,
    );
    let serial = build(&sources, "app.a", 1, false);
    // One slot dispatches nothing, so it judges nothing.
    assert_eq!(serial.stats.deferred_units.macro_free, 0);
    let pooled = build(&sources, "app.a", 4, false);
    // `a`, `b`, `c` (native `defn`, `let` and `loop`), `g` (a function of a
    // required alias), `twice`, `h`, `l` (quoted), `helper` and `m`, which calls it
    // before the file defines a macro; not `d` (a core macro), `e` (an
    // imported macro), `i` (a nested definition), `k` (an intrinsic),
    // `n` (a source macro) or `o` (a `:test` unit).
    assert_eq!(pooled.stats.deferred_units.macro_free, pooled_count(9));
    assert_eq!(pooled.output, serial.output);
    assert_eq!(execute(&serial.output.unwrap()), "1 2 3 1 1 2 5 when 2\n");
}

#[test]
fn several_slots_emit_the_one_slot_program_warnings_and_errors() {
    let sources = Sources::new();
    sources.write("app/m.cljs", MACROS);
    let mut requires = String::new();
    let mut uses = String::new();
    let write = |warning_mode: bool| {
        for n in 0..6 {
            let mut body = format!("(ns app.n{n} (:require [app.m :refer [mac]]))\n");
            for k in 0..12 {
                // Macro-free units, a warning in one of them, and units that
                // expand macros in between.
                body.push_str(&format!(
                "(defn f{k} [x] (let [[a b] [x {k}]] (+ a b {n})))\n(def v{k} (when (f{k} 1) (mac)))\n"
            ));
            }
            // An undeclared Var is an error without warnings.
            let callee = if warning_mode {
                format!("undefined-{n}")
            } else {
                "f0".into()
            };
            body.push_str(&format!("(def w (fn* [] ({callee} 1)))\n"));
            fs::write(sources.0.join(format!("app/n{n}.cljs")), body).unwrap();
        }
    };
    for n in 0..6 {
        requires.push_str(&format!(" [app.n{n}]"));
        uses.push_str(&format!(" (app.n{n}/f11 1) app.n{n}/v0"));
    }
    sources.write(
        "app/main.cljs",
        &format!("(ns app.main (:require{requires})) (js/console.log{uses})"),
    );
    for warning_mode in [false, true] {
        write(warning_mode);
        let serial = same_at_every_slot_count(&sources, "app.main", warning_mode);
        assert!(serial.output.is_ok(), "{:?}", serial.output.as_ref().err());
        assert_eq!(serial.warnings.is_empty(), !warning_mode);
        let pooled = build(&sources, "app.main", 8, warning_mode);
        assert_eq!(
            pooled.stats.deferred_units.macro_free,
            pooled_count(1 + 6 * 13)
        );
    }
    write(false);
    let serial = build(&sources, "app.main", 1, false);
    assert_eq!(
        execute(&serial.output.unwrap()),
        "12 1 13 1 14 1 15 1 16 1 17 1\n"
    );
    // A failing macro-free unit is reported where one slot reports it,
    // although units after it ran on workers.
    sources.write(
        "app/n3.cljs",
        "(ns app.n3) (defn f11 [x] x) (def v0 1) (def bad (fn* [] (recur 1 2))) (def later (fn* [] 1))",
    );
    let serial = same_at_every_slot_count(&sources, "app.main", true);
    let error = serial.output.unwrap_err();
    assert!(error.contains("app/n3.cljs"), "{error}");
}

#[test]
fn the_counter_fixture_prints_one_two_at_eight_slots() {
    // Its units expand a macro, so they drain on the coordinator in order.
    let sources = Sources::new();
    sources
        .write(
            "macros.cljs",
            "(ns macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))",
        )
        .write(
            "dep.cljs",
            "(ns dep (:require [macros :refer [next-value]])) (def value (next-value))",
        )
        .write(
            "main.cljs",
            "(ns main (:require [dep :as d] [macros :refer [next-value]])) (js/console.log d/value (next-value))",
        );
    for slots in [1, 8] {
        let built = build(&sources, "main", slots, false);
        assert_eq!(execute(&built.output.unwrap()), "1 2\n", "{slots} slots");
    }
}

#[test]
fn a_limit_crossed_in_a_macro_free_unit_fails_as_with_one_slot() {
    // Each `defn` expands on the coordinator, and its unit's `fn` and `let`
    // expand natively in the unit, wherever it is analyzed; the unit is
    // charged where the coordinator takes it, in unit order.
    let sources = Sources::new();
    let mut body = String::from("(ns app.l)\n");
    for k in 0..30 {
        body.push_str(&format!(
            "(defn f{k} [x] (let [y (+ x {k})] (let [z y] z)))\n"
        ));
    }
    body.push_str("(js/console.log (f29 1))");
    sources.write("app/l.cljs", &body);
    let limited = |expansions, generated_nodes| {
        let mut host = MacroHost::new();
        host.limits = light_cljs::MacroLimits {
            expansions,
            generated_nodes,
            ..Default::default()
        };
        host
    };
    let mut failures = 0;
    for (expansions, nodes) in [(40, 100_000), (75, 100_000), (1024, 300), (1024, 900)] {
        let serial = build_with(&sources, "app.l", 1, false, limited(expansions, nodes));
        failures += usize::from(serial.output.is_err());
        for slots in [2, 8] {
            for _ in 0..3 {
                let pooled =
                    build_with(&sources, "app.l", slots, false, limited(expansions, nodes));
                assert_eq!(pooled.output, serial.output, "{expansions} {nodes} {slots}");
                assert_eq!(pooled.stats.macro_calls, serial.stats.macro_calls);
            }
        }
    }
    assert_eq!(failures, 4);
    let serial = build(&sources, "app.l", 1, false);
    assert_eq!(execute(&serial.output.unwrap()), "30\n");
    // An expansion limit already reached fails a unit's first native
    // expansion even when that expansion's nodes alone exceed the node
    // limit: the expansion limit is checked first.
    sources.write(
        "app/l.cljs",
        &format!(
            "(ns app.l) (let [] nil) (def x (fn [] [{}]))",
            (0..100)
                .map(|n| n.to_string())
                .collect::<Vec<_>>()
                .join(" ")
        ),
    );
    let serial = build_with(&sources, "app.l", 1, false, limited(1, 50));
    assert!(
        serial
            .output
            .as_ref()
            .unwrap_err()
            .contains("expansion limit exceeded"),
        "{:?}",
        serial.output
    );
    for slots in [2, 8] {
        let pooled = build_with(&sources, "app.l", slots, false, limited(1, 50));
        assert_eq!(pooled.output, serial.output, "{slots} slots");
    }
}

#[test]
fn a_poll_budget_spent_in_macro_free_units_fails_as_with_one_slot() {
    // `start`'s `when` begins an engine operation, which renews the poll
    // budget; every form the later units analyze polls it, and their polls
    // alone exceed the small budget. A worker analyzing them polls nothing,
    // so their polls are spent where they are charged. V8's interrupts poll
    // on a timer too, so where the budget runs out is not deterministic,
    // even with one slot; that it runs out is.
    let sources = Sources::new();
    let mut body = String::from("(ns app.p) (def start (when true 1))\n");
    for k in 0..40 {
        let terms = (0..50)
            .map(|n| format!("(+ x {n})"))
            .collect::<Vec<_>>()
            .join(" ");
        body.push_str(&format!("(defn f{k} [x] [{terms}])\n"));
    }
    body.push_str("(js/console.log start (count (f39 1)))");
    sources.write("app/p.cljs", &body);
    let budgeted = |budget| {
        let mut host = MacroHost::new();
        host.evaluator_options.poll_budget = budget;
        host
    };
    let large = build_with(&sources, "app.p", 1, false, budgeted(10_000_000));
    assert_eq!(execute(large.output.as_ref().unwrap()), "1 50\n");
    for slots in [1, 2, 8] {
        let small = build_with(&sources, "app.p", slots, false, budgeted(3_000));
        let error = small.output.unwrap_err();
        assert!(
            error.contains("poll budget exhausted"),
            "{slots} slots: {error}"
        );
        let pooled = build_with(&sources, "app.p", slots, false, budgeted(10_000_000));
        assert_eq!(pooled.output, large.output, "{slots} slots");
    }
}

#[test]
fn a_template_naming_a_var_by_placeholder_is_not_macro_free() {
    // `app.hidden` is loaded only through `app.bridge`; the template's named
    // placeholder resolves it, which a dependency observation records.
    let sources = Sources::new();
    sources
        .write("app/hidden.cljs", "(ns app.hidden) (def x 41)")
        .write("app/bridge.cljs", "(ns app.bridge (:require [app.hidden]))")
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.bridge])) (def value (js* \"~{app.hidden/x} + 1\")) (def plain (js* \"~{} + 1\" 1)) (js/console.log value plain)",
        );
    let serial = same_at_every_slot_count(&sources, "app.main", false);
    assert_eq!(execute(&serial.output.unwrap()), "42 2\n");
    // `plain`, `x`, and nothing else of the three namespaces.
    let pooled = build(&sources, "app.main", 8, false);
    assert_eq!(pooled.stats.deferred_units.macro_free, pooled_count(2));
}

/// A namespace's units merge after the next namespace's top-level pass, the
/// one ID-order change of stage A (docs/compiler-concurrency.md, "Merge"):
/// the entry's top-level expressions come before its dependency's bodies,
/// and the program is the same at every slot count.
#[test]
fn a_namespaces_bodies_follow_the_next_namespaces_top_level_pass() {
    let sources = Sources::new();
    sources
        .write(
            "app/a.cljs",
            "(ns app.a) (defn body [] \"a-body\") (def top 1)",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.a :as a])) (defn main-body [] \"main-top\") (js/console.log (a/body) a/top (main-body))",
        );
    let mut programs = Vec::new();
    for slots in [1, 2, 8] {
        let mut warnings = Vec::new();
        let (program, _) = analyze_namespace_entry_recorded(
            "app.main",
            std::slice::from_ref(sources.0.path()),
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut warnings,
            EntryRecorders {
                slots: Slots::new(slots).unwrap(),
                ..Default::default()
            },
        )
        .unwrap();
        let string = |text: &str| {
            let text = text.encode_utf16().collect::<Vec<_>>();
            program
                .exprs
                .iter()
                .position(|expr| {
                    matches!(expr.kind, ExprKind::String(id) if program.strings[id.0] == text)
                })
                .expect("a string expression")
        };
        let source = |name: &str| program.namespaces.iter().position(|n| n == name).unwrap();
        let (a, main) = (source("app.a"), source("app.main"));
        let first_main = program
            .exprs
            .iter()
            .position(|expr| expr.source_index == main)
            .unwrap();
        // The program's root, last, is not a source's.
        let last_a = program
            .exprs
            .iter()
            .take(program.root.0)
            .rposition(|expr| expr.source_index == a)
            .unwrap();
        assert!(
            first_main < string("a-body") && string("a-body") <= last_a,
            "{slots} slots: app.a's body follows app.main's first top-level expression"
        );
        assert!(
            string("main-top") > last_a,
            "{slots} slots: app.main's bodies follow app.a's"
        );
        let js = emit_js(&program);
        assert_eq!(execute(&js), "a-body 1 main-top\n");
        programs.push(js);
    }
    assert!(programs.windows(2).all(|pair| pair[0] == pair[1]));
}

/// A Var a deferred initializer defines is the next namespace's to resolve,
/// qualified, through an alias and referred, although the initializer's body
/// merges only after that namespace's top-level pass; and cached builds,
/// installing either namespace, match.
#[test]
fn body_discovered_vars_resolve_in_the_next_namespace() {
    let sources = Sources::new();
    sources
        .write(
            "app/a.cljs",
            "(ns app.a) (def init (do (def answer 42) nil))",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.a :as a :refer [answer]])) (js/console.log app.a/answer a/answer answer)",
        );
    let serial = same_at_every_slot_count(&sources, "app.main", false);
    assert_eq!(execute(&serial.output.unwrap()), "42 42 42\n");
    crate::cache_contract::check_mixed(sources.0.path(), "app.main");
}
