//! Startup and steady-state cost of whole-program output at every optimization
//! level, in Node and in the QuickJS engine light-cljs embeds (light-cljs-216).
//! `cargo bench --bench compiler -- --levels --output DIR` runs it; the method and
//! the recorded baseline are in benchmarks/compiler.md ("Startup and steady state
//! by optimization level"). tests/benchmark_variants.rs runs a tiny configuration
//! as a smoke test.
//!
//! The machine may be shared and noisy, so nothing is measured once. Untimed
//! warm-up rounds come first. Each timed round then measures every fixture at every
//! level, rotating the level order, before the next round starts, so a burst of
//! outside load lands on all levels alike. The report gives the median and the
//! minimum over rounds, with the one-minute load average sampled every round.
//! Every sample's output is checked, including during warm-up.
//!
//! With `--upstream`, each steady-state workload is also compiled by the
//! ClojureScript checkout at `:advanced` and timed in Node by the same driver, in the
//! same rounds as light-cljs (light-cljs-233). The collection workloads run at two
//! sizes, so that an operation that is linear where upstream's is constant shows as
//! quadratic growth.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{ApplicationOptions, OptimizationLevel, analyze, parse, prepare_application};
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use std::{env, fs};

type Result<T> = std::result::Result<T, Box<dyn Error>>;

/// A program measured in a fresh engine: parse plus its first execution.
pub struct Startup<'a> {
    pub name: &'a str,
    pub source: &'a str,
    /// Expected stdout: what the program prints, then its value as Node's
    /// `console.log` shows it, except that a function shows as `#function`.
    pub printed: &'a str,
}

/// A program that evaluates to a one-argument function, timed per call once warm.
struct Steady {
    name: &'static str,
    source: &'static str,
    /// The arguments `n`. A ratchet workload has one, sized so that a QuickJS call
    /// takes tens of milliseconds; a collection workload has [`GROWTH`].
    inputs: &'static [u64],
    /// Whether the workload runs at every level in both engines. The collection
    /// workloads run in Node at `advanced` only, the build compared with upstream:
    /// their quadratic calls take seconds in Node today and minutes in QuickJS.
    every_level: bool,
    /// What `(f n)` returns.
    expected: fn(u64) -> u64,
}

/// The collection workloads' sizes. Tenfold growth makes n constant-time
/// operations ten times as slow and n linear-time ones a hundred times;
/// `map-assoc-existing` makes a thousand operations at either size.
const GROWTH: &[u64] = &[2_000, 20_000];

/// Defines `(vector-of n)`, `(vec (range n))` built once and kept, so that a
/// workload over a built vector times only its own operation. The first call, which
/// is untimed, builds it.
macro_rules! built_vector {
    () => {
        "(def built (volatile! nil))
         (defn vector-of [n] (let [v @built] (if (and v (== n (count v))) v (vreset! built (vec (range n))))))\n"
    };
}

/// Defines `(map-of n)`, the map from each of `(range n)` to itself, built once
/// with `into` (a transient in both compilers) and kept, as [`built_vector`].
macro_rules! built_map {
    () => {
        "(def built (volatile! nil))
         (defn map-of [n]
           (let [m @built] (if (and m (== n (count m))) m (vreset! built (into {} (map (fn [i] [i i])) (range n))))))\n"
    };
}

