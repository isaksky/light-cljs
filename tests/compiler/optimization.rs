//! Rust application preparation regressions.
use light_cljs::{
    ApplicationOptions, GlobalId, MacroHost, OptimizationLevel, Program, analyze,
    analyze_with_macros, compile_namespace, emit_js, parse, prepare_application,
};
use std::{
    fs,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicUsize, Ordering},
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}
/// Run `js` as a Node script and return its stdout; a nonzero exit fails.
pub(crate) fn execute(js: &str) -> String {
    let output = Command::new(std::env::var_os("NODE").unwrap_or_else(|| "node".into()))
        .args(["--eval", js])
        .output()
        .expect("optimization tests require Node");
    assert!(
        output.status.success(),
        "{}\n{js}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}
/// Print the value of the expression `js` with `console.log`.
fn result(js: &str) -> String {
    execute(&format!("console.log({js});"))
}
/// Every optimization level prints `expected`.
fn both(source: &str, expected: &str) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(&program, ApplicationOptions::new(level));
        assert_eq!(
            result(&prepared.emit_js()),
            expected,
            "optimize={level}: {source}"
        );
    }
}

#[test]
fn macro_only_helpers_are_removed_after_expansion_but_shared_helpers_survive() {
    let source = r#"
      (def optimizer-only-constant "OPTIMIZER_MACRO_ONLY_SENTINEL")
      (defn optimizer-only-helper [] optimizer-only-constant)
      (defmacro expanded [] (count (optimizer-only-helper)))
      (expanded)"#;
    let mut host = MacroHost::new();
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
    let off = prepare_application(&program, ApplicationOptions::new(OptimizationLevel::None));
    let on = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Prepare),
    );
    assert!(off.emit_js().contains("OPTIMIZER_MACRO_ONLY_SENTINEL"));
    assert!(!on.emit_js().contains("OPTIMIZER_MACRO_ONLY_SENTINEL"));
    assert!(on.stats().removed_definitions >= 2);
    assert_eq!(result(&off.emit_js()), result(&on.emit_js()));
    assert_eq!(emit_js(&program), on.emit_js());
    assert!(!on.emit_js().contains("__lightCompilerRoots"));
    both(
        r#"(def factor 2) (defn twice [x] (* factor x))
        (defmacro doubled [x] (twice x)) (pr-str [(doubled 21) (twice 21)])"#,
        "[42 42]\n",
    );
}

#[test]
fn unused_initializers_and_standalone_effects_keep_order_and_run_once() {
    let program = program(
        r#"
      (def never-read (js/connect))
      (js/trace.push "standalone")
      (def also-unused (js/trace.push "initializer"))
      (js/trace.push "last")
      42"#,
    );
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!(
                "globalThis.trace=[];globalThis.connect=()=>{{trace.push('connect');return 7;}};console.log({js});console.log(JSON.stringify(trace));"
            )),
            "42\n[\"connect\",\"standalone\",\"initializer\",\"last\"]\n"
        );
    }
}

#[test]
fn discarded_throwing_initializer_still_aborts_after_prior_effects() {
    let program = program(
        r#"(js/trace.push "before")
        (def unused (throw (js/Error. "optimizer-boom")))
        (js/trace.push "after") 42"#,
    );
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!(
                "globalThis.trace=[];try{{{js};}}catch(e){{console.log(e.message);}}console.log(JSON.stringify(trace));"
            )),
            "optimizer-boom\n[\"before\"]\n"
        );
    }
}

#[test]
fn specialization_preserves_mutable_callees_vars_and_dynamic_bindings() {
    both(
        r#"(defn f [] 1) (def saved f) (def f (fn [] 2))
        (pr-str [(saved) (f)])"#,
        "[1 2]\n",
    );
    both(
        r#"(defn f [x] (+ 1 x))
        (pr-str [(f (do (set! f (fn [x] (+ 10 x))) 2)) (f 2)])"#,
        "[3 12]\n",
    );
    both(
        r#"(def ^:dynamic f (fn [] 1))
        (pr-str [(binding [f (fn [] 2)] (f)) (f)
                 (with-redefs [f (fn [] 3)] (f)) (f)])"#,
        "[2 1 3 1]\n",
    );
    both(
        r#"(defn f [] 1) (def v (var f)) (def saved-v v) (def f (fn [] 2))
        (pr-str [(v) (@v) (identical? v saved-v) (:name (meta v))])"#,
        "[2 2 true f]\n",
    );
}

#[test]
fn retained_protocol_extensions_change_saved_dispatch() {
    both(
        r#"(defprotocol P (f [x])) (def saved f)
        (extend-type number P (f [x] (+ x 1)))
        (def before (saved 4))
        (extend-type number P (f [x] (+ x 2)))
        (pr-str [before (saved 4) (f 4)])"#,
        "[5 6 6]\n",
    );
}

