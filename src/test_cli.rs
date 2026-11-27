//! `light-cljs test`: discover the `cljs.test` namespaces of the source
//! roots, compile a runner of them as `--compile --entry` does (with the
//! persistent compiler cache), and run it in an embedded V8 heap, exiting
//! nonzero unless every test passed (docs/cli.md, "light-cljs test").
//!
//! The source roots are the `--source-root DIR` flags, relative to the
//! current directory, or without them the top-level `:source-paths` of
//! `light-cljs.edn`, found as `compile` finds it, whose `:cache-root`,
//! `:cache-retention` and `:cache-blockers` the run takes too. The runner's
//! two namespaces ([`light_cljs::test_runner::runner_sources`]) are written
//! under `<cache root>/test-runner`, another source root.

use std::{
    error::Error,
    ffi::OsString,
    fs,
    io::{self, Write},
    path::{Path, PathBuf},
    rc::Rc,
    time::Duration,
};

use light_cljs::analyzer::AnalysisOptions;
use light_cljs::config_file::{ConfigFile, FILE_NAME, Setting, Value, ValueKind};
use light_cljs::evaluator::EvaluatorOptions;
use light_cljs::test_runner::{self, RunOutcome};

use crate::{CompileFlags, NamespaceFlags, build_namespace_into, count_flag};

pub(crate) const USAGE: &str =
    "light-cljs test [NAMESPACE ...] [--ns-regexp REGEX] [--source-root DIR ...] [FLAGS]

Discover the cljs.test namespaces of the source roots, compile a runner of them
with the persistent compiler cache, and run it in an embedded V8 heap. Test
output goes to stdout; the exit status is 0 only when every test passed.

  NAMESPACE           Run this namespace (repeatable); it must be under a root
  --ns-regexp REGEX   Also run each namespace whose name REGEX, a JavaScript
                      regular expression, finds a match in; -test$ when
                      neither this nor a NAMESPACE is given
  --source-root DIR   Source root, repeatable; without one, the :source-paths
                      of light-cljs.edn, whose :cache-root, :cache-retention
                      and :cache-blockers apply too
  --cache-dir DIR | --no-cache, --cache-blocker NS, --cache-retention N,
  --macro-expansions N, --macro-nodes N, --macro-timeout-ms N,
  --macro-memory-limit N, --macro-poll-budget N, --config KEY=EDN,
  --elide-asserts, --jobs N, --stats
                      As for --compile --entry
  --timeout-ms N      Deadline of the run in milliseconds (10000)
  --memory-limit N    JavaScript heap bytes of the run (67108864)

Synchronous tests and promise-based tests (^:async deftest, async with a
promise) run; the heap is ECMAScript only, with console but no timers, DOM or
Node APIs, so tests that need them belong in a Node or browser build.";

/// The flags of `light-cljs test`.
struct TestFlags {
    names: Vec<String>,
    ns_regexp: Option<String>,
    roots: Vec<PathBuf>,
    compile: CompileFlags,
    limits: EvaluatorOptions,
}

/// The `--compile` flags `test` accepts: the cache, macro limit,
/// configuration, assertion, job and stats flags, not the output's.
const COMPILE_FLAGS: &[&str] = &[
    "--cache-dir",
    "--no-cache",
    "--cache-blocker",
    "--cache-retention",
    "--config",
    "--elide-asserts",
    "--jobs",
    "--stats",
    "--macro-expansions",
    "--macro-nodes",
    "--macro-timeout-ms",
    "--macro-memory-limit",
    "--macro-poll-budget",
];

fn flags(mut args: impl Iterator<Item = OsString>) -> Result<Option<TestFlags>, Box<dyn Error>> {
    let mut flags = TestFlags {
        names: Vec::new(),
        ns_regexp: None,
        roots: Vec::new(),
        compile: CompileFlags::default(),
        limits: EvaluatorOptions::default(),
    };
    while let Some(arg) = args.next() {
        let text = arg
            .into_string()
            .map_err(|_| "test arguments must be valid UTF-8")?;
        match text.as_str() {
            "--help" | "-h" => {
                println!("{USAGE}");
                return Ok(None);
            }
            "--ns-regexp" => {
                if flags.ns_regexp.is_some() {
                    return Err("--ns-regexp may only be given once".into());
                }
                flags.ns_regexp = Some(
                    args.next()
                        .ok_or("--ns-regexp requires a regular expression")?
                        .into_string()
                        .map_err(|_| "--ns-regexp must be valid UTF-8")?,
                );
            }
            "--source-root" => flags.roots.push(PathBuf::from(
                args.next().ok_or("--source-root requires a directory")?,
            )),
            "--timeout-ms" => {
                flags.limits.timeout = Duration::from_millis(count_flag(&text, &mut args)?);
            }
            "--memory-limit" => {
                flags.limits.memory_limit = usize::try_from(count_flag(&text, &mut args)?)
                    .map_err(|_| "--memory-limit value is too large")?;
            }
            flag if COMPILE_FLAGS.contains(&flag) => {
                flags.compile.accept(flag, &mut args)?;
            }
            flag if flag.starts_with('-') => {
                return Err(format!("{flag} is not a flag of test\n\n{USAGE}").into());
            }
            _ => flags.names.push(text),
        }
    }
    if flags.compile.no_cache && flags.compile.cache_dir.is_some() {
        return Err("--no-cache and --cache-dir exclude each other".into());
    }
    Ok(Some(flags))
}