/// The ratchet's seq-pipeline, record-protocol, keyword-lookups,
/// constant-literals, constant-keys, multi-arity, event-maps, closed-params and
/// local-arities fixtures, as workloads over `n`. `local-arities` runs the
/// functions of `multi-arity` as local and named functions. Then the collection
/// workloads of light-cljs-233, each at [`GROWTH`]: building a vector, map and set
/// one element at a time, `nth` and `get` over a built one, `assoc` onto a built
/// map, `subvec` then `reduce`, and `into`, which goes through a transient.
const STEADY: [Steady; 17] = [
    Steady {
        name: "seq-pipeline",
        source: "(fn [n] (reduce + 0 (->> (map inc (range n)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity))))",
        inputs: &[200],
        every_level: true,
        expected: pipeline,
    },
    Steady {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n) (recur (inc i) (+ total (area (->Rect i 2)))) total)))",
        inputs: &[2_000],
        every_level: true,
        expected: areas,
    },
    Steady {
        name: "keyword-lookups",
        source: "(defn score [m] (+ (get m :a 0) (get m :b 0) (:c m 0) (get m :ns/d 0)))
                 (defn kind [k] (case k :x 1 :y 2 0))
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n)
                             (recur (inc i) (+ total (score {:a i :b 1}) (score {:c 2 :ns/d i})
                                               (kind (if (even? i) :x :y))))
                             total)))",
        inputs: &[2_000],
        every_level: true,
        expected: keyword_lookups,
    },
    Steady {
        name: "constant-literals",
        source: "(defn row [i] {:id i :tags #{:a :b} :pos [1 2] :empty []
                                :meta {:kind :row :dims [1 2 3]}})
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n)
                             (let [r (row i)]
                               (recur (inc i) (+ total (count (:pos r)) (count (:tags r))
                                                 (count (get-in r [:meta :dims])))))
                             total)))",
        inputs: &[2_000],
        every_level: true,
        expected: constant_literals,
    },
    Steady {
        name: "constant-keys",
        source: "(def table (zipmap (concat (map (fn [i] {:id i :kind :row}) (range 12))
                                        (map (fn [i] [:pos i (inc i)]) (range 12))
                                        (map (fn [i] #{i :tag}) (range 12)))
                                (range 36)))
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n)
                             (recur (inc i) (+ total (get table {:id 3 :kind :row}) (get table [:pos 5 6])
                                               (get table #{7 :tag}) (get table {:id 11 :kind :row} 0)))
                             total)))",
        inputs: &[200],
        every_level: true,
        expected: constant_keys,
    },
    Steady {
        name: "multi-arity",
        source: "(defn clamp ([x] (clamp x 0 10)) ([x lo hi] (max lo (min hi x))))
                 (defn area ([w] (area w w)) ([w h] (* w h)))
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n)
                             (recur (inc i) (+ total (clamp i) (area (rem i 7)) (area 2 i)))
                             total)))",
        inputs: &[50_000],
        every_level: true,
        expected: multi_arity,
    },
    // A sample's 20 timed calls build 100,000 event maps.
    Steady {
        name: "event-maps",
        source: "(defn event [i] {:type :click :x i :y (- 100 i) :button 0 :target \"submit\"})
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n)
                             (let [e (event i)] (recur (inc i) (+ total (:x e) (:y e) (count e))))
                             total)))",
        inputs: &[5_000],
        every_level: true,
        expected: event_maps,
    },
    Steady {
        name: "closed-params",
        source: "(defrecord Pt [x y])
                 (defn dist2 [p q] (let [dx (- (:x p) (:x q)) dy (- (:y p) (:y q))] (+ (* dx dx) (* dy dy))))
                 (defn at [v i] (nth v i))
                 (defn width [s] (if (= s \"\") 0 (count s)))
                 (defn weight [k] (if (= k :heavy) 10 1))
                 (defn score [acc v i s k]
                   (+ acc (dist2 (->Pt i 0) (->Pt 0 2)) (at v (rem i 3)) (width s) (weight k)))
                 (fn [n] (loop [i 0 acc 0]
                           (if (< i n)
                             (recur (inc i) (score acc [1 2 3] i (if (even? i) \"\" \"word\") (if (odd? i) :heavy :light)))
                             acc)))",
        inputs: &[1_000],
        every_level: true,
        expected: closed_params,
    },
    Steady {
        name: "local-arities",
        source: "(fn [n]
                   (let [clamp (fn clamp ([x] (clamp x 0 10)) ([x lo hi] (max lo (min hi x))))
                         area (fn area ([w] (area w w)) ([w h] (* w h)))]
                     (loop [i 0 total 0]
                       (if (< i n)
                         (recur (inc i) (+ total (clamp i) (area (rem i 7)) (area 2 i)))
                         total))))",
        inputs: &[50_000],
        every_level: true,
        expected: multi_arity,
    },
    Steady {
        name: "vector-conj",
        source: "(fn [n] (let [v (reduce conj [] (range n))] (+ (count v) (nth v (dec n)))))",
        inputs: GROWTH,
        every_level: false,
        expected: count_and_last,
    },
    Steady {
        name: "map-assoc",
        source: "(fn [n] (let [m (reduce (fn [m i] (assoc m i i)) {} (range n))] (+ (count m) (get m (dec n)))))",
        inputs: GROWTH,
        every_level: false,
        expected: count_and_last,
    },
    Steady {
        name: "set-conj",
        source: "(fn [n] (let [s (reduce conj #{} (range n))] (+ (count s) (get s (dec n)))))",
        inputs: GROWTH,
        every_level: false,
        expected: count_and_last,
    },
    Steady {
        name: "vector-nth",
        source: concat!(
            built_vector!(),
            "(fn [n] (let [v (vector-of n)]
                       (loop [i 0 total 0] (if (< i n) (recur (inc i) (+ total (nth v i))) total))))"
        ),
        inputs: GROWTH,
        every_level: false,
        expected: below,
    },
    Steady {
        name: "map-get",
        source: concat!(
            built_map!(),
            "(fn [n] (let [m (map-of n)]
                       (loop [i 0 total 0] (if (< i n) (recur (inc i) (+ total (get m i))) total))))"
        ),
        inputs: GROWTH,
        every_level: false,
        expected: below,
    },
    // A thousand new keys, each onto the same n-entry map: the count is n + 1 every
    // time unless an `assoc` changed the map it was given.
    Steady {
        name: "map-assoc-existing",
        source: concat!(
            built_map!(),
            "(fn [n] (let [m (map-of n)]
                       (loop [i 0 total 0]
                         (if (< i 1000) (recur (inc i) (+ total (count (assoc m (+ n i) i)))) total))))"
        ),
        inputs: GROWTH,
        every_level: false,
        expected: assoc_existing,
    },
    Steady {
        name: "subvec-reduce",
        source: concat!(
            built_vector!(),
            "(fn [n] (reduce + 0 (subvec (vector-of n) 1 n)))"
        ),
        inputs: GROWTH,
        every_level: false,
        expected: below,
    },
    Steady {
        name: "vector-into",
        source: "(fn [n] (let [v (into [] (range n))] (+ (count v) (nth v (dec n)))))",
        inputs: GROWTH,
        every_level: false,
        expected: count_and_last,
    },
];

/// A collection of `(range n)` built up: its count plus its last element.
fn count_and_last(n: u64) -> u64 {
    2 * n - 1
}

/// The sum of `(range n)`.
fn below(n: u64) -> u64 {
    n * (n - 1) / 2
}

fn assoc_existing(n: u64) -> u64 {
    1_000 * (n + 1)
}

fn event_maps(n: u64) -> u64 {
    105 * n
}

/// Whether `name` is a steady-state workload, which `--only` can select.
pub fn is_workload(name: &str) -> bool {
    STEADY.iter().any(|workload| workload.name == name)
}

fn closed_params(n: u64) -> u64 {
    (0..n)
        .map(|i| i * i + 4 + i % 3 + 1 + if i % 2 == 1 { 14 } else { 1 })
        .sum()
}

fn constant_keys(n: u64) -> u64 {
    62 * n
}

fn multi_arity(n: u64) -> u64 {
    (0..n).map(|i| i.min(10) + (i % 7) * (i % 7) + 2 * i).sum()
}

fn constant_literals(n: u64) -> u64 {
    7 * n
}

fn keyword_lookups(n: u64) -> u64 {
    (0..n).map(|i| (i + 1) + (2 + i) + 1 + i % 2).sum()
}

