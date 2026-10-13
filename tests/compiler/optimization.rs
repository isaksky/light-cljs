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
/// Run `source` at every optimization level in Node, printing its value as
/// `JSON.stringify` gives it, and require every level to print what `None`
/// (the oracle) prints; returns that output. The protocol, record, reify,
/// type and extension suites run their programs through this, so rapid type
/// analysis (light-cljs-211) and every other `Advanced` pass see them.
pub(crate) fn json_at_every_level(source: &str) -> String {
    let program = analyze(
        source,
        &parse(source).unwrap_or_else(|error| panic!("{source}: {error}")),
    )
    .unwrap_or_else(|error| panic!("{source}: {error}"));
    let outputs = OptimizationLevel::ALL.map(|level| {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        execute(&format!("console.log(JSON.stringify({js}));"))
    });
    for (level, output) in OptimizationLevel::ALL.iter().zip(&outputs).skip(1) {
        assert_eq!(output, &outputs[0], "at {level}: {source}");
    }
    outputs[0].clone()
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
            defines: Vec::new(),
            no_inlining: false,
            no_folding: false,
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
/// A temporary source root, removed on drop. Its path has the same length for
/// every root of one test process, since output quotes it in source metadata
/// that size comparisons (generated_optimization) measure.
pub(crate) struct Sources(pub(crate) PathBuf);
impl Sources {
    pub(crate) fn new() -> Self {
        let path = std::env::temp_dir().join(format!(
            "light-cljs-optimization-{}-{:06}",
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

/// Print the value of the expression `js`, or the message it throws, and then
/// the JSON of `globalThis.trace`, which starts empty.
fn traced(js: &str) -> String {
    execute(&format!(
        "globalThis.trace = [];\ntry {{ console.log({js}); }} catch (error) {{ console.log('threw', error.message); }}\nconsole.log(JSON.stringify(trace));"
    ))
}

/// Phase 2 step 6 of docs/advanced-optimization.md: at `Advanced`, unread
/// definitions whose initializers only call pure core operations on inert
/// arguments, bind locals, branch, add metadata or construct program types are
/// removed. `Prepare` keeps them, and its output is unchanged.
#[test]
fn advanced_removes_unused_definitions_with_pure_initializers() {
    let source = r#"
      (defrecord Point [x y])
      (deftype Box [v])
      (def label (str "PURE_STR" (+ 1 2)))
      (def pair (vector "PURE_VECTOR" (inc 1)))
      (def table (hash-map :a "PURE_HASH_MAP" :b (* 2 3)))
      (def small (array-map :a "PURE_ARRAY_MAP"))
      (def size (count ["PURE_COUNT" (- 5 1)]))
      (def nested {:a (str "PURE_NESTED") :b [(inc 1)]})
      (def bound (let [n (+ 1 2) s (str "PURE_LET" n)] (if (pos? n) s "no")))
      (def tagged (with-meta ["PURE_META"] {:tag (inc 1)}))
      (def origin (->Point "PURE_FACTORY" 0))
      (def boxed (Box. "PURE_NEW"))
      (def looked-up (get {:k "PURE_GET"} :k))
      (def named (do (str "PURE_DO") (keyword "PURE_KEYWORD")))
      (pr-str [(:x (->Point 1 2)) (.-v (Box. 3))])"#;
    let p = program(source);
    let sentinels = |js: &str| js.matches("\"PURE_").count();
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(&p, ApplicationOptions::new(level));
        let js = prepared.emit_js();
        let removed = if level.advanced() { 12 } else { 0 };
        assert_eq!(prepared.stats().removed_definitions, removed, "{level}");
        assert_eq!(
            sentinels(&js),
            if level.advanced() { 0 } else { 13 },
            "{level}"
        );
        assert_eq!(result(&js), "[1 3]\n", "{level}");
    }
    assert_eq!(
        emit_js(&p),
        prepare_application(&p, ApplicationOptions::default()).emit_js()
    );
}

/// The removal above never drops a throw, an effect, code that arguments
/// supply, or a call of an operation the program replaces: these keep their
/// sentinel at `Advanced` and print the same value and trace at every level.
#[test]
fn advanced_keeps_initializers_that_may_throw_run_code_or_see_replaced_operations() {
    for (source, expected) in [
        // `throws` rows stay although this call returns.
        (r#"(def unused (into {} [["KEPT_INTO" 1]])) 42"#, "42\n[]\n"),
        // A `hash-map` call without a value for its last key throws.
        (
            r#"(def unused (hash-map "KEPT_ODD" 1 :b)) 42"#,
            "threw No value supplied for key: :b\n[]\n",
        ),
        // `invoke` rows run their function arguments.
        (
            r#"(def unused (mapv (fn [x] (.push js/trace x) x) ["KEPT_MAPV"])) 42"#,
            "42\n[\"KEPT_MAPV\"]\n",
        ),
        // Outside its domain an operation may do anything.
        (r#"(def unused (inc "KEPT_DOMAIN")) 42"#, "42\n[]\n"),
        // `set!` replaces a core operation everywhere.
        (
            r#"(set! cljs.core/vector (fn [& xs] (.push js/trace "replaced") xs))
            (def unused (vector "KEPT_SET")) 42"#,
            "42\n[\"replaced\"]\n",
        ),
        // A core protocol extended to a runtime type reaches program code.
        (
            r#"(extend-type PersistentVector ICounted
                (-count [v] (.push js/trace "count") 99))
            (def unused (count ["KEPT_EXTENSION"])) 42"#,
            "42\n[\"count\"]\n",
        ),
        // A replaced type or positional factory is no longer a constructor.
        (
            r#"(deftype T [x]) (set! T (fn [x] (.push js/trace x)))
            (def unused (T. "KEPT_TYPE")) 42"#,
            "42\n[\"KEPT_TYPE\"]\n",
        ),
        (
            r#"(defrecord R [x]) (set! ->R (fn [x] (.push js/trace x)))
            (def unused (->R "KEPT_FACTORY")) 42"#,
            "42\n[\"KEPT_FACTORY\"]\n",
        ),
    ] {
        let p = program(source);
        for level in OptimizationLevel::ALL {
            let prepared = prepare_application(&p, ApplicationOptions::new(level));
            let js = prepared.emit_js();
            assert!(js.contains("KEPT_"), "{level}: {source}");
            assert_eq!(traced(&js), expected, "{level}: {source}");
        }
    }
}

/// Phase 2 step 7 of docs/advanced-optimization.md: at `Advanced`, retained raw
/// JavaScript no longer roots every top-level form or disables specialization
/// (see [`raw_js_names_resolve_only_to_holes_and_host_globals`]). Its holes are
/// ordinary children, so a definition a hole reads is kept, and a global in a
/// hole, which raw text could assign, is never specialized.
#[test]
fn advanced_treats_raw_javascript_as_an_opaque_leaf() {
    // `f` throws for other arguments, which keeps it from being inlined
    // (light-cljs-214), so its call is specialized.
    let p = program(
        r#"(def unused "RAW_UNUSED_SENTINEL")
        (js* "void 0")
        (defn f ([x] x) ([x y] (if (number? x) (+ x y) (throw (js/Error. "number")))))
        (f 21 21)"#,
    );
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(&p, ApplicationOptions::new(level));
        let js = prepared.emit_js();
        let advanced = usize::from(level.advanced());
        assert_eq!(prepared.stats().removed_definitions, advanced, "{level}");
        assert_eq!(prepared.stats().specialized_calls, advanced, "{level}");
        assert_eq!(js.contains("RAW_UNUSED_SENTINEL"), !level.advanced());
        assert_eq!(result(&js), "42\n", "{level}");
    }
    let p = program(
        r#"(def kept "RAW_HOLE_SENTINEL")
        (js* "trace.push(~{})" kept)
        (defn f ([x] x) ([x y] (+ x y)))
        (js* "~{} = function () { return 0; }" f)
        (f 21 21)"#,
    );
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(&p, ApplicationOptions::new(level));
        assert_eq!(prepared.stats().removed_definitions, 0, "{level}");
        assert_eq!(prepared.stats().specialized_calls, 0, "{level}");
        assert_eq!(
            traced(&prepared.emit_js()),
            "0\n[\"RAW_HOLE_SENTINEL\"]\n",
            "{level}"
        );
    }
}

/// Raw `js*` text in `context`: report to the harness of
/// [`raw_js_names_resolve_only_to_holes_and_host_globals`], with a direct eval
/// that resolves names in the raw text's own scope.
fn probe(context: &str) -> String {
    format!("(js* \"__probe('{context}', (__probeCode) => eval(__probeCode))\")")
}

/// Advanced mode treats `js*` as an opaque leaf rather than an escape to every
/// definition, because the hygiene pass renames every identifier the compiler
/// emits away from raw text (docs/advanced-optimization.md, Phase 2 step 7).
/// This checks that claim from inside raw text: in every scope where the emitter
/// binds names (functions, arities, the method entries of local functions with
/// hoisted siblings, loops, closures, `try`, `case`, `binding`,
/// `with-out-str`, type and `reify` methods, `letfn`, `this-as`, initializers and
/// the top level), each identifier token of the whole output, with the hygiene
/// prefix removed, must resolve exactly as it does at global scope. A new
/// emitter construct that binds names should get a probe here.
#[test]
fn raw_js_names_resolve_only_to_holes_and_host_globals() {
    let contexts = [
        "deftype-method",
        "record-method",
        "fn",
        "multi-arity-1",
        "multi-arity-2",
        "variadic",
        "local-multi-arity",
        "loop",
        "loop-closure",
        "catch",
        "finally",
        "keyword-case",
        "number-case",
        "string-case",
        "binding",
        "with-redefs",
        "with-out-str",
        "reify",
        "letfn",
        "this-as",
        "let",
        "initializer",
        "top-level",
    ];
    let p = |context: &str| {
        assert!(contexts.contains(&context), "{context}");
        probe(context)
    };
    let source = format!(
        r#"
        (deftype T [x] Object (toString [this] {} "t"))
        (defprotocol P (method [this a]))
        (defrecord R [x] P (method [this a] {} (+ x a)))
        (def ^:dynamic *d* 1)
        (defn fixed [a b] {} (+ a b))
        (defn multi ([a] {} a) ([a b] {} (+ a b)))
        (defn variadic [a & more] {} (apply + a more))
        (defn local-arities [n]
          (let [pure (fn ([a] (* a 2)) ([a b] (* a b)))
                scoped (fn ([a] {} (+ a n)) ([a b] (+ (pure a b) n)))]
            [(scoped 1) (scoped 1 2) (pure 3)]))
        (defn looping [n]
          (loop [i 0 fs []]
            (if (< i n)
              (recur (inc i) (conj fs (fn [] {} i)))
              (do {} (mapv #(%) fs)))))
        (defn caught [] (try (throw (js/Error. "x")) (catch :default e {} (.-message e)) (finally {})))
        (defn cases [k n s]
          [(case k :a (do {} 1)) (case n 1 (do {} 2)) (case s "s" (do {} 3) 4)])
        (defn bound [] (binding [*d* 2] {} *d*))
        (defn redefined [] (with-redefs [fixed (fn [a b] 0) inc dec] {} (+ (fixed 1 2) (inc 1))))
        (defn printed [] (with-out-str {} (print "x")))
        (defn reified [y] (str (reify Object (toString [_] {} (str y)))))
        (defn letfns [] (letfn [(a [] {} 1) (b [] (a))] (b)))
        (defn this-as-fn [] (this-as t {} 1))
        (defn let-fn [] (let [x 1 y (inc x)] {} (+ x y)))
        (def initialized (let [v (js/Math.max 1 2)] {} v))
        (defn table [] [{}])
        (defn constants [] [(count (table)) (:a {{:a [1 2]}}) '(x y)])
        {}
        (pr-str [(constants) (str (T. 1)) (method (->R 1) 2) (fixed 1 2) (multi 1) (multi 1 2)
                 (variadic 1 2 3) (local-arities 1) (looping 2) (caught) (cases :a 1 "s") (bound)
                 (redefined) (printed) (reified 3) (letfns) (this-as-fn) (let-fn)
                 initialized])"#,
        p("deftype-method"),
        p("record-method"),
        p("fn"),
        p("multi-arity-1"),
        p("multi-arity-2"),
        p("variadic"),
        p("local-multi-arity"),
        p("loop-closure"),
        p("loop"),
        p("catch"),
        p("finally"),
        p("keyword-case"),
        p("number-case"),
        p("string-case"),
        p("binding"),
        p("with-redefs"),
        p("with-out-str"),
        p("reify"),
        p("letfn"),
        p("this-as"),
        p("let"),
        p("initializer"),
        // A constant large enough to be a lazy cell (`c<n>`), which
        // `(constants)` fills before the other probes run.
        (0..1024)
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join(" "),
        p("top-level"),
    );
    let program = program(&source);
    let ran = {
        let mut contexts = contexts.to_vec();
        contexts.sort_unstable();
        format!("[\"{}\"]", contexts.join("\",\""))
    };
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        // The names to try are the identifier tokens of the program's text.
        // `arguments` and `this` are the raw text's own function context, and
        // `__probeCode` is the probe's parameter. The harness's names are global.
        let harness = format!(
            r#"const program = () => (
{js}
);
const names = [...new Set(program.toString().match(/[A-Za-z_$][\w$]*/g)
  .map(name => name.replace(/^__light\d+_/, "")))]
  .filter(name => !["arguments", "this", "__probeCode"].includes(name));
const kind = (resolve) => {{ try {{ return resolve(); }} catch (error) {{ return error.name; }} }};
const ran = new Set(), leaks = [];
globalThis.__probe = (context, evalHere) => {{
  ran.add(context);
  for (const name of names) {{
    const local = kind(() => evalHere("typeof " + name));
    if (local !== kind(() => (0, eval)("typeof " + name))) leaks.push(context + ": " + name + " is " + local);
  }}
}};
const value = program();
console.log(JSON.stringify(leaks));
console.log(JSON.stringify([...ran].sort()));
console.log(value);"#
        );
        let output = execute(&harness);
        let lines: Vec<_> = output.lines().collect();
        assert_eq!(
            lines[0], "[]",
            "raw text resolves compiler names at {level}"
        );
        assert_eq!(lines[1], ran, "probes that ran at {level}");
        assert_eq!(
            lines[2],
            r#"[[1024 [1 2] (x y)] "t" 3 3 1 3 6 [2 3 6] [0 1] "x" [1 2 3] 2 0 "x" "3" 1 1 3 2]"#,
            "{level}"
        );
    }
}
