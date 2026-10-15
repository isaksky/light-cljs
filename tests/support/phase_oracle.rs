//! Complete-source oracles in both compiler phases and both host lifecycles.
//!
//! [`run`] compiles each case twice, once as an application and once as a macro
//! computation, and each of those twice: with a fresh `MacroHost` and with a reused
//! one. All four programs run in QuickJS and in Node, and every run must produce the
//! case's expected result. The reused host first compiles and expands an unrelated
//! macro, then serves every case of the `run` in turn, so later cases also check a
//! host that has already compiled other namespaces.
//!
//! Each case's application program is also built as a dev build
//! (docs/dev-workflow.md, "Phase 0"), which must print what its `--optimize none`
//! build prints through the Node driver `tests/support/dev_execution.mjs`.
//!
//! Feature suites group their cases into tests of a `phase_oracles` module, so
//! `cargo nextest run phase_oracles::` runs all of them. Set
//! `LIGHT_PHASE_ORACLE_ARTIFACTS` to a directory to keep every program and compiled
//! unit there; otherwise they go to a temporary directory that is removed when the
//! test ends (`LIGHT_CLJS_KEEP_TMP=1` keeps it; see `tests/support/temp_dir.rs`).
use crate::temp_dir::TempDir;
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{MacroHost, compile_namespace_with_macros};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

/// Complete upstream bodies get a deadline well above their run time, so a loaded
/// machine does not fail them.
const TIMEOUT: Duration = Duration::from_secs(120);
const OBSERVER: &str = include_str!("sci_execution_observation.js");

/// One complete source and the result that every phase, host and engine must produce.
#[derive(Clone, Copy)]
pub struct Case<'a> {
    id: &'a str,
    source: &'a str,
    expected: &'a str,
    kind: Kind<'a>,
    sources: &'a [(&'a str, &'a str)],
}

#[derive(Clone, Copy)]
enum Kind<'a> {
    /// The program prints `invoke`'s value after the namespace source; with an empty
    /// `invoke`, it prints the value of the source as an expression in `user`.
    Value { namespace: &'a str, invoke: &'a str },
    /// The program is `(fn [observe] source)`; the SCI observer encodes its outcome,
    /// `observe` trace and output (see `sci_execution_observation.js`).
    Observed(MacroPhase),
}

/// How the macro phase of an observed case produces its packet.
#[derive(Clone, Copy, PartialEq)]
enum MacroPhase {
    /// The macro returns the source's value as quoted code, which the observer
    /// encodes at run time.
    QuotedValue,
    /// The observer itself runs during expansion, effects and all; the program only
    /// returns its packet.
    Expansion,
}

impl<'a> Case<'a> {
    /// A complete expression, run in namespace `user`, whose value prints as `true`.
    pub const fn expression(id: &'a str, source: &'a str) -> Self {
        Self::namespace(id, "user", "", source)
    }

    /// A complete namespace source after which `invoke` prints as `true`. The macro
    /// phase calls `invoke` during expansion, so the namespace's functions run there.
    pub const fn namespace(
        id: &'a str,
        namespace: &'a str,
        invoke: &'a str,
        source: &'a str,
    ) -> Self {
        Self {
            id,
            source,
            expected: "true",
            kind: Kind::Value { namespace, invoke },
            sources: &[],
        }
    }

    /// The body of `(fn [observe] ...)`, whose observation packet is `expected`. The
    /// macro phase computes the value during expansion and returns it as quoted code.
    pub const fn observed(id: &'a str, source: &'a str, expected: &'a str) -> Self {
        Self {
            id,
            source,
            expected,
            kind: Kind::Observed(MacroPhase::QuotedValue),
            sources: &[],
        }
    }

    /// Like [`Case::observed`], but the macro phase runs the observer during expansion.
    pub const fn observed_during_expansion(
        id: &'a str,
        source: &'a str,
        expected: &'a str,
    ) -> Self {
        Self {
            kind: Kind::Observed(MacroPhase::Expansion),
            ..Self::observed(id, source, expected)
        }
    }

