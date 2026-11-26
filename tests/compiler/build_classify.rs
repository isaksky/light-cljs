//! `light.build/classify`: what a symbol names where a macro expands
//! (`:fn-var`, `:var`, `:macro`, `:special-form`, `:local`, `:js` or
//! `:unresolved`), the compile-time question upstream `cljs.test` asks
//! `cljs.analyzer.api/resolve` for `:fn-var` (docs/compiler.md,
//! "Compile-time services: light.build"). Each case is checked in a clean
//! compilation, through the persistent compiler cache (warm, and with the
//! caller recompiled against cached artifacts), and in an interactive
//! session.
use crate::temp_dir::TempDir;
use light_cljs::compiler::analyze_namespace_entry_with_dependencies;
use light_cljs::dependency_manifest::EdgeKinds;
use light_cljs::{EvalOptions, EvalSession, MacroHost, analyzer::AnalysisOptions};
use std::{fs, path::Path, process::Command};

const MACROS: &str = r#"(ns probe.macros (:require [light.build :as b]))
(defmacro kinds [& syms] (pr-str (mapv #(b/classify &env %) syms)))
(defmacro user-macro [x] x)"#;

const LIBRARY: &str = r#"(ns probe.lib)
(defn helper [x] x)
(defn- hidden [x] x)
(def value 42)
(def literal (fn [x] x))
(defmulti dispatch identity)
(defprotocol Shape (area [s]))
(defrecord Square [n])"#;

/// The caller: every case, then the expected answers.
const MAIN: &str = r#"(ns probe.main
  (:require [probe.macros :refer [kinds user-macro]]
            [probe.lib :as l :refer [helper]]))
(defn own [x] x)
(def own-value 1)
(def results
  (let [x 1]
    [(kinds = pos? string? apply str)
     (kinds own helper l/helper probe.lib/helper l/literal l/area l/->Square)
     (kinds own-value l/value l/dispatch l/Square)
     (kinds and when defn cljs.core/when user-macro probe.macros/user-macro)
     (kinds if let* fn* do quote String.)
     (kinds .startsWith .-length js/isNaN js/Array.isArray)
     (kinds x)
     (kinds missing l/missing nowhere/thing)]))
(js/console.log (pr-str results))"#;

const EXPECTED: &str = r#"["[:fn-var :fn-var :fn-var :fn-var :fn-var]" "[:fn-var :fn-var :fn-var :fn-var :fn-var :fn-var :fn-var]" "[:var :var :var :var]" "[:macro :macro :macro :macro :macro :macro]" "[:special-form :special-form :special-form :special-form :special-form :special-form]" "[:js :js :js :js]" "[:local]" "[:unresolved :unresolved :unresolved]"]"#;

fn write(root: &Path, path: &str, text: &str) {
    let path = root.join(path);
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(path, text).unwrap();
}

fn project() -> TempDir {
    let root = TempDir::new("cljs-build-classify");
    write(root.path(), "src/probe/macros.cljs", MACROS);
    write(root.path(), "src/probe/lib.cljs", LIBRARY);
    write(root.path(), "src/probe/main.cljs", MAIN);
    root
}

fn node(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--input-type=commonjs", "-"])
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write as _;
            child.stdin.take().unwrap().write_all(js.as_bytes())?;
            child.wait_with_output()
        })
        .expect("compiler integration requires Node");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .trim_end()
        .to_owned()
}

#[test]
fn classify_answers_every_kind_in_a_clean_compilation() {
    let root = project();
    let mut host = MacroHost::new();
    let (program, _, manifest) = analyze_namespace_entry_with_dependencies(
        "probe.main",
        &[root.join("src")],
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert_eq!(node(&light_cljs::emit_js(&program)), EXPECTED);
    // The answers about probe.lib's names are an observed dependency on it.
    let main = manifest.id("probe.main").unwrap();
    let lib = manifest.id("probe.lib").unwrap();
    let kinds = manifest
        .edge(main, lib)
        .expect("an edge from probe.main to probe.lib");
    assert!(kinds.contains(EdgeKinds::OBSERVED), "{kinds:?}");
}

fn compile(root: &Path, cached: bool, extra: &[&str]) -> (String, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_light-cljs"));
    command
        .args(["--compile", "--entry", "probe.main", "--source-root"])
        .arg(root.join("src"))
        .arg("--stats");
    if cached {
        command.arg("--cache-dir").arg(root.join("cache"));
    } else {
        command.arg("--no-cache");
    }
    command.args(extra);
    let output = command.output().unwrap();
    let stats = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "the build fails: {stats}");
    (String::from_utf8(output.stdout).unwrap(), stats)
}