fn pipeline(n: u64) -> u64 {
    let evens: Vec<u64> = (1..=n).filter(|x| x % 2 == 0).collect();
    evens.as_chunks::<2>().0.iter().skip(1).flatten().sum()
}

fn areas(n: u64) -> u64 {
    (0..n).map(|i| 2 * i).sum()
}

pub struct Config {
    /// Receives samples.csv and summary.md; must not hold an earlier run.
    pub output: PathBuf,
    /// Timed rounds. Each gives one sample per engine, measure, fixture and level.
    pub samples: usize,
    /// Untimed rounds before them, for process, file and engine caches. Only the
    /// startup fixtures run in them: a steady-state sample warms its own engine.
    pub warmups: usize,
    /// Replaces every steady-state workload's arguments with this one `n` (the
    /// smoke test's is tiny).
    pub input: Option<u64>,
    /// Steady-state calls per sample: up to `warmup_calls` untimed, then up to
    /// `calls` timed as one batch. Each phase makes at least one call and stops
    /// early when one more, at the phase's average so far, would take it past
    /// `budget`. A target whose call takes longer than `budget` is sampled in
    /// [`SLOW_SAMPLES`] rounds only.
    pub warmup_calls: usize,
    pub calls: usize,
    pub budget: Duration,
    /// ClojureScript checkout for the optional upstream columns.
    pub upstream: Option<PathBuf>,
    /// The benchmark arguments, recorded in the summary.
    pub command: String,
    /// `--only`: the startup fixtures and steady-state workloads to measure,
    /// by name; empty measures all of them.
    pub only: Vec<String>,
}
impl Config {
    pub fn new(output: PathBuf) -> Self {
        Self {
            output,
            samples: 15,
            warmups: 2,
            input: None,
            warmup_calls: 10,
            calls: 20,
            budget: Duration::from_millis(500),
            upstream: None,
            command: String::new(),
            only: Vec::new(),
        }
    }

    /// Whether `--only` selects the fixture or workload `name`.
    pub fn selects(&self, name: &str) -> bool {
        self.only.is_empty() || self.only.iter().any(|only| only == name)
    }

    /// The steady-state workloads `--only` selects.
    fn workloads(&self) -> Vec<&'static Steady> {
        STEADY
            .iter()
            .filter(|workload| self.selects(workload.name))
            .collect()
    }

    /// Each selected workload at each of its arguments, as [`Row`]s.
    fn rows(&self) -> Vec<Row> {
        let mut rows = Vec::new();
        for (workload, steady) in self.workloads().into_iter().enumerate() {
            let inputs = self.input.map_or(steady.inputs.to_vec(), |n| vec![n]);
            rows.extend(inputs.into_iter().map(|input| Row {
                workload,
                steady,
                input,
            }));
        }
        rows
    }
}

/// A steady-state workload at one argument.
struct Row {
    /// Index into [`Config::workloads`], and so into the emitted programs.
    workload: usize,
    steady: &'static Steady,
    input: u64,
}

/// A target whose call takes longer than the budget gets this many samples.
const SLOW_SAMPLES: usize = 3;

/// What a sample ran: light-cljs output at a level, or upstream ClojureScript's
/// `:advanced` output of the same program.
#[derive(Clone, Copy, PartialEq)]
enum Build {
    Level(OptimizationLevel),
    Upstream,
}
impl std::fmt::Display for Build {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Build::Level(level) => write!(f, "{level}"),
            Build::Upstream => f.write_str("upstream"),
        }
    }
}

struct Sample {
    engine: &'static str,
    measure: &'static str,
    /// The startup fixture, or for `call` the steady-state [`Row`].
    fixture: usize,
    build: Build,
    round: usize,
    /// For `call`, per call.
    nanoseconds: f64,
    /// Timed calls the sample averages (1 at startup).
    calls: usize,
}

/// A program file at one level.
struct Emitted {
    path: PathBuf,
    bytes: usize,
    gzip: Option<usize>,
}

/// Removes the emitted programs, which are too large to keep, however the run ends.
struct Scratch(PathBuf);
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

const DRIVER: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/benchmarks/support/compiler_levels.js"
);
const UPSTREAM_SCRIPT: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/benchmarks/support/upstream_sizes.clj"
);

/// Installs `console.log` in a QuickJS session and evaluates to a function that
/// returns everything printed so far plus its argument shown the way the Node
/// driver shows a value. It formats only what the fixtures print (primitives and
/// flat arrays); anything else fails the output check instead of passing silently.
const QUICKJS_CONSOLE: &str = r##"(() => {
  let printed = "";
  const show = (value, nested) =>
    value === undefined ? "undefined"
    : value === null ? "null"
    : typeof value === "string" ? (nested ? `'${value}'` : value)
    : typeof value === "function" ? (nested ? "[Function]" : "#function")
    : Array.isArray(value) ? (value.length ? `[ ${value.map(v => show(v, true)).join(", ")} ]` : "[]")
    : typeof value === "object" ? "[unsupported object]"
    : String(value);
  globalThis.console = { log: (...values) => { printed += values.map(v => show(v, false)).join(" ") + "\n"; } };
  return value => printed + show(value, false) + "\n";
})()"##;