    /// Expects a value case to print `expected` instead of `true`.
    pub const fn printing(self, expected: &'a str) -> Self {
        Self { expected, ..self }
    }

    /// Adds source files (path relative to the source root, contents) that the
    /// namespace requires.
    pub const fn with_sources(self, sources: &'a [(&'a str, &'a str)]) -> Self {
        Self { sources, ..self }
    }
}

fn host() -> MacroHost {
    let mut host = MacroHost::new();
    host.evaluator_options.timeout = TIMEOUT;
    host
}

/// The working directory of one test: `LIGHT_PHASE_ORACLE_ARTIFACTS/<test>`, kept, when
/// that variable is set; otherwise a temporary directory.
fn artifacts() -> TempDir {
    // The test harness names each test's thread after the test.
    let test = std::thread::current()
        .name()
        .unwrap_or("phase-oracle")
        .replace("::", "-");
    match std::env::var_os("LIGHT_PHASE_ORACLE_ARTIFACTS") {
        Some(root) => TempDir::retained(PathBuf::from(root).join(test)),
        None => TempDir::new(&format!("phase-oracle-{test}")),
    }
}

/// Runs every case in both phases, first with a fresh host per program and then with
/// one reused host shared by all cases (see the module documentation).
pub fn run(cases: &[Case]) {
    let artifacts = artifacts();
    let mut reused = host();
    let warm = artifacts.join("warmup");
    fs::create_dir_all(&warm).unwrap();
    fs::write(
        warm.join("warm.cljs"),
        "(ns warm) (defmacro initialized [] :warm) (initialized)",
    )
    .unwrap();
    compile_namespace_with_macros("warm", &[warm], &mut reused).unwrap();
    // The observer reads values through a runtime of its own.
    let runtime = light_cljs::compile("[]").unwrap();
    for case in cases {
        for lifecycle in ["fresh", "reused"] {
            for phase in ["application", "macro"] {
                let mut fresh = host();
                let host = if lifecycle == "fresh" {
                    &mut fresh
                } else {
                    &mut reused
                };
                let root = artifacts.join(case.id).join(lifecycle).join(phase);
                fs::create_dir_all(&root).unwrap();
                let context = format!("{} {lifecycle} {phase}", case.id);
                let (entry, start) = match case.kind {
                    Kind::Value { namespace, invoke } => {
                        value(case, namespace, invoke, phase, &root, host, &context);
                        (namespace, false)
                    }
                    Kind::Observed(macro_phase) => {
                        let expansion = phase == "macro" && macro_phase == MacroPhase::Expansion;
                        observed(case, &runtime, phase, expansion, &root, host, &context);
                        ("user", true)
                    }
                };
                if lifecycle == "fresh" && phase == "application" {
                    dev_build(entry, start, &root, &context);
                }
            }
        }
    }
}

fn value(
    case: &Case,
    namespace: &str,
    invoke: &str,
    phase: &str,
    root: &Path,
    host: &mut MacroHost,
    context: &str,
) {
    let source = case.source;
    // A namespace keeps its entire setup and calls its unchanged entry point; an
    // expression is the complete macro body.
    let program = match (invoke.is_empty(), phase) {
        (true, "macro") => format!(
            "(ns user)\n(defmacro phase-witness [] (do {source}))\n(pr-str (phase-witness))\n"
        ),
        (true, _) => format!("(ns user)\n(pr-str (do {source}))\n"),
        (false, "macro") => {
            format!("{source}\n(defmacro phase-witness [] {invoke})\n(pr-str (phase-witness))\n")
        }
        (false, _) => format!("{source}\n(pr-str {invoke})\n"),
    };
    let path = root.join(namespace.replace('.', "/").replace('-', "_") + ".cljs");
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, &program).unwrap();
    for &(relative, contents) in case.sources {
        let path = root.join(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, contents).unwrap();
    }
    let js = compile_namespace_with_macros(namespace, &[root.to_path_buf()], host)
        .unwrap_or_else(|e| panic!("{context}: {e}"));
    fs::write(root.join("compiled.js"), &js).unwrap();
    let evaluator = Evaluator::new(EvaluatorOptions {
        timeout: TIMEOUT,
        ..EvaluatorOptions::default()
    })
    .unwrap();
    let actual = evaluator
        .string(evaluator.evaluate_js(&js).unwrap())
        .unwrap();
    assert_eq!(actual, case.expected, "{context} QuickJS");
    let actual = node(&format!("console.log(String({js}));"));
    assert_eq!(actual.trim(), case.expected, "{context} Node");
}

