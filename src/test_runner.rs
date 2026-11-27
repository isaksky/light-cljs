//! Test-namespace discovery, the synthesized runner namespaces and the
//! embedded V8 run of `light-cljs test` (docs/cli.md, "light-cljs test").
//!
//! [`test_namespaces`] lists the namespaces of the `.cljs` and `.cljc` files
//! under a build's source roots that a run selects: those named, and those
//! whose name a regular expression finds a match in, `-test$` by default, as
//! shadow-cljs's `:ns-regexp` does. The regular expression is JavaScript's,
//! the dialect of `#"..."` in ClojureScript, so it is matched in a V8 heap.
//! [`runner_sources`] writes the entry namespace that requires them, records
//! the summary `cljs.test` reports when the run ends, and runs them, and
//! [`run_script`] runs the compiled program in a fresh V8 heap with
//! `console` writing to the host, draining promise jobs, so synchronous and
//! promise-based (`^:async`, `async`) tests finish before it reads the
//! summary. Timers, the DOM and Node's APIs do not exist in that heap.
//!
//! [`TestBuild`] is a `:node-test` or `:browser-test` build of
//! `light-cljs.edn` (docs/build-configuration.md, "Test builds"): the same
//! discovery, a runner of [`build_runner_sources`] written under the cache
//! root, and for `:autorun` a run of the output in Node.
use crate::eval_session::{HostArity, HostCallback};
use crate::evaluator::{EvalEngine, EvaluatorOptions, session_evaluator};
use std::{
    fs, io,
    path::{Path, PathBuf},
    rc::Rc,
};

/// The namespaces a run selects when it names none and gives no pattern:
/// shadow-cljs's `:ns-regexp` default.
pub const DEFAULT_NS_REGEXP: &str = "-test$";

/// The synthesized entry namespace: it requires the test namespaces and runs them.
pub const RUNNER_NAMESPACE: &str = "light-cljs.test-runner";

/// The synthesized namespace the runner requires first, which sends printing
/// to `console` before any test namespace loads.
pub const CONSOLE_NAMESPACE: &str = "light-cljs.test-console";

/// The global the runner sets when `cljs.test` reports the end of the run:
/// `true` when every test passed, `false` otherwise.
const RESULT_GLOBAL: &str = "lightCljsTestPassed";

/// The namespace of every `.cljs` and `.cljc` file under `roots`, named by
/// its path as a require resolves it (`app/sum_test.cljs` is
/// `app.sum-test`), sorted and without duplicates. Hidden directories and
/// directories reached through a symbolic link are skipped; a root that does
/// not exist is an error.
pub fn source_namespaces(roots: &[PathBuf]) -> Result<Vec<String>, String> {
    let mut namespaces = Vec::new();
    for root in roots {
        if !root.is_dir() {
            return Err(format!("source root {} is not a directory", root.display()));
        }
        walk(root, &mut Vec::new(), &mut namespaces)
            .map_err(|error| format!("cannot read {}: {error}", root.display()))?;
    }
    namespaces.sort();
    namespaces.dedup();
    Ok(namespaces)
}

fn walk(dir: &Path, segments: &mut Vec<String>, out: &mut Vec<String>) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        // A hidden entry, or one with a hyphen, which no require resolves
        // (`app.sum-test` is `app/sum_test.cljs`).
        if name.starts_with('.') || name.contains('-') {
            continue;
        }
        let kind = entry.file_type()?;
        let is_file = kind.is_file() || (kind.is_symlink() && entry.path().is_file());
        if kind.is_dir() {
            if name.contains('.') {
                continue;
            }
            segments.push(name.replace('_', "-"));
            walk(&entry.path(), segments, out)?;
            segments.pop();
        } else if is_file {
            let Some(stem) = name
                .strip_suffix(".cljs")
                .or_else(|| name.strip_suffix(".cljc"))
            else {
                continue;
            };
            if stem.is_empty() || stem.contains('.') {
                continue;
            }
            let mut namespace = segments.join(".");
            if !namespace.is_empty() {
                namespace.push('.');
            }
            namespace.push_str(&stem.replace('_', "-"));
            out.push(namespace);
        }
    }
    Ok(())
}