#[test]
fn explicit_exports_retain_otherwise_unused_definitions() {
    let program = program(
        r#"(def exported "OPTIMIZER_EXPORT_SENTINEL")
        (def private-value "OPTIMIZER_PRIVATE_SENTINEL") 42"#,
    );
    let exported = GlobalId(
        program
            .globals
            .iter()
            .position(|binding| binding.name == "exported")
            .unwrap(),
    );
    let without = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Prepare),
    );
    let with = prepare_application(
        &program,
        ApplicationOptions {
            optimize: OptimizationLevel::Prepare,
            exports: vec![exported],
        },
    );
    assert!(!without.emit_js().contains("OPTIMIZER_EXPORT_SENTINEL"));
    assert!(with.emit_js().contains("OPTIMIZER_EXPORT_SENTINEL"));
    assert!(!with.emit_js().contains("OPTIMIZER_PRIVATE_SENTINEL"));
    assert_eq!(
        without.stats().removed_definitions,
        with.stats().removed_definitions + 1
    );
    assert_eq!(result(&with.emit_js()), "42\n");
}

static NEXT: AtomicUsize = AtomicUsize::new(0);
/// A temporary source root, removed on drop.
pub(crate) struct Sources(pub(crate) PathBuf);
impl Sources {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-cljs-optimization-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&path).unwrap();
        Self(path)
    }
    /// Write `source` at the relative path `name`, creating its directories.
    pub(crate) fn write(&self, name: &str, source: &str) {
        let path = self.0.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, source).unwrap();
    }
}
impl Drop for Sources {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn macro_only_require_retains_namespace_initialization_effects() {
    let sources = Sources::new();
    sources.write(
        "tools.cljs",
        r#"(ns tools)
      (def unused (js/trace.push "tools-initializer"))
      (js/trace.push "tools-standalone")
      (defn helper [] "OPTIMIZER_NAMESPACE_SENTINEL")
      (defmacro answer [] (count (helper)))"#,
    );
    sources.write(
        "app.cljs",
        r#"(ns app (:require [tools :refer [answer]]))
      (js/trace.push "app") (answer)"#,
    );
    let js = compile_namespace("app", std::slice::from_ref(&sources.0)).unwrap();
    assert!(!js.contains("OPTIMIZER_NAMESPACE_SENTINEL"));
    assert_eq!(
        execute(&format!(
            "globalThis.trace=[];{js};console.log(JSON.stringify(trace));"
        )),
        "[\"tools-initializer\",\"tools-standalone\",\"app\"]\n"
    );
}

#[test]
fn immutable_multi_arity_and_core_targets_specialize_in_rust() {
    for source in [
        "(defn f ([x] x) ([x y] (+ x y))) (f 21 21)",
        "(def f +) (f 21 21)",
    ] {
        let program = program(source);
        let on = prepare_application(&program, ApplicationOptions::default());
        let off = prepare_application(&program, ApplicationOptions::new(OptimizationLevel::None));
        assert_eq!(on.stats().specialized_calls, 1, "{source}");
        assert_eq!(off.stats().specialized_calls, 0, "{source}");
        assert_eq!(result(&on.emit_js()), "42\n");
        assert_eq!(result(&off.emit_js()), "42\n");
        assert_eq!(on.emit_js(), on.emit_js());
    }
}

#[test]
fn inert_macro_data_and_functions_containing_raw_js_do_not_become_runtime_roots() {
    let source = r#"
        (def data {:only "PURE_MAP_SENTINEL"})
        (def choices #{:a :b})
        (defn raw-helper [] (js* "41 + 1"))
        (defmacro value [] (get data :only) (contains? choices :a) (raw-helper))
        (value)"#;
    let program = program(source);
    let prepared = prepare_application(&program, Default::default());
    assert_eq!(prepared.stats().removed_definitions, 3);
    let js = prepared.emit_js();
    assert!(!js.contains("PURE_MAP_SENTINEL"));
    assert!(!js.contains("41 + 1"));
    assert_eq!(result(&js), "42\n");
}

#[test]
fn observed_definition_results_and_nonconstant_set_failures_are_preserved() {
    both("(def f 42)", "42\n");
    let source = r#"(def one 1) (def other 1)
        (def unused #{one other}) 42"#;
    let p = program(source);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&p, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!(
                "try {{{js};}} catch (e) {{console.log(e.message.includes('Duplicate'));}}"
            )),
            "true\n"
        );
    }
}

#[test]
fn mutable_escaped_dynamic_and_raw_targets_disable_fixed_entry_specialization() {
    for source in [
        "(defn f ([x] x) ([x y] (+ x y))) (defn f ([x] x) ([x y] (+ x y))) (f 21 21)",
        "(defn f ([x] x) ([x y] (+ x y))) (def saved f) (f 21 21)",
        "(defn f ([x] x) ([x y] (+ x y))) (def v (var f)) (f 21 21)",
        "(def ^:dynamic f (fn ([x] x) ([x y] (+ x y)))) (f 21 21)",
        "(defn f ([x] x) ([x y] (+ x y))) (with-redefs [f (fn [x y] 0)] (f 1 2)) (f 21 21)",
        "(defn f ([x] x) ([x y] (+ x y))) (js* \"null\") (f 21 21)",
    ] {
        let p = program(source);
        let on = prepare_application(&p, Default::default());
        assert_eq!(on.stats().specialized_calls, 0, "{source}");
        assert_eq!(result(&on.emit_js()), "42\n", "{source}");
    }
}