fn observed(
    case: &Case,
    runtime: &str,
    phase: &str,
    expansion: bool,
    root: &Path,
    host: &mut MacroHost,
    context: &str,
) {
    let (id, source) = (case.id, case.source);
    let program = if phase == "application" {
        format!("(ns user)\n(defn run [observe] (do {source}))\nrun\n")
    } else if expansion {
        let observer = format!("(({OBSERVER})(rt))({id:?}, ~{{}})");
        format!(
            "(ns user)\n(defn phase-original [observe] (do {source}))\n(defmacro phase-witness [] (js* {observer:?} phase-original))\n(defn run [] (phase-witness))\nrun\n"
        )
    } else {
        format!(
            "(ns user)\n(defmacro phase-witness [] (list 'quote (do {source})))\n(defn run [observe] (phase-witness))\nrun\n"
        )
    };
    fs::write(root.join("user.cljs"), &program).unwrap();
    let js = compile_namespace_with_macros("user", &[root.to_path_buf()], host)
        .unwrap_or_else(|e| panic!("{context}: {e}"));
    fs::write(root.join("compiled.js"), &js).unwrap();
    let script = if expansion {
        format!("(() => {{const run={};return run();}})()", capture(&js))
    } else {
        format!(
            "(() => {{void {};const run={};return ({OBSERVER})(globalThis.lightRuntime)('{id}',observe=>run(observe));}})()",
            capture(runtime),
            capture(&js),
        )
    };
    fs::write(root.join("script.js"), &script).unwrap();
    let quickjs = rquickjs::Runtime::new().unwrap();
    let quickjs = rquickjs::Context::full(&quickjs).unwrap();
    let actual = quickjs.with(|ctx| {
        ctx.eval::<String, _>(format!("(0, eval)({script:?})").as_bytes())
            .unwrap_or_else(|e| panic!("{context} observer: {e:?} {:?}", ctx.catch()))
    });
    assert_eq!(actual, case.expected, "{context} QuickJS");
    let actual = node(&format!("console.log({script});"));
    assert_eq!(actual, case.expected, "{context} Node");
}

/// The dev build of the application program at `root` prints what its
/// `--optimize none` build prints (docs/dev-workflow.md, "Phase 0"): an
/// observed case's program value, `run`, is called with the driver's
/// `observe` (`start`), so the trace is compared too.
fn dev_build(entry: &str, start: bool, root: &Path, context: &str) {
    let (program, sources) = light_cljs::compiler::analyze_namespace_entry_with_sources(
        entry,
        &[root.to_path_buf()],
        &mut host(),
        light_cljs::analyzer::AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|e| panic!("{context} dev build: {e}"));
    let [none, dev] =
        crate::dev_execution::none_and_dev(&program, &sources, &root.join("builds"), start);
    let none = none.unwrap_or_else(|e| panic!("{context} none build: {e}"));
    let dev = dev.unwrap_or_else(|e| panic!("{context} dev build: {e}"));
    assert_eq!(dev, none, "{context} dev build");
}

/// Exposes the runtime that a compiled unit embeds as `globalThis.lightRuntime`, so
/// the observer can inspect its values.
fn capture(source: &str) -> String {
    let anchor = " = // Original bounded runtime";
    if source.contains(anchor) {
        assert_eq!(source.matches(anchor).count(), 1);
        source.replacen(
            anchor,
            " = globalThis.lightRuntime = // Original bounded runtime",
            1,
        )
    } else {
        source.into()
    }
}

fn node(script: &str) -> String {
    let mut child = Command::new("node")
        .arg("-")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(script.as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(out.stderr.is_empty(), "unexpected Node stderr");
    String::from_utf8(out.stdout).unwrap().trim_end().into()
}