pub fn run(config: &Config, fixtures: &[Startup]) -> Result<()> {
    let csv = config.output.join("samples.csv");
    if csv.exists() {
        return Err(format!("{} exists; use a new output directory", csv.display()).into());
    }
    fs::create_dir_all(&config.output)?;
    let scratch = Scratch(env::temp_dir().join(format!(
        "light-cljs-levels-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
    )));
    fs::create_dir_all(&scratch.0)?;
    let workloads = config.workloads();
    let rows = config.rows();
    let startup = emit_all(
        &scratch.0,
        "startup",
        fixtures.iter().map(|f| (f.name, f.source)),
    )?;
    let steady = emit_all(
        &scratch.0,
        "steady",
        workloads.iter().map(|s| (s.name, s.source)),
    )?;
    let node = env::var_os("NODE").unwrap_or_else(|| "node".into());
    // Compiled before the rounds, so that upstream's steady state is timed in them. A
    // failure leaves the light-cljs timings in place and is reported at the end.
    let upstream = config.upstream.as_ref().map(|checkout| {
        upstream_builds(checkout, &scratch.0, &node, fixtures, &workloads)
            .map_err(|e| e.to_string())
    });
    let upstream_steady: &[PathBuf] = match &upstream {
        Some(Ok(upstream)) => &upstream.steady,
        _ => &[],
    };
    let first_load = load_average();
    let mut samples = Vec::new();
    let mut loads = Vec::new();
    let rounds = config.warmups + config.samples;
    for round in 0..rounds {
        let load = load_average();
        let timed = round >= config.warmups;
        eprintln!(
            "levels: {} round {}/{rounds}, load {}",
            if timed { "timed" } else { "warm-up" },
            round + 1,
            load.map_or("unknown".into(), |l| format!("{:.2}", l[0]))
        );
        if timed {
            loads.push(load.map(|l| l[0]));
        }
        let round_index = round.saturating_sub(config.warmups);
        let mut record = |engine, measure, fixture, level, nanoseconds: f64| {
            if timed {
                samples.push(Sample {
                    engine,
                    measure,
                    fixture,
                    build: Build::Level(level),
                    round: round_index,
                    nanoseconds,
                    calls: 1,
                });
            }
        };
        let levels: Vec<_> = (0..3)
            .map(|offset| OptimizationLevel::ALL[(round + offset) % 3])
            .collect();
        for (index, fixture) in fixtures.iter().enumerate() {
            for &level in &levels {
                let program = &startup[index][level_index(level)];
                let context = || format!("{} at {level}", fixture.name);
                let [parse, execute, process] = node_startup(&node, &program.path, fixture.printed)
                    .map_err(|e| format!("Node startup, {}: {e}", context()))?;
                record("node", "parse", index, level, parse);
                record("node", "execute", index, level, execute);
                record("node", "parse+execute", index, level, parse + execute);
                record("node", "process", index, level, process);
                let total = quickjs_startup(&fs::read_to_string(&program.path)?, fixture.printed)
                    .map_err(|e| format!("QuickJS startup, {}: {e}", context()))?;
                record("quickjs", "parse+execute", index, level, total);
            }
        }
        // A steady-state sample warms its own fresh engine with untimed calls.
        if !timed {
            continue;
        }
        for (index, row) in rows.iter().enumerate() {
            let calls = Calls {
                input: row.input,
                expected: (row.steady.expected)(row.input),
                warmups: config.warmup_calls,
                timed: config.calls,
                budget: config.budget,
            };
            let mut builds: Vec<Build> = if row.steady.every_level {
                OptimizationLevel::ALL
                    .iter()
                    .map(|&l| Build::Level(l))
                    .collect()
            } else {
                vec![Build::Level(OptimizationLevel::Advanced)]
            };
            if !upstream_steady.is_empty() {
                builds.push(Build::Upstream);
            }
            let turn = round % builds.len();
            builds.rotate_left(turn);
            for build in builds {
                let program = match build {
                    Build::Level(level) => &steady[row.workload][level_index(level)].path,
                    Build::Upstream => &upstream_steady[row.workload],
                };
                let context = || format!("{} (n = {}) at {build}", row.steady.name, row.input);
                let sample = |engine, (nanoseconds, calls): (f64, usize)| Sample {
                    engine,
                    measure: "call",
                    fixture: index,
                    build,
                    round: round_index,
                    nanoseconds,
                    calls,
                };
                if !sampled_enough(&samples, "node", index, build, config.budget) {
                    let call = node_steady(&node, program, &calls)
                        .map_err(|e| format!("Node steady state, {}: {e}", context()))?;
                    samples.push(sample("node", call));
                }
                if row.steady.every_level
                    && build != Build::Upstream
                    && !sampled_enough(&samples, "quickjs", index, build, config.budget)
                {
                    let call = quickjs_steady(&fs::read_to_string(program)?, &calls)
                        .map_err(|e| format!("QuickJS steady state, {}: {e}", context()))?;
                    samples.push(sample("quickjs", call));
                }
            }
        }
    }
    let last_load = load_average();
    let mut lines =
        String::from("engine,measure,fixture,n,level,round,nanoseconds,calls,load_1m\n");
    for sample in &samples {
        let (fixture, input) = match sample.measure {
            "call" => {
                let row = &rows[sample.fixture];
                (row.steady.name, row.input.to_string())
            }
            _ => (fixtures[sample.fixture].name, String::new()),
        };
        let load = loads[sample.round].map_or(String::new(), |l| format!("{l:.2}"));
        writeln!(
            lines,
            "{},{},{fixture},{input},{},{},{:.0},{},{load}",
            sample.engine,
            sample.measure,
            sample.build,
            sample.round,
            sample.nanoseconds,
            sample.calls
        )?;
    }
    fs::write(&csv, lines)?;
    let summary = summary(
        config,
        fixtures,
        &startup,
        &samples,
        [first_load, last_load],
        &loads,
        upstream.as_ref(),
    );
    fs::write(config.output.join("summary.md"), &summary)?;
    println!("{summary}");
    match upstream {
        Some(Err(error)) => Err(format!("upstream ClojureScript: {error}").into()),
        _ => Ok(()),
    }
}

fn level_index(level: OptimizationLevel) -> usize {
    OptimizationLevel::ALL
        .iter()
        .position(|&l| l == level)
        .unwrap()
}

