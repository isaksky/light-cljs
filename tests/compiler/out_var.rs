//! `cljs.core/*out*` dynamic Var: root nil value, `binding` with
//! `StringBufferWriter` and `reify` `IWriter`, `set!` inside `binding`,
//! `(var *out*)`, independence from core printing (`print`/`println`/`pr`/
//! `prn`/`with-out-str`), macro computations, `--compile` at every `--optimize`
//! level, reloadable dev builds, `--eval`, and `--repl` (light-cljs-1167).

use super::optimization::{execute, readable};
use crate::dev_execution::none_and_dev;
use crate::temp_dir::TempDir;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry_with_sources;
use light_cljs::emitter::emit_js_unit;
use light_cljs::evaluator::RUNTIME_SOURCE;
#[cfg(light_v8)]
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::runtime_manifest::CORE_HUB;
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, Program, analyze_with_macros, parse,
    prepare_application,
};
use std::fs;
use std::io::Write;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// A self-contained expression that checks all `*out*` contracts and returns
/// a deterministic summary vector.
const OUT_CONTRACT_BODY: &str = r#"(let [mk-buf (fn []
                #js {:text ""
                     :append (fn [s]
                               (this-as this
                                 (set! (.-text this) (str (.-text this) s))
                                 this))})
      root-nils [(nil? *out*)
                 (nil? cljs.core/*out*)
                 (nil? clojure.core/*out*)]
      v (var *out*)
      var-checks [(var? v)
                  (= v #'cljs.core/*out*)
                  (= v (resolve '*out*))
                  (= 'cljs.core/*out* (.-sym v))
                  (true? (:dynamic (meta v)))
                  (nil? @v)]
      sb1 (mk-buf)
      sb-write (binding [*out* (StringBufferWriter. sb1)]
                 (-write *out* "x")
                 (-write *out* "y")
                 [(identical? *out* @v) (.-text sb1)])
      restored-after-sb (nil? *out*)
      sb2 (mk-buf)
      thrown-restored (let [caught (try
                                     (binding [*out* (->StringBufferWriter sb2)]
                                       (-write *out* "before")
                                       (throw (js/Error. "boom")))
                                     (catch js/Error e (.-message e)))]
                        [caught (.-text sb2) (nil? *out*)])
      events (atom [])
      reified-writer (reify IWriter
                       (-write [this s] (swap! events conj [:write s]) this)
                       (-flush [_] (swap! events conj [:flush]) nil))
      reify-res (binding [*out* reified-writer]
                  (-write *out* "hello")
                  (-flush *out*)
                  [(identical? *out* reified-writer) @events])
      restored-after-reify (nil? *out*)
      sb-a (mk-buf)
      sb-b (mk-buf)
      set-in-binding (binding [*out* (StringBufferWriter. sb-a)]
                       (-write *out* "a")
                       (set! *out* (StringBufferWriter. sb-b))
                       (-write *out* "b")
                       [(.-text sb-a) (.-text sb-b) (identical? *out* @v)])
      restored-after-set (nil? *out*)
      nested (let [outer (mk-buf)
                   inner (mk-buf)]
               (binding [*out* (StringBufferWriter. outer)]
                 (-write *out* "1")
                 (binding [*out* (StringBufferWriter. inner)]
                   (-write *out* "2"))
                 (-write *out* "3")
                 [(.-text outer) (.-text inner)]))
      restored-after-nested (nil? *out*)
      print-log (atom [])
      out-log (atom [])
      out-spy (reify IWriter
                (-write [this s] (swap! out-log conj s) this)
                (-flush [_] (swap! out-log conj :flush) nil))
      print-independence (binding [*out* out-spy
                                   *print-fn* (fn [s] (swap! print-log conj s))
                                   *print-newline* true]
                           (print "p1")
                           (println "p2")
                           (pr "p3")
                           (prn "p4")
                           (let [captured (with-out-str
                                            (print "c1")
                                            (println "c2")
                                            (pr "c3")
                                            (prn "c4"))]
                             [@print-log captured @out-log (identical? *out* out-spy)]))
      restored-after-print (nil? *out*)]
  [root-nils
   var-checks
   sb-write
   restored-after-sb
   thrown-restored
   reify-res
   restored-after-reify
   set-in-binding
   restored-after-set
   nested
   restored-after-nested
   print-independence
   restored-after-print])"#;

const EXPECTED_CONTRACT: &str = r#"[[true true true] [true true true true true true] [true "xy"] true ["boom" "before" true] [true [[:write "hello"] [:flush]]] true ["a" "b" true] true ["13" "2"] true [["p1" "p2" "\n" "\"p3\"" "\"p4\"" "\n"] "c1c2\n\"c3\"\"c4\"\n" [] true] true]"#;

fn analyze_program(source: &str) -> Program {
    analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn run_compiled(program: &Program, level: OptimizationLevel) -> String {
    let js = prepare_application(program, ApplicationOptions::new(level)).emit_js();
    execute(&format!("console.log({js});")).trim().to_owned()
}

fn light_cljs(dir: &Path, args: &[&str], input: &str) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_light-cljs"))
        .current_dir(dir)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn stdout(output: Output) -> String {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).unwrap()
}

#[test]
fn out_root_binding_reify_set_and_var_across_optimization_levels() {
    let source = format!("(pr-str {OUT_CONTRACT_BODY})");
    let program = analyze_program(&source);

    for level in OptimizationLevel::ALL {
        assert_eq!(
            run_compiled(&program, level),
            EXPECTED_CONTRACT,
            "optimization level {level}"
        );
    }

    // At `advanced`, `*out*` lives in its own `out` unit rather than inside
    // `coreCells`, so a program that binds or sets `*out*` uses an output cell
    // `core$out` initialized from `rt$out` with no `core` hub, and cross-function
    // reads observe the dynamic binding.
    let advanced_readable =
        prepare_application(&program, readable(OptimizationLevel::Advanced)).emit_js();
    assert!(
        advanced_readable.contains("let core$out = rt$out;"),
        "{advanced_readable}"
    );
    assert!(
        !advanced_readable.contains("coreCells.out"),
        "{advanced_readable}"
    );

    let report = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .retention();
    assert!(report.units.iter().all(|unit| unit.name != CORE_HUB));
    assert!(report.units.iter().any(|unit| unit.name == "out"));

    // Cross-function reads of `*out*` see `binding` and `set!` at every level.
    let cross_fn = r#"(defn write-out! [s] (-write *out* s))
(let [sb #js {:text "" :append (fn [s] (this-as this (set! (.-text this) (str (.-text this) s)) this))}]
  (pr-str [(binding [*out* (StringBufferWriter. sb)]
             (write-out! "hi")
             (.-text sb))
           *out*]))"#;
    let cross_program = analyze_program(cross_fn);
    for level in OptimizationLevel::ALL {
        assert_eq!(
            run_compiled(&cross_program, level),
            r#"["hi" nil]"#,
            "cross-fn at {level}"
        );
    }

    // A program that binds and reads `*out*` without printing does not pull in
    // `coreCells` or the printer (`printWriterImpl`).
    let lean_source = "(binding [*out* (js-obj)] (set! *out* nil) *out*)";
    let lean = analyze_program(lean_source);
    let lean_report =
        prepare_application(&lean, ApplicationOptions::new(OptimizationLevel::Advanced))
            .retention();
    assert!(
        lean_report
            .units
            .iter()
            .all(|unit| unit.name != "printWriterImpl" && unit.name != "coreCells"),
        "binding/reading *out* alone should not retain printWriterImpl or coreCells"
    );

    // A printing program that never names `*out*` must not retain `out` or grow
    // its `coreCells` unit at `advanced`.
    let prn_program = analyze_program("(prn 1)");
    let prn_prepared = prepare_application(
        &prn_program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    let prn_report = prn_prepared.retention();
    assert!(
        prn_report.units.iter().any(|unit| unit.name == "coreCells"),
        "(prn 1) is expected to retain coreCells"
    );
    assert!(
        prn_report.units.iter().all(|unit| unit.name != "out"),
        "(prn 1) must not retain the `out` unit"
    );
    let prn_readable =
        prepare_application(&prn_program, readable(OptimizationLevel::Advanced)).emit_js();
    assert!(
        !prn_readable.contains("out: null")
            && !prn_readable.contains("rt$out")
            && !prn_readable.contains("core$out"),
        "(prn 1) output must not include any `*out*` cell or binding"
    );
    assert!(
        prn_prepared.emit_js().len() <= 5828,
        "(prn 1) at advanced grew past master's 5828 bytes: {}",
        prn_prepared.emit_js().len()
    );
}

#[test]
fn out_works_in_macro_computations() {
    let source = format!(
        r#"
(defn out-contract []
  {OUT_CONTRACT_BODY})
(defmacro macro-out-contract []
  (list 'quote
        [(out-contract)
         (contains? (ns-publics 'cljs.core) '*out*)
         (contains? (ns-interns 'cljs.core) '*out*)]))
[(out-contract) (macro-out-contract)]
"#
    );
    let expected = format!("[{EXPECTED_CONTRACT} [{EXPECTED_CONTRACT} true true]]");

    // Fresh and reused macro hosts must both observe a clean nil root for *out*.
    let mut host = MacroHost::new();
    for pass in 0..2 {
        let program = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_or_else(|error| panic!("pass {pass}: {error}"));
        #[cfg(light_v8)]
        {
            let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
            assert_eq!(
                evaluator
                    .print(evaluator.evaluate(&program).unwrap())
                    .unwrap(),
                expected,
                "V8 evaluator pass {pass}"
            );
        }
        let script = format!(
            "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
            emit_js_unit(&program)
        );
        assert_eq!(execute(&script).trim(), expected, "Node pass {pass}");
    }
}

#[test]
fn out_works_in_reloadable_dev_build() {
    let dir = TempDir::new("out-var-dev");
    let src = dir.join("src");
    fs::create_dir_all(src.join("app")).unwrap();
    let main_source =
        format!("(ns app.main)\n(enable-console-print!)\n(println (pr-str {OUT_CONTRACT_BODY}))\n");
    fs::write(src.join("app/main.cljs"), &main_source).unwrap();

    let (program, sources) = analyze_namespace_entry_with_sources(
        "app.main",
        std::slice::from_ref(&src),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let [none, dev] = none_and_dev(&program, &sources, &dir.join("out"), false);
    let none = none.unwrap();
    let dev = dev.unwrap();
    assert_eq!(dev, none);
    let expected_json = EXPECTED_CONTRACT.replace('\\', "\\\\").replace('"', "\\\"");
    assert_eq!(
        none,
        format!(
            r#"{{"id":"program","outcome":{{"status":"returned","value":null}},"trace":[],"stdout":"{expected_json}\n"}}"#
        )
    );
}

#[test]
fn out_works_in_cli_compile_eval_and_repl() {
    let dir = TempDir::new("out-var-cli");
    let src = dir.join("src");
    fs::create_dir_all(src.join("app")).unwrap();
    let main_file = src.join("app/main.cljs");
    fs::write(
        &main_file,
        format!("(ns app.main)\n(enable-console-print!)\n(println (pr-str {OUT_CONTRACT_BODY}))\n"),
    )
    .unwrap();

    // `--compile` at every `--optimize` level.
    for level in ["none", "prepare", "advanced"] {
        let js = stdout(light_cljs(
            dir.path(),
            &[
                "--compile",
                "--no-cache",
                "--optimize",
                level,
                "--entry",
                "app.main",
                "--source-root",
                src.to_str().unwrap(),
            ],
            "",
        ));
        assert_eq!(
            execute(&js).trim(),
            EXPECTED_CONTRACT,
            "CLI --compile --optimize {level}"
        );
    }

    // `--eval`
    let eval_out = stdout(light_cljs(
        dir.path(),
        &["--eval", "--text", OUT_CONTRACT_BODY],
        "",
    ));
    assert_eq!(eval_out.trim(), EXPECTED_CONTRACT);

    // `--repl` across multiple forms, including top-level `set!` and restore.
    let repl_input = r#"*out*
(let [sb #js {:text "" :append (fn [s] (this-as this (set! (.-text this) (str (.-text this) s)) this))}] [(binding [*out* (StringBufferWriter. sb)] (-write *out* "x") (.-text sb)) *out*])
(let [log (atom []) w (reify IWriter (-write [this s] (swap! log conj s) this) (-flush [_] (swap! log conj :flush) nil))] (binding [*out* w] (-write *out* "a") (-flush *out*) (set! *out* nil) [*out* @log]))
[(var? #'*out*) (:dynamic (meta #'*out*)) @#'*out*]
(set! *out* :custom)
[*out* @#'*out* (with-out-str (print "ok"))]
(set! *out* nil)
*out*
"#;
    let repl_out = stdout(light_cljs(dir.path(), &["--repl"], repl_input));
    assert_eq!(
        repl_out,
        "nil\n[\"x\" nil]\n[nil [\"a\" :flush]]\n[true true nil]\n:custom\n[:custom :custom \"ok\"]\nnil\nnil\n"
    );
}
