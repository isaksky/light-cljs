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
    /// The argument `n`, sized so that a QuickJS call takes tens of milliseconds.
    input: u64,
    /// What `(f n)` returns.
    expected: fn(u64) -> u64,
}

/// The ratchet's seq-pipeline and record-protocol fixtures, as workloads over `n`.
const STEADY: [Steady; 2] = [
    Steady {
        name: "seq-pipeline",
        source: "(fn [n] (reduce + 0 (->> (map inc (range n)) (filter even?) (partition 2) \
                 (drop 1) (mapcat identity))))",
        input: 200,
        expected: pipeline,
    },
    Steady {
        name: "record-protocol",
        source: "(defprotocol Area (area [shape]))
                 (defrecord Rect [w h] Area (area [_] (* w h)))
                 (fn [n] (loop [i 0 total 0]
                           (if (< i n) (recur (inc i) (+ total (area (->Rect i 2)))) total)))",
        input: 2_000,
        expected: areas,
    },
];

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
    /// Untimed rounds before them, for process, file and engine caches.
    pub warmups: usize,
    /// Replaces every steady-state workload's argument `n` (the smoke test's is tiny).
    pub input: Option<u64>,
    /// Steady-state calls per sample: untimed first, then timed as one batch.
    pub warmup_calls: usize,
    pub calls: usize,
    /// ClojureScript checkout for the optional upstream size column.
    pub upstream: Option<PathBuf>,
    /// The benchmark arguments, recorded in the summary.
    pub command: String,
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
            upstream: None,
            command: String::new(),
        }
    }
}

struct Sample {
    engine: &'static str,
    measure: &'static str,
    fixture: usize,
    level: OptimizationLevel,
    round: usize,
    nanoseconds: f64,
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
    let startup = emit_all(
        &scratch.0,
        "startup",
        fixtures.iter().map(|f| (f.name, f.source)),
    )?;
    let steady = emit_all(
        &scratch.0,
        "steady",
        STEADY.iter().map(|s| (s.name, s.source)),
    )?;
    let node = env::var_os("NODE").unwrap_or_else(|| "node".into());
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
                    level,
                    round: round_index,
                    nanoseconds,
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
        for (index, workload) in STEADY.iter().enumerate() {
            let input = config.input.unwrap_or(workload.input);
            let calls = Calls {
                input,
                expected: (workload.expected)(input),
                warmups: config.warmup_calls,
                timed: config.calls,
            };
            for &level in &levels {
                let program = &steady[index][level_index(level)];
                let context = || format!("{} at {level}", workload.name);
                let call = node_steady(&node, &program.path, &calls)
                    .map_err(|e| format!("Node steady state, {}: {e}", context()))?;
                record("node", "call", index, level, call);
                let call = quickjs_steady(&fs::read_to_string(&program.path)?, &calls)
                    .map_err(|e| format!("QuickJS steady state, {}: {e}", context()))?;
                record("quickjs", "call", index, level, call);
            }
        }
    }
    let last_load = load_average();
    let mut rows = String::from("engine,measure,fixture,level,round,nanoseconds,load_1m\n");
    for sample in &samples {
        let fixture = match sample.measure {
            "call" => STEADY[sample.fixture].name,
            _ => fixtures[sample.fixture].name,
        };
        let load = loads[sample.round].map_or(String::new(), |l| format!("{l:.2}"));
        writeln!(
            rows,
            "{},{},{fixture},{},{},{:.0},{load}",
            sample.engine, sample.measure, sample.level, sample.round, sample.nanoseconds
        )?;
    }
    // Saved before the upstream column, so that a failure there keeps the timings.
    fs::write(&csv, rows)?;
    let upstream = config.upstream.as_ref().map(|checkout| {
        upstream_sizes(checkout, &scratch.0, &node, fixtures).map_err(|e| e.to_string())
    });
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
        Some(Err(error)) => Err(format!("upstream column: {error}").into()),
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

/// One steady-state sample: `(f input)` called `warmups` times untimed, then
/// `timed` times in one batch, always returning `expected`.
struct Calls {
    input: u64,
    expected: u64,
    warmups: usize,
    timed: usize,
}