/// Emits every program at every level into `dir`, checking that each compiles.
fn emit_all<'a>(
    dir: &Path,
    kind: &str,
    programs: impl Iterator<Item = (&'a str, &'a str)>,
) -> Result<Vec<[Emitted; 3]>> {
    programs
        .map(|(name, source)| {
            let program = analyze(source, &parse(source).map_err(|e| format!("{name}: {e}"))?)
                .map_err(|e| format!("{name}: {e}"))?;
            let emit = |level: OptimizationLevel| -> Result<Emitted> {
                let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
                let path = dir.join(format!("{kind}-{name}-{level}.js"));
                fs::write(&path, &js)?;
                Ok(Emitted {
                    gzip: gzip_bytes(&path),
                    bytes: js.len(),
                    path,
                })
            };
            Ok([
                emit(OptimizationLevel::None)?,
                emit(OptimizationLevel::Prepare)?,
                emit(OptimizationLevel::Advanced)?,
            ])
        })
        .collect()
}

fn node_command(node: &OsStr) -> Command {
    let mut command = Command::new(node);
    // No user flags or on-disk compile cache: every sample parses from source.
    command
        .env_remove("NODE_OPTIONS")
        .env_remove("NODE_COMPILE_CACHE")
        .stdin(Stdio::null());
    command
}

/// The tab-separated fields after the `LEVELS` tag on the driver's last stderr line.
fn driver_fields(output: &Output) -> Result<Vec<String>> {
    let stderr = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        return Err(format!("{}: {stderr}", output.status).into());
    }
    let line = stderr
        .lines()
        .last()
        .and_then(|line| line.strip_prefix("LEVELS\t"))
        .ok_or_else(|| format!("no timing line in: {stderr}"))?;
    Ok(line.split('\t').map(String::from).collect())
}

fn check_printed(actual: &str, expected: &str) -> Result<()> {
    if actual == expected {
        Ok(())
    } else {
        Err(format!("printed {actual:?}, expected {expected:?}").into())
    }
}

/// Parse, execute and whole-process nanoseconds of one fresh Node process.
fn node_startup(node: &OsStr, program: &Path, expected: &str) -> Result<[f64; 3]> {
    let start = Instant::now();
    let output = node_command(node)
        .arg(DRIVER)
        .arg("startup")
        .arg(program)
        .output()?;
    let process = start.elapsed().as_nanos() as f64;
    let fields = driver_fields(&output)?;
    check_printed(&String::from_utf8_lossy(&output.stdout), expected)?;
    Ok([fields[0].parse()?, fields[1].parse()?, process])
}

/// One steady-state sample: `(f input)` called up to `warmups` times untimed, then
/// up to `timed` times in one batch, always returning `expected`. Each phase makes
/// at least one call and stops early when one more, at the phase's average so
/// far, would take it past `budget`.
struct Calls {
    input: u64,
    expected: u64,
    warmups: usize,
    timed: usize,
    budget: Duration,
}
impl Calls {
    /// Whether a phase that has made `made` calls in `elapsed` makes another.
    fn another(&self, made: usize, elapsed: Duration) -> bool {
        made == 0 || elapsed + elapsed / made as u32 <= self.budget
    }
}

/// Nanoseconds per call of a warm function in one fresh Node process, and the
/// timed calls they average.
fn node_steady(node: &OsStr, program: &Path, calls: &Calls) -> Result<(f64, usize)> {
    let output = node_command(node)
        .arg(DRIVER)
        .arg("steady")
        .arg(program)
        .args([
            calls.input.to_string(),
            calls.warmups.to_string(),
            calls.timed.to_string(),
            calls.budget.as_nanos().to_string(),
        ])
        .output()?;
    let fields = driver_fields(&output)?;
    if fields[2] != calls.expected.to_string() {
        return Err(format!("returned {}, expected {}", fields[2], calls.expected).into());
    }
    let made: usize = fields[1].parse()?;
    Ok((fields[0].parse::<f64>()? / made.max(1) as f64, made))
}

/// A fresh embedded-engine session, as `EvalSession` creates one, with limits that
/// a long benchmark cannot reach.
fn quickjs() -> Result<Evaluator> {
    Ok(Evaluator::new(EvaluatorOptions {
        memory_limit: 1 << 30,
        poll_budget: u64::MAX,
        timeout: Duration::from_secs(24 * 60 * 60),
        ..EvaluatorOptions::default()
    })?)
}

/// Nanoseconds to parse and first execute `js` in a fresh QuickJS session. The
/// session's own runtime is loaded before timing starts.
fn quickjs_startup(js: &str, expected: &str) -> Result<f64> {
    let engine = quickjs()?;
    let show = engine.evaluate_js(QUICKJS_CONSOLE)?;
    // What `evaluate_js` would build, built before timing starts.
    let unit = format!("(rt) => ({js}\n)");
    let start = Instant::now();
    let value = engine.evaluate_unit(&unit)?;
    let elapsed = start.elapsed().as_nanos() as f64;
    let printed = engine.call(show, &[value])?;
    check_printed(&engine.string(printed)?, expected)?;
    Ok(elapsed)
}

/// Nanoseconds per call of a warm function in a fresh QuickJS session, and the
/// timed calls they average.
fn quickjs_steady(js: &str, calls: &Calls) -> Result<(f64, usize)> {
    let engine = quickjs()?;
    let run = engine.evaluate_js(js)?;
    let input = engine.evaluate_js(&calls.input.to_string())?;
    let check = |result| -> Result<()> {
        let value = engine.number(result)?;
        engine.release(result)?;
        if value == calls.expected as f64 {
            Ok(())
        } else {
            Err(format!("returned {value}, expected {}", calls.expected).into())
        }
    };
    let warm = Instant::now();
    let mut made = 0;
    while made < calls.warmups && calls.another(made, warm.elapsed()) {
        check(engine.call(run, &[input])?)?;
        made += 1;
    }
    let mut results = Vec::with_capacity(calls.timed);
    let mut elapsed = Duration::ZERO;
    let start = Instant::now();
    while results.len() < calls.timed && calls.another(results.len(), elapsed) {
        results.push(engine.call(run, &[input])?);
        elapsed = start.elapsed();
    }
    let made = results.len();
    for result in results {
        check(result)?;
    }
    Ok((elapsed.as_nanos() as f64 / made.max(1) as f64, made))
}

