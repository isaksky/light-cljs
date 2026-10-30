//! Macro-free deferred units on the compiler's pool (docs/compiler-concurrency.md,
//! "As built (light-cljs-338)"): which units are found macro-free when they
//! are registered, and that a build with several slots, whose workers
//! analyze those units with no compile-time engine, emits the program,
//! warnings and errors of a build with one.
use crate::temp_dir::TempDir;
use light_cljs::analyzer::{AnalysisOptions, AnalysisWarning, ExprKind};
use light_cljs::application::{ApplicationOptions, OptimizationLevel};
use light_cljs::compiler::{EntryRecorders, analyze_namespace_entry_recorded};
use light_cljs::scheduler::Slots;
use light_cljs::{CompileStats, MacroHost, emit_js};
use std::{fs, process::Command};

const PANTS_BUILD: bool = cfg!(all(
    feature = "bbp-macros",
    not(any(light_v8, light_quickjs))
));

pub(crate) struct Sources(TempDir);

impl Sources {
    pub(crate) fn new() -> Self {
        Self(TempDir::new("cljs-macro-free"))
    }
    pub(crate) fn root(&self) -> &std::path::PathBuf {
        self.0.path()
    }
    pub(crate) fn write(&self, path: &str, source: &str) -> &Self {
        let path = self.0.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
        self
    }
}

/// What a build made: its JavaScript or error, its warnings with their
/// spans and its stats.
pub(crate) struct Built {
    pub(crate) output: Result<String, String>,
    /// Every expression's source index and span, and the source-mapped
    /// output at `none` and at `prepare`, of a build that succeeded.
    pub(crate) spans: Vec<(usize, std::ops::Range<usize>)>,
    pub(crate) mapped: Vec<(String, light_cljs::source_map::SourceMapping)>,
    pub(crate) warnings: Vec<(String, String, usize, std::ops::Range<usize>)>,
    pub(crate) stats: CompileStats,
}

pub(crate) fn build(sources: &Sources, entry: &str, slots: usize, warning_mode: bool) -> Built {
    build_with(sources, entry, slots, warning_mode, MacroHost::new())
}

/// A macro deadline that the work before a spinning macro does not reach:
/// 300 ms, or four times what `before`, that work without the spin, takes
/// now if load makes it slower. Tests that assert which form a wall-clock
/// deadline stops a build at use it (light-cljs-378).
pub(crate) fn deadline_after(before: impl FnOnce()) -> std::time::Duration {
    let started = std::time::Instant::now();
    before();
    (started.elapsed() * 4).max(std::time::Duration::from_millis(300))
}

pub(crate) fn build_with(
    sources: &Sources,
    entry: &str,
    slots: usize,
    warning_mode: bool,
    host: MacroHost,
) -> Built {
    build_blocked(sources, entry, slots, warning_mode, host, &[])
}

/// [`build_with`] with `blockers` as the `--cache-blocker` set.
pub(crate) fn build_blocked(
    sources: &Sources,
    entry: &str,
    slots: usize,
    warning_mode: bool,
    host: MacroHost,
    blockers: &[String],
) -> Built {
    build_configured(sources, entry, slots, warning_mode, host, blockers, |_| {})
}