/// The test namespaces under `roots` that a run selects, sorted: each of
/// `names`, which must be there, and each whose name `ns_regexp` (a
/// JavaScript regular expression) finds a match in; with neither,
/// [`DEFAULT_NS_REGEXP`]. The synthesized namespaces are never selected.
pub fn test_namespaces(
    roots: &[PathBuf],
    names: &[String],
    ns_regexp: Option<&str>,
) -> Result<Vec<String>, String> {
    let available = source_namespaces(roots)?
        .into_iter()
        .filter(|namespace| namespace != RUNNER_NAMESPACE && namespace != CONSOLE_NAMESPACE)
        .collect::<Vec<_>>();
    let mut selected = Vec::new();
    for name in names {
        if !available.contains(name) {
            return Err(format!(
                "no namespace {name} under the source roots {}",
                describe_roots(roots)
            ));
        }
        selected.push(name.clone());
    }
    let pattern = match (ns_regexp, names.is_empty()) {
        (Some(pattern), _) => Some(pattern),
        (None, true) => Some(DEFAULT_NS_REGEXP),
        (None, false) => None,
    };
    if let Some(pattern) = pattern {
        selected.extend(matching(pattern, &available)?);
    }
    selected.sort();
    selected.dedup();
    Ok(selected)
}

/// The roots, for messages: `src, test`.
pub fn describe_roots(roots: &[PathBuf]) -> String {
    roots
        .iter()
        .map(|root| root.display().to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

/// The `names` in which the JavaScript regular expression `pattern` finds a
/// match, in order.
pub fn matching(pattern: &str, names: &[String]) -> Result<Vec<String>, String> {
    if names.is_empty() {
        return Ok(Vec::new());
    }
    // The default needs no engine: no namespace name holds a line break.
    if pattern == DEFAULT_NS_REGEXP {
        return Ok(names
            .iter()
            .filter(|name| name.ends_with("-test"))
            .cloned()
            .collect());
    }
    let engine = session_evaluator(EvalEngine::V8, EvaluatorOptions::default())
        .map_err(|error| error.to_string())?;
    engine
        .begin_operation(&EvaluatorOptions::default())
        .map_err(|error| error.to_string())?;
    let list = names
        .iter()
        .map(|name| json_string(name))
        .collect::<Vec<_>>()
        .join(",");
    let source = format!(
        "(() => {{ let pattern; try {{ pattern = new RegExp({}); }} catch (error) {{ return 'E' + error.message; }} return [{list}].map(name => pattern.test(name) ? '1' : '0').join(''); }})()",
        json_string(pattern)
    );
    let value = engine
        .evaluate_js(&source)
        .map_err(|error| error.to_string())?;
    let flags = engine.string(value).map_err(|error| error.to_string())?;
    if let Some(message) = flags.strip_prefix('E') {
        return Err(format!("invalid regular expression {pattern}: {message}"));
    }
    Ok(names
        .iter()
        .zip(flags.chars())
        .filter(|(_, flag)| *flag == '1')
        .map(|(name, _)| name.clone())
        .collect())
}

/// `text` as a JSON (and JavaScript) string literal.
fn json_string(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{2028}' => out.push_str("\\u2028"),
            '\u{2029}' => out.push_str("\\u2029"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The synthesized namespaces' files, as paths relative to a source root and
/// their text: [`CONSOLE_NAMESPACE`], which enables console printing, and
/// [`RUNNER_NAMESPACE`], which requires it and then `namespaces`, replaces
/// `cljs.test`'s `:end-run-tests` report of the default reporter to record
/// whether the run succeeded, and calls `run-tests` on them.
pub fn runner_sources(namespaces: &[String]) -> [(&'static str, String); 2] {
    let console = format!("(ns {CONSOLE_NAMESPACE})\n(enable-console-print!)\n");
    let mut requires = String::new();
    let mut quoted = String::new();
    for namespace in namespaces {
        requires.push_str(&format!("\n            [{namespace}]"));
        quoted.push_str(&format!(" '{namespace}"));
    }
    let runner = format!(
        ";; Generated by light-cljs test; rewritten on every run.\n\
         (ns {RUNNER_NAMESPACE}\n  (:require [{CONSOLE_NAMESPACE}]\n            [cljs.test :as t]{requires}))\n\n\
         (defmethod t/report [::t/default :end-run-tests] [m]\n  \
         (set! (.-{RESULT_GLOBAL} js/globalThis) (t/successful? m)))\n\n\
         (t/run-tests{quoted})\n"
    );
    [
        ("light_cljs/test_console.cljs", console),
        ("light_cljs/test_runner.cljs", runner),
    ]
}

/// The test target of a build of `light-cljs.edn`.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestTarget {
    /// `:node-test`: the runner runs once in Node, which it tells, through
    /// `process.exitCode`, whether every test passed.
    Node,
    /// `:browser-test`: the runner runs in a page, and again after every
    /// reload the dev client applies.
    Browser,
}

impl TestTarget {
    /// The target a `:target` keyword names, without its colon.
    pub fn named(target: &str) -> Option<Self> {
        match target {
            "node-test" => Some(Self::Node),
            "browser-test" => Some(Self::Browser),
            _ => None,
        }
    }

    /// The keyword, without its colon.
    pub fn name(self) -> &'static str {
        match self {
            Self::Node => "node-test",
            Self::Browser => "browser-test",
        }
    }
}

/// The global a test build's runner sets when `cljs.test` reports the end of
/// a run: an object of its `test`, `pass`, `fail` and `error` counts.
pub const BUILD_RESULT_GLOBAL: &str = "lightCljsTestResult";

/// The runner namespaces of a test build of `target` that runs
/// `namespaces`, as paths relative to its runner root and their text:
/// [`CONSOLE_NAMESPACE`], and [`RUNNER_NAMESPACE`], which requires it and
/// the test namespaces, records each run's counts in
/// [`BUILD_RESULT_GLOBAL`] and prints the summary (the default reporter's).
///
/// A Node runner runs the tests when it loads and sets `process.exitCode`
/// to 1 when one failed or threw. A browser runner runs them when it first
/// loads, again from its `^:dev/after-load` function after each reload
/// (of a test, of the code it tests, or of the runner when a test namespace
/// appears), and shows the counts in the page's title.
pub fn build_runner_sources(
    target: TestTarget,
    namespaces: &[String],
) -> [(&'static str, String); 2] {
    let console = format!("(ns {CONSOLE_NAMESPACE})\n(enable-console-print!)\n");
    let mut requires = String::new();
    let mut quoted = String::new();
    for namespace in namespaces {
        requires.push_str(&format!("\n            [{namespace}]"));
        quoted.push_str(&format!(" '{namespace}"));
    }
    let head = format!(
        ";; Generated by light-cljs for a :{} build; rewritten when its test\n\
         ;; namespaces change.\n\
         (ns {RUNNER_NAMESPACE}\n  (:require [{CONSOLE_NAMESPACE}]\n            [cljs.test :as t]{requires}))\n\n",
        target.name()
    );
    let record = format!(
        "(set! (.-{BUILD_RESULT_GLOBAL} js/globalThis)\n        \
         #js {{:test (:test m 0) :pass (:pass m 0) :fail (:fail m 0) :error (:error m 0)}})"
    );
    let runner = match target {
        TestTarget::Node => format!(
            "{head}(defmethod t/report [::t/default :end-run-tests] [m]\n  \
             {record}\n  \
             (when (and (exists? js/process) (not (t/successful? m)))\n    \
             (set! (.-exitCode js/process) 1)))\n\n\
             ;; A run Node ends before cljs.test reports its end, as when an\n\
             ;; async test never calls done, fails.\n\
             (when (exists? js/process)\n  \
             (.on js/process \"exit\"\n       \
             (fn [_]\n         \
             (when (undefined? (.-{BUILD_RESULT_GLOBAL} js/globalThis))\n           \
             (js/console.error \"The test run did not finish: a test never called done\")\n           \
             (set! (.-exitCode js/process) 1)))))\n\n\
             (t/run-tests{quoted})\n"
        ),
        TestTarget::Browser => format!(
            "{head}(defmethod t/report [::t/default :end-run-tests] [m]\n  \
             {record}\n  \
             (when (exists? js/document)\n    \
             (set! (.-title js/document)\n          \
             (str (if (t/successful? m) \"PASS\" \"FAIL\") \": \" (:test m 0) \" tests, \"\n               \
             (:fail m 0) \" failures, \" (:error m 0) \" errors\"))))\n\n\
             (defn run []\n  (t/run-tests{quoted}))\n\n\
             (defn ^:dev/after-load rerun []\n  \
             (println \"\\nRunning the tests again after a reload\")\n  \
             (run))\n\n\
             (defonce started (do (run) true))\n"
        ),
    };
    [
        ("light_cljs/test_console.cljs", console),
        ("light_cljs/test_runner.cljs", runner),
    ]
}

/// A `:node-test` or `:browser-test` build: what selects its tests and
/// where its runner is written.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TestBuild {
    pub target: TestTarget,
    /// The build's source roots, without the runner root.
    pub roots: Vec<PathBuf>,
    /// `:ns-regexp`; [`DEFAULT_NS_REGEXP`] when `None`.
    pub ns_regexp: Option<String>,
    /// `:autorun`, of a `:node-test` build: run it in Node after each build.
    pub autorun: bool,
    /// The directory the runner namespaces are written under, one more
    /// source root: `<:cache-root>/test-runner/<build id>`.
    pub runner_root: PathBuf,
}

impl TestBuild {
    /// The test namespaces the build selects now, sorted.
    pub fn namespaces(&self) -> Result<Vec<String>, String> {
        test_namespaces(&self.roots, &[], self.ns_regexp.as_deref()).map_err(|error| {
            if error.starts_with("invalid regular expression") {
                format!(":ns-regexp: {error}")
            } else {
                error
            }
        })
    }

    /// Discover the test namespaces and write the runner for them, leaving
    /// files that already hold their text as they are: the paths written.
    /// No test namespace is an error.
    pub fn write_runner(&self) -> Result<Vec<PathBuf>, String> {
        let namespaces = self.namespaces()?;
        if namespaces.is_empty() {
            return Err(format!(
                "no test namespaces match :ns-regexp {} under the source roots {}",
                self.ns_regexp.as_deref().unwrap_or(DEFAULT_NS_REGEXP),
                describe_roots(&self.roots)
            ));
        }
        let mut written = Vec::new();
        for (path, text) in build_runner_sources(self.target, &namespaces) {
            let path = self.runner_root.join(path);
            if fs::read_to_string(&path).is_ok_and(|old| old == text) {
                continue;
            }
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent)
                    .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
            }
            fs::write(&path, text)
                .map_err(|error| format!("cannot write {}: {error}", path.display()))?;
            written.push(path);
        }
        Ok(written)
    }

    /// The file `node` runs of the build's output in `out_dir`: the dev
    /// build's `index.js`, or the release build's startup chunk.
    pub fn node_file(out_dir: &Path, release: bool) -> PathBuf {
        if release {
            out_dir.join(format!("{RUNNER_NAMESPACE}.js"))
        } else {
            out_dir.join("index.js")
        }
    }

    /// Run the build's output in `out_dir` in Node, `node FILE` with the
    /// process's own standard streams: whether it exited with status 0,
    /// which the runner leaves only when every test passed.
    pub fn run_node(out_dir: &Path, release: bool) -> Result<bool, String> {
        let file = Self::node_file(out_dir, release);
        let status = std::process::Command::new("node")
            .arg(&file)
            .stdin(std::process::Stdio::null())
            .status()
            .map_err(|error| format!("cannot run node {}: {error}", file.display()))?;
        Ok(status.success())
    }
}

/// What [`run_script`] found once the program and its promise jobs ran.
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum RunOutcome {
    /// `cljs.test` reported the end of the run, and every test passed.
    Passed,
    /// It reported the end of the run, with failures or errors.
    Failed,
    /// The jobs ran out before it reported the end: a test waits on
    /// something other than a promise, such as a timer.
    Unfinished,
}

/// Where printed text goes: `console.log` and `console.info` to `out`,
/// `console.error`, `console.warn` and `console.debug` to `err`.
pub type TextOutput = Rc<dyn Fn(&str) -> Result<(), String>>;

/// Run `js`, a compiled program written as one JavaScript expression (the
/// output of `--compile --entry` without `--esm`) whose entry is a runner of
/// [`runner_sources`], in a fresh V8 heap under `limits`, then run its
/// promise jobs to their end, and say whether the tests passed. An exception
/// the program or a job throws, a limit it reaches and a promise rejected
/// with no handler are errors.
pub fn run_script(
    js: &str,
    limits: &EvaluatorOptions,
    out: TextOutput,
    err: TextOutput,
) -> Result<RunOutcome, String> {
    let message = |error: crate::evaluator::EvalError| error.0;
    let engine = session_evaluator(EvalEngine::V8, limits.clone()).map_err(message)?;
    engine.begin_operation(limits).map_err(message)?;
    let printer = |output: TextOutput| -> HostCallback {
        Rc::new(move |call, args| {
            let text = call.string(args[0])?;
            output(&text).map_err(crate::evaluator::EvalError)?;
            call.value(&crate::compiler_data::Datum::new(
                crate::compiler_data::DatumKind::Nil,
            ))
        })
    };
    let print = engine
        .install_host_function("lightCljsTestOut", HostArity::Exact(1), printer(out))
        .map_err(message)?;
    let print_err = engine
        .install_host_function("lightCljsTestErr", HostArity::Exact(1), printer(err))
        .map_err(message)?;
    let install = engine
        .evaluate_js(
            "(out, err) => {
  const text = args => args.map(value => { try { return String(value); } catch { return Object.prototype.toString.call(value); } }).join(' ') + '\\n';
  const to = sink => (...args) => { sink(text(args)); };
  const none = () => {};
  globalThis.console = {log: to(out), info: to(out), dir: to(out), group: to(out), groupCollapsed: to(out),
    error: to(err), warn: to(err), debug: to(err), trace: to(err),
    assert: (ok, ...args) => { if (!ok) to(err)('Assertion failed:', ...args); },
    groupEnd: none, time: none, timeEnd: none, timeLog: none, count: none, countReset: none, table: to(out)};
  return null;
}",
        )
        .map_err(message)?;
    let installed = engine.call(install, &[print, print_err]).map_err(message)?;
    for id in [installed, install, print, print_err] {
        engine.release(id).map_err(message)?;
    }
    let ran = engine.evaluate_js(js);
    // The jobs run after an exception too, as after a script's in Node,
    // but the program's exception is the error.
    let jobs = engine.run_jobs(ran.is_err());
    let value = ran.map_err(message)?;
    engine.release(value).map_err(message)?;
    jobs.map_err(message)?;
    let rejected = engine.unhandled_rejections(1, false).map_err(message)?;
    if let Some(reason) = rejected.first() {
        return Err(format!("unhandled promise rejection: {reason}"));
    }
    let result = engine
        .evaluate_js(&format!(
            "globalThis.{RESULT_GLOBAL} === true ? 1 : globalThis.{RESULT_GLOBAL} === false ? 0 : -1"
        ))
        .map_err(message)?;
    let state = engine.number(result).map_err(message)?;
    engine.release(result).map_err(message)?;
    Ok(if state == 1.0 {
        RunOutcome::Passed
    } else if state == 0.0 {
        RunOutcome::Failed
    } else {
        RunOutcome::Unfinished
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runner_requires_and_runs_each_namespace() {
        let [(console_path, console), (runner_path, runner)] =
            runner_sources(&["app.a-test".into(), "app.b-test".into()]);
        assert_eq!(console_path, "light_cljs/test_console.cljs");
        assert_eq!(runner_path, "light_cljs/test_runner.cljs");
        assert!(console.contains("(enable-console-print!)"));
        assert!(runner.contains("[light-cljs.test-console]"));
        assert!(runner.contains("[app.a-test]"));
        assert!(runner.contains("(t/run-tests 'app.a-test 'app.b-test)"));
    }

    #[test]
    fn build_runners_set_node_exit_codes_and_rerun_pages_after_reloads() {
        let names = ["app.a-test".to_owned()];
        let [_, (_, node)] = build_runner_sources(TestTarget::Node, &names);
        assert!(node.contains("(set! (.-exitCode js/process) 1)"));
        assert!(node.contains("(.on js/process \"exit\""));
        assert!(node.ends_with("(t/run-tests 'app.a-test)\n"));
        assert!(!node.contains("after-load"));
        let [_, (_, page)] = build_runner_sources(TestTarget::Browser, &names);
        assert!(!page.contains("js/process"));
        assert!(page.contains("(defn ^:dev/after-load rerun []"));
        assert!(page.ends_with("(defonce started (do (run) true))\n"));
        for runner in [node, page] {
            assert!(runner.contains(&format!("(.-{BUILD_RESULT_GLOBAL} js/globalThis)")));
        }
    }

    #[test]
    fn json_strings_escape_quotes_and_controls() {
        assert_eq!(json_string("a\"b\\c\n"), "\"a\\\"b\\\\c\\u000a\"");
    }
}