fn stat(stats: &str, name: &str) -> u64 {
    stats
        .lines()
        .find_map(|line| {
            let mut fields = line.split_whitespace();
            (fields.next() == Some(name)).then(|| fields.next()?.parse().ok())?
        })
        .unwrap_or_else(|| panic!("no {name} in {stats}"))
}

#[test]
fn classify_answers_match_through_the_compiler_cache() {
    let root = project();
    let (oracle, _) = compile(root.path(), false, &[]);
    assert_eq!(node(&oracle), EXPECTED);
    let (cold, _) = compile(root.path(), true, &[]);
    assert_eq!(cold, oracle);
    let (warm, stats) = compile(root.path(), true, &[]);
    assert_eq!(warm, oracle);
    assert!(stat(&stats, "cache.hits") >= 3, "{stats}");
    // Recompiling only the caller asks about probe.lib's names against its
    // installed artifact, which keeps which of its Vars are functions.
    write(
        root.path(),
        "src/probe/main.cljs",
        &MAIN.replace("(def own-value 1)", "(def own-value 2)"),
    );
    let (edited, stats) = compile(root.path(), true, &[]);
    assert!(stat(&stats, "cache.hits") >= 2, "{stats}");
    assert_eq!(node(&edited), EXPECTED);
    // Redefining a library function as a value changes the caller's answer.
    write(
        root.path(),
        "src/probe/lib.cljs",
        &LIBRARY.replace("(defn helper [x] x)", "(def helper 7)"),
    );
    let (changed, _) = compile(root.path(), true, &[]);
    assert_eq!(
        node(&changed),
        EXPECTED.replacen(
            "[:fn-var :fn-var :fn-var :fn-var :fn-var :fn-var :fn-var]",
            "[:fn-var :var :var :var :fn-var :fn-var :fn-var]",
            1
        )
    );
}

#[test]
fn classify_answers_in_an_interactive_session() {
    let root = project();
    let mut session = EvalSession::new(EvalOptions {
        source_provider: Some(std::rc::Rc::new(
            light_cljs::source_provider::FileSourceProvider::new(vec![root.join("src")]),
        )),
        ..Default::default()
    })
    .unwrap();
    let mut printed = |source: &str| {
        let value = session.eval_string(source).unwrap().value;
        let text = session.print(value).unwrap();
        session.release(value).unwrap();
        text
    };
    let forms = MAIN.replace("(js/console.log (pr-str results))", "results");
    assert_eq!(printed(&forms), EXPECTED);
    // A later form sees a redefinition in the session.
    assert_eq!(printed("(def own 3) (kinds own)"), "\"[:var]\"");
    assert_eq!(
        printed("(defn own-value [] 1) (kinds own-value)"),
        "\"[:fn-var]\""
    );
}

#[test]
fn classify_is_only_available_while_a_macro_expands() {
    let root = project();
    write(
        root.path(),
        "src/probe/runtime.cljs",
        "(ns probe.runtime (:require [light.build :as b])) \
         (js/console.log (try (b/classify {} 'x) (catch :default e (.-message e))))",
    );
    let mut host = MacroHost::new();
    let (program, _, _) = analyze_namespace_entry_with_dependencies(
        "probe.runtime",
        &[root.join("src")],
        &mut host,
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    assert!(node(&light_cljs::emit_js(&program)).contains("only available at compile time"),);
}

/// Macros expanding on pool workers answer from their own thread's
/// declarations, not the coordinator's, as one job does.
#[test]
fn classify_answers_match_on_parallel_workers() {
    let root = project();
    let (oracle, _) = compile(root.path(), false, &[]);
    let (parallel, stats) = compile(
        root.path(),
        false,
        &["--jobs", "4", "--macro-memory-limit", "268435456"],
    );
    assert_eq!(parallel, oracle, "{stats}");
    assert!(stat(&stats, "pool.worker-isolates") > 0, "{stats}");
    assert_eq!(node(&parallel), EXPECTED);
}