/// [`build_blocked`] with its recorders adjusted by `configure`.
pub(crate) fn build_configured(
    sources: &Sources,
    entry: &str,
    slots: usize,
    warning_mode: bool,
    mut host: MacroHost,
    blockers: &[String],
    configure: impl FnOnce(&mut EntryRecorders<'_>),
) -> Built {
    let mut warnings: Vec<AnalysisWarning> = Vec::new();
    let mut stats = CompileStats::new();
    let mut recorders = EntryRecorders {
        stats: Some(&mut stats),
        slots: Slots::new(slots).unwrap(),
        blockers,
        ..Default::default()
    };
    configure(&mut recorders);
    let analyzed = analyze_namespace_entry_recorded(
        entry,
        std::slice::from_ref(sources.0.path()),
        &mut host,
        AnalysisOptions {
            warning_mode,
            ..Default::default()
        },
        &mut warnings,
        recorders,
    );
    let program = analyzed.as_ref().ok().map(|(program, _)| program);
    Built {
        spans: program.map_or_else(Vec::new, |program| {
            program
                .exprs
                .iter()
                .map(|e| (e.source_index, e.span.clone()))
                .collect()
        }),
        mapped: program.map_or_else(Vec::new, |program| {
            [OptimizationLevel::None, OptimizationLevel::Prepare]
                .map(|optimize| {
                    let options = ApplicationOptions {
                        optimize,
                        ..Default::default()
                    };
                    light_cljs::prepare_application(program, options).emit_js_mapped()
                })
                .into()
        }),
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

pub(crate) fn execute(js: &str) -> String {
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

/// What a build's memory budget needs beyond the main engine's 64 MiB for
/// each worker engine: 64 MiB for a V8 isolate or a QuickJS runtime in
/// QuickJS-only builds (light-cljs-436, light-cljs-441).
pub(crate) const WORKER_SHARE: usize = 64 << 20;

/// A host whose memory budget gives the main isolate its default 64 MiB
/// and one worker isolate its share: with more slots, the other workers
/// have no engine and take only macro-free units. The default budget gives no
/// worker an isolate.
pub(crate) fn one_worker_isolate() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.memory_limit = (64 << 20) + WORKER_SHARE;
    host
}

/// A host whose memory budget gives each of eight slots an isolate of the
/// default size, where the default budget gives no worker an isolate.
pub(crate) fn isolate_per_slot() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.memory_limit *= 8;
    host
}

/// Build `entry` at one, two, four and eight slots, several rounds each,
/// with an isolate for every slot, and check every build's program, spans,
/// source maps, warnings and macro counts against the one-slot build;
/// return that build.
pub(crate) fn same_at_every_slot_count(
    sources: &Sources,
    entry: &str,
    warning_mode: bool,
) -> Built {
    let serial = build_with(sources, entry, 1, warning_mode, isolate_per_slot());
    for slots in [2, 4, 8] {
        for _ in 0..3 {
            let pooled = build_with(sources, entry, slots, warning_mode, isolate_per_slot());
            assert_eq!(pooled.output, serial.output, "{slots} slots");
            assert_eq!(pooled.spans, serial.spans, "{slots} slots");
            assert_eq!(pooled.mapped, serial.mapped, "{slots} slots");
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
    // `a`, `b`, `c` (native `defn`, `let` and `loop`), `d` (`when`, a core
    // macro expanded natively, light-cljs-432), `g` (a function of a
    // required alias), `twice`, `h`, `l` (quoted), `helper` and `m`, which
    // calls it before the file defines a macro; not `e` (an imported
    // macro), `i` (a nested definition), `k` (an intrinsic), `n` (a source
    // macro) or `o` (a `:test` unit).
    assert_eq!(pooled.stats.deferred_units.macro_free, 10);
    assert_eq!(pooled.output, serial.output);
    assert_eq!(execute(&serial.output.unwrap()), "1 2 3 1 1 2 5 when 2\n");
}

/// Units whose only macros are core macros the analyzer expands natively,
/// names allocated in their scopes included, are macro-free: workers with
/// no engine analyze them, each under `MacroFreeUnit`, which panics at any
/// entry into an engine, and the build is the one-slot build
/// (light-cljs-432). A unit with a call the native expansion would leave to
/// the engine is not.
#[test]
fn units_whose_only_macros_expand_natively_need_no_engine() {
    let sources = Sources::new();
    sources.write(
        "app/a.cljs",
        r#"(ns app.a)
(defn f1 [x] (or (seq x) (and (pos? (count x)) :empty)))
(defn f2 [m] (when-let [v (:k m)] (if-let [w (:w m)] [v w] v)))
(defn f3 [x] (case x 1 :one (2 3) :few "s" :str :other))
(defn f4 [xs] (vec (for [x xs :let [y (* x 2)] :when (odd? x)] y)))
(defn f5 [xs] (let [a (atom 0)] (doseq [x xs] (swap! a + x)) @a))
(defn f6 [m] (cond-> m (:a m) (assoc :b 1) true (update :c (fnil inc 0))))
(defn f7 [m] (some-> m :a inc))
(defn f8 [x] (cond (nil? x) :nil (number? x) :n :else (-> x str count)))
(defn f9 [] (js-obj "a" 1 "b" (inc 1)))
(defn f10 [v] (vswap! v inc))
(defn f11 [x] (if-some [y x] (when-some [z y] (when-first [w [z]] w)) (if-not x :no :yes)))
(defn f12 [x] (condp = x 1 :one 2 :two :other))
(defn f13 [x] (->> x (map inc) (filter odd?) (when-not (nil? x)) vec))
(defn f14 [x] (-> [y x] (if-let y :no)))
(defn wide [] (js-obj "a" 1 "b" 2 "c" 3 "d" 4 "e" 5 "f" 6 "g" 7 "h" 8 "i" 9))
(js/console.log (pr-str [(f1 [1]) (f2 {:k 1 :w 2}) (f3 2) (f4 [1 2 3]) (f5 [1 2]) (f6 {:a 1}) (f7 {:a 1}) (f8 "ab") (.-b (f9)) (f10 (volatile! 1)) (f11 3) (f12 2) (f13 [1 2]) (f14 5)]))"#,
    );
    let serial = same_at_every_slot_count(&sources, "app.a", false);
    assert_eq!(
        execute(serial.output.as_ref().unwrap()),
        "[(1) [1 2] :few [2 6] 3 {:a 1, :b 1, :c 1} 2 2 2 2 3 :two [3] 5]\n"
    );
    // One isolate's memory: the workers have no engine.
    for slots in [2, 4, 8] {
        let mut host = MacroHost::new();
        host.evaluator_options.memory_limit = 32 << 20;
        let pooled = build_with(&sources, "app.a", slots, false, host);
        assert_eq!(pooled.output, serial.output, "{slots} slots");
        assert_eq!(pooled.spans, serial.spans, "{slots} slots");
        assert_eq!(pooled.mapped, serial.mapped, "{slots} slots");
        assert_eq!(
            pooled.stats.macro_calls, serial.stats.macro_calls,
            "{slots} slots"
        );
        assert_eq!(pooled.stats.deferred_units.worker_isolates, 0);
        // All but `wide`, whose `js-obj` of nine entries the engine
        // expands; `f14`'s `if-let` is judged as `->` threads it.
        assert_eq!(pooled.stats.deferred_units.macro_free, 14, "{slots} slots");
    }
}

/// The check's trial expansion admits no unit whose analysis expands a
/// call in the engine: its stand-in names equal no source symbol (a key
/// `some->` threads into `js-obj` stays apart from `thread0`), a name the
/// namespace shadows is a function call (`if-let`, excluded and defined),
/// and what an expansion leaves out a call of a local analyzes anyway (the
/// first value of a repeated `js-obj` key) (light-cljs-432).
#[test]
fn trial_expansions_admit_no_call_the_engine_expands() {
    let sources = Sources::new();
    sources.write(
        "app/b.cljs",
        r#"(ns app.b (:refer-clojure :exclude [if-let]))
(def if-let (fn* [& xs] xs))
(defn f [x thread0] (some-> x (js-obj (doto 1) thread0 2)))
(defn g [x] (if-let [x 1] 2 3 (doto 1)))
(defn h [] (let [js-obj (fn [& xs] xs)] (js-obj "a" (doto 1) "a" 2)))
(defn ok [x] (when x 1))
(js/console.log (count (js-keys (f "k" "t"))) (count (g 0)) (count (h)) (ok true))"#,
    );
    let serial = build(&sources, "app.b", 1, false);
    assert_eq!(execute(serial.output.as_ref().unwrap()), "2 4 4 1\n");
    for slots in [2, 4, 8] {
        let mut host = MacroHost::new();
        host.evaluator_options.memory_limit = 32 << 20;
        let pooled = build_with(&sources, "app.b", slots, false, host);
        assert_eq!(pooled.output, serial.output, "{slots} slots");
        // `if-let`'s definition and `ok`. The Pants build also proves `f`
        // macro-free; its guarded analysis must never enter the runtime.
        assert_eq!(
            pooled.stats.deferred_units.macro_free,
            if PANTS_BUILD { 3 } else { 2 },
            "{slots} slots"
        );
    }
}

/// Declaration bundles go to the pool as definitions do (light-cljs-428):
/// with an isolate per slot every unit is pooled, and with one isolate's
/// memory the workers, which have no engine, take the types, records,
/// protocols, extensions and methods whose only macros expand natively,
/// each under `MacroFreeUnit`, which panics at any engine entry, as well as
/// at a definition other than the bundle's own products. A record whose
/// method calls a source macro, the multimethod and the macro's own unit
/// are not macro-free; the macro demands the record's and the protocol's
/// bundles from the main isolate at compile time, and a later namespace's
/// top-level pass demands them again, so those are claimed, waited for or
/// reused through the catalog. Every build is the one-slot build.
#[test]
fn declaration_bundles_are_pooled_and_macro_free_ones_need_no_engine() {
    let sources = Sources::new();
    sources
        .write(
            "app/p.cljs",
            r#"(ns app.p)
(defprotocol Shape "Shapes." (area [s]) (label [s] [s prefix] "A label."))
(defrecord Square [n] Shape (area [_] (* n n)) (label [_] (str "sq" (when (pos? n) n))) (label [_ p] (str p n)))
(deftype Circle [r] Shape (area [_] (* 3 r r)) (label [_] (or nil "circle")) (label [_ p] p))
(extend-type number Shape (area [n] n) (label ([n] (cond (neg? n) "neg" :else "num")) ([n p] p)))
(extend-protocol Shape
  nil (area [_] 0) (label [_] "nil")
  string (area [s] (count s)) (label [s] (if-let [x (seq s)] (str (first x)) "")))
(defmulti kind (fn [x] (cond (number? x) :n :else :other)))
(defmethod kind :n [x] (when x :number))
(defmethod kind :other [x] (case x "a" :a :other))
(defmacro sq-area [n] (area (->Square n)))
(defrecord Measured [x] Shape (area [_] (sq-area 3)) (label [_] "m"))
(defrecord Guarded [when] Shape (area [_] (when 2)) (label [_] "g"))"#,
        )
        .write(
            "app/main.cljs",
            r#"(ns app.main (:require [app.p :as p :refer [area label kind]]))
(defn total [xs] (reduce + (map area xs)))
(def nine (p/sq-area 3))
(js/console.log (total [(p/->Square 2) (p/->Circle 1) 5 nil "abc"]) (label (p/->Square 2)) (label (p/->Circle 1)) (label -1) (label nil) (label "xy") (label (p/->Square 2) "n") (str (kind 1)) (str (kind "a")) (area (p/->Measured 0)) nine (p/sq-area 2) (area (p/->Guarded (fn [x] (* 2 x)))))"#,
        );
    let serial = same_at_every_slot_count(&sources, "app.main", false);
    assert_eq!(
        execute(serial.output.as_ref().unwrap()),
        "15 sq2 circle neg nil x n2 :number :a 9 9 4 4\n"
    );
    let registered = serial.stats.deferred_units.registered;
    let pooled = build_with(&sources, "app.main", 4, false, isolate_per_slot());
    assert_eq!(pooled.output, serial.output);
    // Every unit: none is a `:test` one or a cache blocker's.
    assert_eq!(
        pooled.stats.deferred_units.pooled, registered,
        "{registered} registered"
    );
    // One isolate's memory: the workers have no engine.
    for slots in [2, 4, 8] {
        let mut host = MacroHost::new();
        host.evaluator_options.memory_limit = 32 << 20;
        let pooled = build_with(&sources, "app.main", slots, false, host);
        assert_eq!(pooled.output, serial.output, "{slots} slots");
        assert_eq!(pooled.spans, serial.spans, "{slots} slots");
        assert_eq!(pooled.mapped, serial.mapped, "{slots} slots");
        assert_eq!(
            pooled.stats.macro_calls, serial.stats.macro_calls,
            "{slots} slots"
        );
        assert_eq!(pooled.stats.deferred_units.worker_isolates, 0);
        // `Shape`, `Square`, `Circle`, the two extensions, both methods
        // and `total`; not `kind`, `Measured`, `sq-area`, `nine` or
        // `Guarded`, whose field a macro's name spells, as a parameter's
        // would make a function ineligible.
        assert_eq!(pooled.stats.deferred_units.macro_free, 8, "{slots} slots");
        // A pool of one worker leaves the bundles to the coordinator
        // (`CompilationSession::pools_bundle`): it takes only `total`.
        assert_eq!(
            pooled.stats.deferred_units.pooled,
            if slots == 2 {
                if PANTS_BUILD { 2 } else { 1 }
            } else if PANTS_BUILD {
                // Shared-runtime tasks also admit the four units that
                // these per-isolate memory limits exclude on the V8 build.
                12
            } else {
                8
            },
            "{slots} slots"
        );
    }
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
        assert_eq!(pooled.stats.deferred_units.macro_free, 1 + 6 * 13);
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
    // `start`'s `doto` begins an engine operation, which renews the poll
    // budget; every form the later units analyze polls it, and their polls
    // alone exceed the small budget. A worker analyzing them polls nothing,
    // so their polls are spent where they are charged. V8's interrupts poll
    // on a timer too, so where the budget runs out is not deterministic,
    // even with one slot; that it runs out is.
    let sources = Sources::new();
    let mut body = String::from("(ns app.p) (def start (doto 1))\n");
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
    assert_eq!(pooled.stats.deferred_units.macro_free, 2);
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

/// A Var a deferred initializer defines is published at its namespace's
/// drain point, after the top-level passes of the `DRAIN_LAG` namespaces
/// after it (light-cljs-391), with or without warning mode, whose drains
/// after each form publish it for the namespace's own later forms only
/// (light-cljs-393): a namespace within the lag resolves it as an
/// undeclared name, identically at every slot count, so that with warnings
/// a qualified or aliased reference still runs, reported as undeclared, and
/// a `:refer` of it is an error; a namespace past the lag resolves it,
/// qualified, through an alias and referred. A top-level definition, or a
/// type's factory, after the body's is visible at once. Cached builds in
/// either mode, installing any of the namespaces, match.
#[test]
fn body_discovered_vars_resolve_past_the_drain_lag() {
    let within = Sources::new();
    within
        .write(
            "app/a.cljs",
            "(ns app.a) (def init (do (def answer 42) nil))",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.a :as a])) (js/console.log app.a/answer a/answer)",
        );
    let serial = same_at_every_slot_count(&within, "app.main", true);
    assert_eq!(execute(serial.output.as_ref().unwrap()), "42 42\n");
    let undeclared = serial
        .warnings
        .iter()
        .filter(|(_, message, _, _)| message == "Use of undeclared Var app.a/answer")
        .count();
    assert_eq!(undeclared, 2, "{:?}", serial.warnings);
    // Without warnings the qualified references fail, so only warning
    // mode's artifacts are compared.
    assert!(crate::cache_contract::check_mixed_in_mode(within.0.path(), "app.main", true) > 0);
    within.write(
        "app/main.cljs",
        "(ns app.main (:require [app.a :as a :refer [answer]])) (js/console.log answer)",
    );
    for warning_mode in [false, true] {
        let referred = same_at_every_slot_count(&within, "app.main", warning_mode);
        let error = referred.output.unwrap_err();
        assert!(
            error.contains("app.a/answer (:refer does not name a public Var or macro)"),
            "warning mode {warning_mode}: {error}"
        );
    }
    // A top-level definition of it after the body's is the top-level
    // pass's own, visible at once.
    within.write(
        "app/a.cljs",
        "(ns app.a) (def init (do (def answer 42) nil)) (def answer 43)",
    );
    for warning_mode in [false, true] {
        let redefined = same_at_every_slot_count(&within, "app.main", warning_mode);
        assert_eq!(
            execute(redefined.output.as_ref().unwrap()),
            "43\n",
            "warning mode {warning_mode}"
        );
    }
    // So is a type's factory the top-level pass declares after a body
    // defined the name.
    within
        .write(
            "app/a.cljs",
            "(ns app.a) (def init (do (def ->T nil) nil)) (deftype T [x])",
        )
        .write(
            "app/main.cljs",
            "(ns app.main (:require [app.a :refer [->T]])) (js/console.log (.-x (->T 7)))",
        );
    for warning_mode in [false, true] {
        let factory = same_at_every_slot_count(&within, "app.main", warning_mode);
        assert_eq!(
            execute(factory.output.as_ref().unwrap()),
            "7\n",
            "warning mode {warning_mode}"
        );
    }
    // `DRAIN_LAG` namespaces between them put `app.main`'s top-level pass
    // after `app.a`'s drain point.
    let past = Sources::new();
    past.write(
        "app/a.cljs",
        "(ns app.a) (def init (do (def answer 42) nil))",
    );
    let mut requires = String::from("[app.a :as a :refer [answer]]");
    for k in 0..light_cljs::analyzer::DRAIN_LAG {
        past.write(
            &format!("app/f{k}.cljs"),
            &format!("(ns app.f{k} (:require [app.a])) (def v{k} {k})"),
        );
        requires.push_str(&format!(" [app.f{k}]"));
    }
    past.write(
        "app/main.cljs",
        &format!(
            "(ns app.main (:require {requires})) (js/console.log app.a/answer a/answer answer)"
        ),
    );
    for warning_mode in [false, true] {
        let serial = same_at_every_slot_count(&past, "app.main", warning_mode);
        assert_eq!(
            execute(serial.output.as_ref().unwrap()),
            "42 42 42\n",
            "warning mode {warning_mode}"
        );
        assert!(
            serial
                .warnings
                .iter()
                .all(|(_, message, _, _)| !message.contains("answer")),
            "{:?}",
            serial.warnings
        );
        assert!(
            crate::cache_contract::check_mixed_in_mode(past.0.path(), "app.main", warning_mode) > 0
        );
    }
}