/// Whether a steady-state target has all the samples it gets: one whose call
/// takes longer than the budget is sampled in [`SLOW_SAMPLES`] rounds only.
fn sampled_enough(
    samples: &[Sample],
    engine: &str,
    row: usize,
    build: Build,
    budget: Duration,
) -> bool {
    let mine: Vec<_> = samples
        .iter()
        .filter(|s| {
            s.measure == "call" && s.engine == engine && s.fixture == row && s.build == build
        })
        .collect();
    mine.len() >= SLOW_SAMPLES
        && mine
            .iter()
            .any(|s| s.nanoseconds > budget.as_nanos() as f64)
}

/// The one-, five- and fifteen-minute load averages, where the OS reports them.
fn load_average() -> Option<[f64; 3]> {
    let text = fs::read_to_string("/proc/loadavg")
        .ok()
        .or_else(|| capture(Command::new("sysctl").args(["-n", "vm.loadavg"])))?;
    let mut values = text
        .split(|c: char| c.is_whitespace() || c == '{' || c == '}')
        .filter(|field| !field.is_empty())
        .map(str::parse::<f64>);
    Some([
        values.next()?.ok()?,
        values.next()?.ok()?,
        values.next()?.ok()?,
    ])
}

/// Trimmed stdout of a successful command, or `None`.
fn capture(command: &mut Command) -> Option<String> {
    let output = command.stderr(Stdio::null()).output().ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// `gzip -9` size, as the size ratchet table reports it, when gzip is installed.
fn gzip_bytes(path: &Path) -> Option<usize> {
    let output = Command::new("gzip")
        .args(["-9", "-n", "-c"])
        .arg(path)
        .stderr(Stdio::null())
        .output()
        .ok()?;
    output.status.success().then_some(output.stdout.len())
}

/// Raw bytes of an output, and its gzip bytes when gzip is installed.
type Size = (usize, Option<usize>);

struct Upstream {
    /// Why the columns are empty, when they are.
    skipped: Option<String>,
    identity: String,
    /// Per fixture: the `:advanced` and the lite-mode output.
    sizes: Vec<[Option<Size>; 2]>,
    /// Per steady-state workload: its `:advanced` program, which stores the
    /// workload's function in `globalThis.levelsSteady` for the Node driver.
    steady: Vec<PathBuf>,
}

/// Compiles every fixture and steady-state workload with the ClojureScript
/// checkout, in one JVM, and checks that each fixture's output prints what the
/// fixture expects; every steady-state sample checks its own result. A fixture
/// becomes namespace `bench.NAME`, compiled at `:advanced` and in lite mode, with
/// its last form logged the way the Node driver logs a value. A workload becomes
/// `bench.steady.NAME`, compiled at `:advanced`, with its last form stored for the
/// driver. Skipped when the checkout or the Clojure CLI is missing.
fn upstream_builds(
    checkout: &Path,
    scratch: &Path,
    node: &OsStr,
    fixtures: &[Startup],
    workloads: &[&Steady],
) -> Result<Upstream> {
    let skipped = |reason: String| {
        eprintln!("levels: upstream skipped: {reason}");
        Ok(Upstream {
            skipped: Some(reason),
            identity: String::new(),
            sizes: Vec::new(),
            steady: Vec::new(),
        })
    };
    if !checkout.join("deps.edn").is_file() {
        return skipped(format!(
            "no ClojureScript checkout at {}",
            checkout.display()
        ));
    }
    let clojure = env::var_os("CLOJURE").unwrap_or_else(|| "clojure".into());
    if capture(Command::new(&clojure).arg("--version")).is_none() {
        return skipped("the Clojure CLI (and a JVM) is not available".into());
    }
    let root = fs::canonicalize(checkout)?;
    // `:main` must be on the classpath, as upstream's src/test/cljs_build is.
    let dir = scratch.join("upstream");
    fs::create_dir_all(dir.join("src/bench/steady"))?;
    let mut arguments = vec![dir.join("out").display().to_string()];
    let mut program = |name: String, source: String, variants: &str| -> Result<()> {
        let path = dir.join(format!(
            "src/bench/{}.cljs",
            name.replace('-', "_").replace('.', "/")
        ));
        fs::write(&path, source)?;
        arguments.extend([name, path.display().to_string(), variants.into()]);
        Ok(())
    };
    for fixture in fixtures {
        let source = upstream_source(fixture.name, fixture.source, |value| {
            format!(
                "(let [value {value}]\n  (js/console.log (js* \"(typeof ~{{}} === 'function' ? '#function' : ~{{}})\" value value)))"
            )
        })?;
        program(fixture.name.into(), source, "advanced,lite")?;
    }
    for workload in workloads {
        let name = format!("steady.{}", workload.name);
        let source = upstream_source(&name, workload.source, |function| {
            format!("(aset js/globalThis \"levelsSteady\" {function})")
        })?;
        program(name, source, "advanced")?;
    }
    eprintln!(
        "levels: compiling {} upstream fixtures and {} workloads",
        fixtures.len(),
        workloads.len()
    );
    let deps = format!(
        "{{:paths [\"src\"] :deps {{org.clojure/clojurescript {{:local/root \"{}\"}}}}}}",
        root.display()
    );
    let output = Command::new(&clojure)
        .current_dir(&dir)
        .args(["-Srepro", "-Sdeps", &deps, "-M", UPSTREAM_SCRIPT])
        .args(&arguments)
        .stdin(Stdio::null())
        .output()?;
    if !output.status.success() {
        return Err(format!(
            "upstream compilation failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr)
        )
        .into());
    }
    let mut sizes = vec![[None, None]; fixtures.len()];
    let mut steady = vec![None; workloads.len()];
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let ["UPSTREAM", name, variant, path] = fields[..] else {
            continue;
        };
        if let Some(name) = name.strip_prefix("steady.") {
            let index = workloads.iter().position(|w| w.name == name).unwrap();
            steady[index] = Some(PathBuf::from(path));
            continue;
        }
        let index = fixtures.iter().position(|f| f.name == name).unwrap();
        let printed = node_command(node).arg(path).output()?;
        check_printed(
            &String::from_utf8_lossy(&printed.stdout),
            fixtures[index].printed,
        )
        .map_err(|e| format!("upstream {variant} {name}: {e}"))?;
        let bytes = fs::metadata(path)?.len() as usize;
        sizes[index][usize::from(variant == "lite")] = Some((bytes, gzip_bytes(Path::new(path))));
    }
    let steady = steady
        .into_iter()
        .zip(workloads)
        .map(|(path, workload)| path.ok_or(format!("no upstream output for {}", workload.name)))
        .collect::<std::result::Result<_, _>>()?;
    let head = fs::read_to_string(root.join(".git/HEAD")).unwrap_or_default();
    let revision = match head.trim().strip_prefix("ref: ") {
        Some(reference) => fs::read_to_string(root.join(".git").join(reference))
            .unwrap_or_default()
            .trim()
            .to_string(),
        None => head.trim().to_string(),
    };
    Ok(Upstream {
        skipped: None,
        identity: format!(
            "ClojureScript checkout `{}` at `{revision}`",
            checkout.display()
        ),
        sizes,
        steady,
    })
}