/// The top-level settings of `light-cljs.edn` the run takes, when no
/// `--source-root` is given: the roots, and the directory of the cache root.
fn configured(flags: &mut TestFlags) -> Result<PathBuf, Box<dyn Error>> {
    let start = std::env::current_dir()?;
    let path = ConfigFile::discover(&start).ok_or_else(|| {
        format!(
            "test requires --source-root DIR or a {FILE_NAME} with :source-paths in {} or a parent directory",
            start.display()
        )
    })?;
    let environment = |name: &str| std::env::var(name).ok();
    let file = ConfigFile::read(&path, &environment).map_err(|error| error.to_string())?;
    let elements = |setting| match file.setting(setting).map(|value: &Value| &value.kind) {
        Some(ValueKind::Vector(values) | ValueKind::Set(values)) => values.as_slice(),
        _ => &[],
    };
    flags.roots = elements(Setting::SourcePaths)
        .iter()
        .filter_map(Value::as_str)
        .map(|root| file.resolve(Path::new(root)))
        .collect();
    if flags.roots.is_empty() {
        return Err(format!("{} gives no :source-paths", path.display()).into());
    }
    // The file's blockers are namespace symbols.
    for blocker in elements(Setting::CacheBlockers)
        .iter()
        .filter_map(|value| match &value.kind {
            ValueKind::Symbol(name) => Some(name.as_str()),
            _ => None,
        })
    {
        if !flags.compile.cache_blockers.iter().any(|b| b == blocker) {
            flags.compile.cache_blockers.push(blocker.to_owned());
        }
    }
    if flags.compile.cache_retention.is_none()
        && let Some(ValueKind::Number { text, .. }) = file
            .setting(Setting::CacheRetention)
            .map(|value| &value.kind)
    {
        flags.compile.cache_retention = text.parse().ok();
    }
    let cache_root = file.resolve(Path::new(
        file.setting(Setting::CacheRoot)
            .and_then(Value::as_str)
            .unwrap_or(".light-cljs"),
    ));
    if !flags.compile.no_cache && flags.compile.cache_dir.is_none() {
        flags.compile.cache_dir = Some(cache_root.join("cache"));
    }
    eprintln!("light-cljs - config: {}", path.display());
    Ok(cache_root)
}

/// Write `text` to `path` unless it already holds it, so an unchanged
/// runner keeps its file as it was.
fn write_if_changed(path: &Path, text: &str) -> Result<(), Box<dyn Error>> {
    if fs::read_to_string(path).is_ok_and(|old| old == text) {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::write(path, text).map_err(|error| format!("cannot write {}: {error}", path.display()))?;
    Ok(())
}

pub(crate) fn run(args: impl Iterator<Item = OsString>) -> Result<(), Box<dyn Error>> {
    let Some(mut flags) = flags(args)? else {
        return Ok(());
    };
    let cache_root = if flags.roots.is_empty() {
        configured(&mut flags)?
    } else {
        PathBuf::from(".light-cljs")
    };
    let namespaces =
        test_runner::test_namespaces(&flags.roots, &flags.names, flags.ns_regexp.as_deref())?;
    if namespaces.is_empty() {
        return Err(format!(
            "no test namespaces match {} under the source roots {}",
            flags
                .ns_regexp
                .as_deref()
                .unwrap_or(test_runner::DEFAULT_NS_REGEXP),
            test_runner::describe_roots(&flags.roots)
        )
        .into());
    }
    let runner_root = cache_root.join("test-runner");
    for (path, text) in test_runner::runner_sources(&namespaces) {
        write_if_changed(&runner_root.join(path), &text)?;
    }
    let mut roots = flags.roots.clone();
    roots.push(runner_root);
    let namespace = NamespaceFlags {
        entry: Some(test_runner::RUNNER_NAMESPACE.to_owned()),
        roots,
        ..NamespaceFlags::default()
    };
    let mut js = String::new();
    build_namespace_into(
        namespace,
        AnalysisOptions::default(),
        flags.compile,
        Some(&mut js),
    )?;
    let out: test_runner::TextOutput = Rc::new(|text| {
        let mut out = io::stdout().lock();
        out.write_all(text.as_bytes())
            .and_then(|_| out.flush())
            .map_err(|error| error.to_string())
    });
    let err: test_runner::TextOutput = Rc::new(|text| {
        let mut err = io::stderr().lock();
        err.write_all(text.as_bytes())
            .and_then(|_| err.flush())
            .map_err(|error| error.to_string())
    });
    match test_runner::run_script(&js, &flags.limits, out, err)? {
        RunOutcome::Passed => Ok(()),
        RunOutcome::Failed => Err("tests failed".into()),
        RunOutcome::Unfinished => Err(
            "the test run did not finish: a test waits on something other than a promise, \
             such as a timer, a DOM event or a Node API, which the embedded V8 heap does not \
             have; run such tests in a Node or browser build"
                .into(),
        ),
    }
}