/// Nanoseconds per call of a warm function in one fresh Node process.
fn node_steady(node: &OsStr, program: &Path, calls: &Calls) -> Result<f64> {
    let output = node_command(node)
        .arg(DRIVER)
        .arg("steady")
        .arg(program)
        .args([
            calls.input.to_string(),
            calls.warmups.to_string(),
            calls.timed.to_string(),
        ])
        .output()?;
    let fields = driver_fields(&output)?;
    if fields[1] != calls.expected.to_string() {
        return Err(format!("returned {}, expected {}", fields[1], calls.expected).into());
    }
    Ok(fields[0].parse::<f64>()? / calls.timed.max(1) as f64)
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

/// Nanoseconds per call of a warm function in a fresh QuickJS session.
fn quickjs_steady(js: &str, calls: &Calls) -> Result<f64> {
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
    for _ in 0..calls.warmups {
        check(engine.call(run, &[input])?)?;
    }
    let mut results = Vec::with_capacity(calls.timed);
    let start = Instant::now();
    for _ in 0..calls.timed {
        results.push(engine.call(run, &[input])?);
    }
    let elapsed = start.elapsed().as_nanos() as f64;
    for result in results {
        check(result)?;
    }
    Ok(elapsed / calls.timed.max(1) as f64)
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
    /// Why the column is empty, when it is.
    skipped: Option<String>,
    identity: String,
    /// Per fixture: the `:advanced` and the lite-mode output.
    sizes: Vec<[Option<Size>; 2]>,
}

/// Compiles every fixture with the ClojureScript checkout at `:advanced` and checks
/// that the output prints what the fixture expects. The upstream program is the
/// fixture in namespace `bench.NAME` with its last form logged the way the Node
/// driver logs a value. Skipped when the checkout or the Clojure CLI is missing.
fn upstream_sizes(
    checkout: &Path,
    scratch: &Path,
    node: &OsStr,
    fixtures: &[Startup],
) -> Result<Upstream> {
    let skipped = |reason: String| {
        eprintln!("levels: upstream column skipped: {reason}");
        Ok(Upstream {
            skipped: Some(reason),
            identity: String::new(),
            sizes: Vec::new(),
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
    fs::create_dir_all(dir.join("src/bench"))?;
    let mut arguments = vec![dir.join("out").display().to_string()];
    for fixture in fixtures {
        let path = dir.join(format!("src/bench/{}.cljs", fixture.name.replace('-', "_")));
        fs::write(&path, upstream_source(fixture)?)?;
        arguments.push(fixture.name.into());
        arguments.push(path.display().to_string());
    }
    eprintln!("levels: compiling {} upstream fixtures", fixtures.len());
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
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let fields: Vec<_> = line.split('\t').collect();
        let ["UPSTREAM", name, variant, path] = fields[..] else {
            continue;
        };
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
    })
}

/// The fixture as namespace `bench.NAME`, logging its last form's value.
fn upstream_source(fixture: &Startup) -> Result<String> {
    let tree = parse(fixture.source).map_err(|e| format!("{}: {e}", fixture.name))?;
    let last = &tree.nodes[tree.roots.last().ok_or("empty fixture")?.0].span;
    Ok(format!(
        "(ns bench.{})\n{}(let [value {}]\n  (js/console.log (js* \"(typeof ~{{}} === 'function' ? '#function' : ~{{}})\" value value)))\n",
        fixture.name,
        &fixture.source[..last.start],
        &fixture.source[last.clone()],
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
    let cell = |engine: &str, measure: &str, fixture: usize, level, scale: f64| {
        stats(
            samples
                .iter()
                .filter(|s| {
                    s.engine == engine
                        && s.measure == measure
                        && s.fixture == fixture
                        && s.level == level
                })
                .map(|s| s.nanoseconds / scale),
        )
        .map_or("–".into(), |s| format!("{:.3} / {:.3}", s.median, s.min))
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
         benchmarks/compiler.md (\"Startup and steady state by optimization level\").\n",
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
    let _ = writeln!(
        out,
        "- Rounds: {} untimed warm-up, {} timed. Steady state: {} untimed then {} timed calls per sample",
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
         Microseconds per call `(f n)`, median / minimum, one fresh engine per sample timing \
         {} calls after {} untimed ones. QuickJS calls go through `Evaluator::call`.\n\n\
         | Fixture | n | Level | Node | QuickJS |\n| --- | ---: | --- | ---: | ---: |\n",
        config.calls, config.warmup_calls
    );
    for (index, workload) in STEADY.iter().enumerate() {
        for level in OptimizationLevel::ALL {
            let _ = writeln!(
                out,
                "| {} | {} | {level} | {} | {} |",
                workload.name,
                config.input.unwrap_or(workload.input),
                cell("node", "call", index, level, 1e3),
                cell("quickjs", "call", index, level, 1e3),
            );
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
            let _ = write!(
                out,
                "{}. Bytes of `main.js` (gzip in parentheses) at `:optimizations :advanced`, \
                 and with `:lite-mode true :elide-to-string true` added, against light-cljs \
                 at `advanced`.\n\n\
                 | Fixture | Upstream :advanced | Upstream lite mode | light-cljs advanced |\n\
                 | --- | ---: | ---: | ---: |\n",
                upstream.identity
            );
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