/// `source` as namespace `bench.NAME`, with its last form rewritten by `last`.
fn upstream_source(name: &str, source: &str, last: impl Fn(&str) -> String) -> Result<String> {
    let tree = parse(source).map_err(|e| format!("{name}: {e}"))?;
    let span = &tree.nodes[tree.roots.last().ok_or("empty program")?.0].span;
    Ok(format!(
        "(ns bench.{name})\n{}{}\n",
        &source[..span.start],
        last(&source[span.clone()]),
    ))
}

struct Stats {
    median: f64,
    min: f64,
}

fn stats(values: impl Iterator<Item = f64>) -> Option<Stats> {
    let mut values: Vec<f64> = values.collect();
    if values.is_empty() {
        return None;
    }
    values.sort_by(f64::total_cmp);
    let middle = values.len() / 2;
    let median = if values.len() % 2 == 1 {
        values[middle]
    } else {
        (values[middle - 1] + values[middle]) / 2.0
    };
    Some(Stats {
        median,
        min: values[0],
    })
}

fn summary(
    config: &Config,
    fixtures: &[Startup],
    startup: &[[Emitted; 3]],
    samples: &[Sample],
    [first_load, last_load]: [Option<[f64; 3]>; 2],
    loads: &[Option<f64>],
    upstream: Option<&std::result::Result<Upstream, String>>,
) -> String {
    let timing = |engine: &str, measure: &str, fixture: usize, build, scale: f64| {
        stats(
            samples
                .iter()
                .filter(|s| {
                    s.engine == engine
                        && s.measure == measure
                        && s.fixture == fixture
                        && s.build == build
                })
                .map(|s| s.nanoseconds / scale),
        )
    };
    let shown = |stats: Option<Stats>| {
        stats.map_or("–".into(), |s| format!("{:.3} / {:.3}", s.median, s.min))
    };
    let cell = |engine: &str, measure: &str, fixture: usize, level, scale: f64| {
        shown(timing(engine, measure, fixture, Build::Level(level), scale))
    };
    let load = |l: Option<[f64; 3]>| {
        l.map_or("unknown".into(), |l| {
            format!("{:.2} / {:.2} / {:.2}", l[0], l[1], l[2])
        })
    };
    let known: Vec<f64> = loads.iter().flatten().copied().collect();
    let round_loads = match stats(known.iter().copied()) {
        Some(s) => format!(
            "median {:.2}, range {:.2}–{:.2}",
            s.median,
            s.min,
            known.iter().copied().fold(f64::MIN, f64::max)
        ),
        None => "unknown".into(),
    };
    let size = |size: Option<Size>| {
        size.map_or("–".into(), |(bytes, gzip)| {
            format!(
                "{} ({})",
                thousands(bytes),
                gzip.map_or("–".into(), thousands)
            )
        })
    };
    let commit =
        capture(Command::new("git").args(["-C", env!("CARGO_MANIFEST_DIR"), "rev-parse", "HEAD"]))
            .unwrap_or_else(|| "unknown".into());
    let dirty = capture(Command::new("git").args([
        "-C",
        env!("CARGO_MANIFEST_DIR"),
        "status",
        "--porcelain",
        "--untracked-files=no",
    ]))
    .is_some_and(|status| !status.is_empty());
    let tool = |program: &str, argument: &str| {
        capture(Command::new(program).arg(argument)).unwrap_or_else(|| "unknown".into())
    };
    let mut out = String::from("# Startup and steady state by optimization level\n\n");
    let _ = writeln!(
        out,
        "Generated by `cargo bench --locked --bench compiler -- {}`; the method is in \
         benchmarks/compiler.md (\"Startup and steady state by optimization level\", and \
         for steady state against upstream \"Collection throughput against upstream\").\n",
        config.command
    );
    let _ = writeln!(
        out,
        "- Commit: `{commit}`{}",
        if dirty {
            " with uncommitted changes"
        } else {
            ""
        }
    );
    let _ = writeln!(
        out,
        "- Measured: {}; Node {}; {}; {}; {} CPUs",
        tool("date", "-u"),
        tool(
            &env::var("NODE").unwrap_or_else(|_| "node".into()),
            "--version"
        ),
        tool("rustc", "-V"),
        capture(Command::new("uname").args(["-srm"])).unwrap_or_else(|| "unknown OS".into()),
        std::thread::available_parallelism().map_or(0, |n| n.get())
    );
    let budget = config.budget.as_secs_f64() * 1e3;
    let _ = writeln!(
        out,
        "- Rounds: {} untimed warm-up (startup only), {} timed. Steady state: up to {} untimed \
         then up to {} timed calls per sample, each phase stopping early at {budget:.0} ms; a \
         target whose call takes longer is sampled in {SLOW_SAMPLES} rounds",
        config.warmups, config.samples, config.warmup_calls, config.calls
    );
    let _ = writeln!(
        out,
        "- Load average (1 / 5 / 15 min): {} at start, {} at end; 1-minute at each timed round: {round_loads}\n",
        load(first_load),
        load(last_load)
    );
    out.push_str(
        "## Startup: parse plus first execution\n\n\
         Milliseconds, median / minimum, one fresh engine per sample. Node: `parse` is \
         `new vm.Script`, `execute` its first `runInThisContext` (Node's own stdout setup is \
         done before timing), `process` spawn to exit. QuickJS: one evaluation in a fresh \
         light-cljs `Evaluator`, the engine behind `EvalSession`, after the session's own \
         runtime has loaded. Bytes are the emitted program, with `gzip -9` in parentheses.\n\n\
         | Fixture | Level | Bytes | Node parse | Node execute | Node parse+execute | Node process | QuickJS parse+execute |\n\
         | --- | --- | ---: | ---: | ---: | ---: | ---: | ---: |\n",
    );
    for (index, fixture) in fixtures.iter().enumerate() {
        for level in OptimizationLevel::ALL {
            let emitted = &startup[index][level_index(level)];
            let _ = writeln!(
                out,
                "| {} | {level} | {} | {} | {} | {} | {} | {} |",
                fixture.name,
                size(Some((emitted.bytes, emitted.gzip))),
                cell("node", "parse", index, level, 1e6),
                cell("node", "execute", index, level, 1e6),
                cell("node", "parse+execute", index, level, 1e6),
                cell("node", "process", index, level, 1e6),
                cell("quickjs", "parse+execute", index, level, 1e6),
            );
        }
    }
    let _ = write!(
        out,
        "\n## Steady state\n\n\
         Microseconds per call `(f n)`, median / minimum, one fresh engine per sample making \
         up to {} timed calls after up to {} untimed ones. Each phase stops early when one more \
         call would take it past {budget:.0} ms, and a target whose call takes longer than that \
         is sampled in {SLOW_SAMPLES} rounds only; `samples.csv` has each sample's timed calls. \
         QuickJS calls go through `Evaluator::call`. The collection workloads run in Node at \
         `advanced` only.\n\n\
         | Fixture | n | Level | Node | QuickJS |\n| --- | ---: | --- | ---: | ---: |\n",
        config.calls, config.warmup_calls
    );
    let rows = config.rows();
    for (index, row) in rows.iter().enumerate() {
        for level in OptimizationLevel::ALL {
            if row.steady.every_level || level == OptimizationLevel::Advanced {
                let _ = writeln!(
                    out,
                    "| {} | {} | {level} | {} | {} |",
                    row.steady.name,
                    row.input,
                    cell("node", "call", index, level, 1e3),
                    cell("quickjs", "call", index, level, 1e3),
                );
            }
        }
    }
    out.push_str("\n## Upstream ClojureScript\n\n");
    match upstream {
        None => out.push_str("Not measured; pass `--upstream CHECKOUT` to add it.\n"),
        Some(Err(error)) => {
            let _ = writeln!(out, "Failed: {error}");
        }
        Some(Ok(Upstream {
            skipped: Some(reason),
            ..
        })) => {
            let _ = writeln!(out, "Skipped: {reason}.");
        }
        Some(Ok(upstream)) => {
            let _ = writeln!(out, "{}.", upstream.identity);
            if !fixtures.is_empty() {
                out.push_str(
                    "\n### Sizes\n\n\
                     Bytes of `main.js` (gzip in parentheses) at `:optimizations :advanced`, \
                     and with `:lite-mode true :elide-to-string true` added, against light-cljs \
                     at `advanced`.\n\n\
                     | Fixture | Upstream :advanced | Upstream lite mode | light-cljs advanced |\n\
                     | --- | ---: | ---: | ---: |\n",
                );
            }
            for (index, fixture) in fixtures.iter().enumerate() {
                let light = &startup[index][level_index(OptimizationLevel::Advanced)];
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} |",
                    fixture.name,
                    size(upstream.sizes[index][0]),
                    size(upstream.sizes[index][1]),
                    size(Some((light.bytes, light.gzip))),
                );
            }
            if !rows.is_empty() {
                out.push_str(
                    "\n### Steady state\n\n\
                     Milliseconds per call `(f n)` in Node, median / minimum: light-cljs at \
                     `advanced` against the same workload compiled by upstream at `:advanced` \
                     (namespace `bench.steady.NAME`), timed by the same driver in the same \
                     rounds. The ratio is of the medians.\n\n\
                     | Workload | n | light-cljs advanced | Upstream :advanced | light-cljs / upstream |\n\
                     | --- | ---: | ---: | ---: | ---: |\n",
                );
            }
            for (index, row) in rows.iter().enumerate() {
                let light = timing(
                    "node",
                    "call",
                    index,
                    Build::Level(OptimizationLevel::Advanced),
                    1e6,
                );
                let theirs = timing("node", "call", index, Build::Upstream, 1e6);
                let ratio = match (&light, &theirs) {
                    (Some(light), Some(theirs)) => {
                        let ratio = light.median / theirs.median;
                        if ratio < 1.0 {
                            format!("{ratio:.2}×")
                        } else if ratio < 10.0 {
                            format!("{ratio:.1}×")
                        } else {
                            format!("{}×", thousands(ratio.round() as usize))
                        }
                    }
                    _ => "–".into(),
                };
                let _ = writeln!(
                    out,
                    "| {} | {} | {} | {} | {ratio} |",
                    row.steady.name,
                    row.input,
                    shown(light),
                    shown(theirs),
                );
            }
        }
    }
    out
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
