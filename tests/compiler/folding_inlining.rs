//! Constant folding, budgeted inlining and build-time defines at `Advanced`
//! (light-cljs-214; docs/advanced-optimization.md, Phase 2 step 8 and "Phase 2
//! folding and inlining extension points").
//!
//! Every program runs at every level and must print what `None` prints, in a
//! sloppy script and in strict code, with `globalThis.trace` for effects. The
//! `Advanced` program text is checked for what folded, what was inlined and
//! what stayed: calls outside an operation's domain, operations the program
//! may replace, bodies that depend on their function's scope, calls that may
//! run before the definition and arguments whose evaluation must not move.
use super::optimization::{execute, readable};
use super::size_ratchet::ratchet_fixtures::FIXTURES;
use light_cljs::{
    ApplicationOptions, ApplicationStats, Define, DefineError, DefineValue, OptimizationLevel,
    Program, analyze, parse, prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn options(level: OptimizationLevel, defines: &[Define]) -> ApplicationOptions {
    let mut options = ApplicationOptions::new(level);
    options.defines = defines.to_vec();
    options
}

/// The output at `level` as a script that logs its value, in strict mode when
/// `strict`.
fn script(program: &Program, options: ApplicationOptions, strict: bool) -> String {
    let js = prepare_application(program, options).emit_js();
    let prologue = if strict { "\"use strict\";\n" } else { "" };
    format!("{prologue}globalThis.trace = [];\nconsole.log({js});")
}

/// The program prints `expected` at every level with `defines`, sloppy and
/// strict; returns the program part of the readable `Advanced` output and its
/// statistics.
fn defined_at_every_level(
    source: &str,
    defines: &[Define],
    expected: &str,
) -> (String, ApplicationStats) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        for strict in [false, true] {
            assert_eq!(
                execute(&script(&program, options(level, defines), strict)),
                format!("{expected}\n"),
                "at {level}, strict {strict}: {source}"
            );
        }
    }
    let mut advanced = options(OptimizationLevel::Advanced, defines);
    advanced.pseudo_names = true;
    let prepared = prepare_application(&program, advanced);
    assert!(prepared.rejected_defines().is_empty(), "{source}");
    (
        program_text(&prepared.emit_js()).to_owned(),
        prepared.stats().clone(),
    )
}

fn every_level(source: &str, expected: &str) -> (String, ApplicationStats) {
    defined_at_every_level(source, &[], expected)
}

/// The program part of `Advanced` output: what follows the runtime slice.
fn program_text(js: &str) -> &str {
    match js.find("  const [") {
        Some(start) => {
            let end = start + js[start..].find("\n})();\n").expect("the slice ends");
            &js[end..]
        }
        None => js,
    }
}

fn define(name: &str, value: DefineValue) -> Define {
    Define {
        name: name.into(),
        value,
    }
}

/// Arithmetic, comparisons, `=`, predicates and `str` on constants are their
/// runtime results, with JavaScript's doubles: `NaN`, `-0`, infinities and
/// the runtime's `mod`, `quot` and `rem`, and numbers printed by `str` as
/// JavaScript prints them.
#[test]
fn constant_core_calls_fold_to_the_runtime_results() {
    let source = r#"
        (pr-str [(+) (*) (+ 0.1 0.2) (- 5) (/ 4) (/ 1 0) (/ 0 0) (/ 1 (- 0.0)) (* -1 0)
                 (inc 1) (dec -1) (abs -0.0) (max 1 ##NaN 3) (min 2 1 3) (max 4)
                 (mod -7 3) (mod 7 -3) (rem -7 3) (quot -7 2) (quot 7.5 2) (mod 5 0)
                 (< 1) (< 1 2 3) (< 1 3 2) (>= 3 3 1) (== 1 1.0) (== ##NaN ##NaN)
                 (= 1 1.0) (= ##NaN ##NaN) (= 0 -0.0) (= "a" "a") (= nil false) (= nil nil)
                 (= :a :a) (= :a :ns/a) (not= 1 2) (identical? "a" "a")
                 (not nil) (not 0) (boolean "") (nil? false) (some? false) (true? 1)
                 (false? false) (zero? -0.0) (pos? 0) (neg? -1) (number? ##NaN)
                 (string? "") (keyword? :k) (boolean? nil)
                 (str) (str nil) (str "a" nil 1.5 true 1e21 1e-7 -0.0 0.1 100 ##Inf)])"#;
    let (text, stats) = every_level(
        source,
        r#"[0 1 0.30000000000000004 -5 0.25 ##Inf ##NaN ##-Inf -0.0 2 -2 0 ##NaN 1 4 2 -2 -1 -3 3 ##NaN true true false true true false true false true true false true true false true true true false true false true false true true false true true true true false "" "" "a1.5true1e+211e-700.1100Infinity"]"#,
    );
    assert!(stats.folded_expressions > 50, "{stats:?}");
    // Only the vector and its printing are left.
    for call in [
        "rt$add", "rt$max", "rt$mod", "rt$less", "rt$equal", "rt$str(",
    ] {
        assert!(!text.contains(call), "{call}: {text}");
    }
    assert!(
        text.contains("\"a1.5true1e+211e-700.1100Infinity\""),
        "{text}"
    );
    // Leading number literals fold, as the operations fold from the left.
    let (text, _) = every_level(
        r#"(pr-str [(+ 1 2 (.-length "ab")) (- 10 1 (.-length "ab") 1) (* 2 3 (.-length "a"))
                    (+ 1 (.-length "ab") 2)])"#,
        "[5 6 6 5]",
    );
    for folded in [
        "(3 + (\"ab\")[\"length\"])",
        "(9 - (\"ab\")[\"length\"] - 1)",
        "(6 * (\"a\")[\"length\"])",
        "(1 + (\"ab\")[\"length\"] + 2)",
    ] {
        assert!(text.contains(folded), "{folded}: {text}");
    }
}

/// A call outside the operation's domain, of an operation the program may
/// replace, or of `=` when the program extends a core protocol to a host type
/// runs as before.
#[test]
fn calls_outside_the_domain_or_of_replaceable_operations_stay() {
    let (text, _) = every_level(
        r#"(pr-str [(+ 1 "a") (inc nil) (str :k)])"#,
        r#"["1a" 1 ":k"]"#,
    );
    assert!(text.contains("\"a\""), "{text}");
    assert!(text.contains("rt$str("), "{text}");
    let (text, _) = every_level(
        "(set! cljs.core/inc (fn [x] (- x 1))) (pr-str [(inc 1) (+ 1 2)])",
        "[0 3]",
    );
    assert!(text.contains("core$inc"), "{text}");
    let (text, _) = every_level(
        "(extend-type number IEquiv (-equiv [a b] true)) (pr-str [(= 1 2) (< 1 2)])",
        "[true true]",
    );
    assert!(text.contains("rt$equal(1, 2)"), "{text}");
}

/// An `if` on a constant is the branch it selects, through `when`, `cond`,
/// `if-not`, `and` and `or`, and the definitions only the other branch read
/// are removed. `0` and `""` are true.
#[test]
fn constant_tests_select_their_branch_and_drop_what_only_the_other_reads() {
    let source = r#"
        (defn helper [] (.push js/trace "HELPER_SENTINEL") :helper)
        (def table {:a (helper)})
        (when false (helper) (js/console.log "DEAD_SENTINEL"))
        (if nil (helper) :ok)
        (pr-str [(if 0 :zero :falsy) (if "" :empty :falsy) (if-not true :no :yes)
                 (cond false 1 nil 2 :else 3) (and 1 :x) (and 1 nil 2) (or false nil :y)
                 (or false nil) (when (and true (or nil :z)) :w) (vec js/trace)])"#;
    let (text, stats) = every_level(
        source,
        r#"[:zero :empty :yes 3 :x nil :y nil :w ["HELPER_SENTINEL"]]"#,
    );
    assert!(!text.contains("DEAD_SENTINEL"), "{text}");
    assert!(!text.contains(" ? "), "{text}");
    assert!(
        !text
            .lines()
            .any(|line| line.trim_start().starts_with("if (")),
        "{text}"
    );
    assert!(text.matches("HELPER_SENTINEL").count() == 1, "{text}");
    assert!(stats.folded_expressions >= 10, "{stats:?}");
    // Collection, `fn` and quoted symbol literals are true, also through the
    // local an `and` or `or` binds them to.
    let (text, _) = every_level(
        "(pr-str [(if [1] :v :x) (and [1] :y) (or {:a 1} :z) (if (fn [] 1) :f :g) (if 'q :s :t)])",
        "[:v :y {:a 1} :f :s]",
    );
    assert!(!text.contains(" ? "), "{text}");
    // Without `table`, `helper` is only read by dead branches and goes.
    let (text, stats) = every_level(
        r#"(defn helper [] (.push js/trace "HELPER_SENTINEL") :helper)
           (when false (helper)) (pr-str (if nil (helper) :ok))"#,
        ":ok",
    );
    assert!(!text.contains("HELPER_SENTINEL"), "{text}");
    assert!(stats.removed_definitions >= 1, "{stats:?}");
}

/// A `let` binding of a constant is substituted and dropped, and so is an
/// unread binding without an effect; a binding with an effect stays.
#[test]
fn let_bound_constants_and_unread_bindings_fold() {
    let source = r#"
        (defn f [n]
          (let [k 2 unused [1 n] effect (.push js/trace n) s (str "x" k)]
            [(* n k) s]))
        (pr-str [(f 3) (vec js/trace)])"#;
    let (text, _) = every_level(source, r#"[[6 "x2"] [3]]"#);
    assert!(text.contains("\"x2\""), "{text}");
    assert!(!text.contains("rt$str"), "{text}");
}

/// A `let` or `do` in value position is emitted as a function call, so one
/// whose contents read `arguments` or hold raw text keeps its form when its
/// bindings fold away.
#[test]
fn function_scope_stays_where_arguments_or_raw_text_see_it() {
    let source = r#"
        (defn args-in-and [x] [(and true (aget (js-arguments) 0))])
        (defn args-in-let [x] [(let [k 1] (+ k (alength (js-arguments))))])
        (defn args-in-do [x] [(do (alength (js-arguments)))])
        (defn raw-in-let [x] [(let [k 1] (js* "arguments.length"))])
        (pr-str [(args-in-and 5) (args-in-let 5) (args-in-do 5) (raw-in-let 5)])"#;
    every_level(source, "[[nil] [1] [0] [0]]");
}

/// A read of a `^:const` global defined once by a top-level `def` is its
/// value in later top-level forms, where the definition has run; earlier
/// forms, including functions they define, read the global, which may still
/// be unset.
#[test]
fn const_reads_are_their_value_after_the_definition_runs() {
    let source = r#"
        (declare LATE)
        (defn early [] LATE)
        (def before (early))
        (def ^:const LATE 5)
        (def ^:const LABEL "label")
        (def ^:const KIND :kind)
        (def ^:const NOTHING nil)
        (def plain 7)
        (defn later [] [LATE LABEL KIND NOTHING plain])
        (pr-str [before (early) (later) (+ LATE 1)])"#;
    let (text, _) = every_level(source, r#"[nil 5 [5 "label" :kind nil 7] 6]"#);
    assert!(text.contains("\"label\""), "{text}");
    // `plain` is no constant and `early` reads `LATE` before it is defined.
    assert!(text.contains("[5, \"label\", k"), "{text}");
    // A constant defined outside the top level is read as a global.
    let (text, _) = every_level(
        "(when (some? js/globalThis) (def ^:const INNER 1)) (pr-str [INNER])",
        "[1]",
    );
    assert!(text.contains("rt$vectorFromArray([g"), "{text}");
}

/// `goog-define` defines a constant whose literal default a build replaces
/// at every level; `Advanced` substitutes the value at every later read.
#[test]
fn goog_define_takes_its_value_from_the_build_at_every_level() {
    let source = r#"
        (goog-define DEBUG false)
        (goog-define LEVEL 2)
        (goog-define NAME "app")
        (pr-str [DEBUG LEVEL NAME (when DEBUG :debugging) (* LEVEL 10) (str NAME "!")])"#;
    let (text, _) = every_level(source, r#"[false 2 "app" nil 20 "app!"]"#);
    assert!(text.contains("\"app!\""), "{text}");
    let defines = [
        define("cljs.user/DEBUG", DefineValue::Boolean(true)),
        define("cljs.user/LEVEL", DefineValue::Number(-0.5)),
        define(
            "cljs.user/NAME",
            DefineValue::String("other \u{1F600}".into()),
        ),
    ];
    let (text, _) = defined_at_every_level(
        source,
        &defines,
        "[true -0.5 \"other \u{1F600}\" :debugging -5 \"other \u{1F600}!\"]",
    );
    assert!(!text.contains("\"app"), "{text}");
    // The CLI reads each value as the kind of the default.
    let program = program(source);
    assert_eq!(
        Define::parse(&program, "cljs.user/LEVEL=1e3"),
        Ok(define("cljs.user/LEVEL", DefineValue::Number(1000.0)))
    );
    assert_eq!(
        Define::parse(&program, "cljs.user/NAME=a=b"),
        Ok(define("cljs.user/NAME", DefineValue::String("a=b".into())))
    );
    assert_eq!(
        Define::parse(&program, "cljs.user/DEBUG=true"),
        Ok(define("cljs.user/DEBUG", DefineValue::Boolean(true)))
    );
}

/// Defines that name no `goog-define` or have another kind are rejected and
/// change nothing; `goog-define` needs a literal default and stays constant.
#[test]
fn defines_and_goog_define_reject_what_does_not_fit() {
    let source = "(goog-define DEBUG false) (def ^:const plain true) (pr-str [DEBUG plain])";
    let program = program(source);
    for (text, error) in [
        ("DEBUG=true", DefineError::Syntax("DEBUG=true".into())),
        (
            "cljs.user/DEBUG",
            DefineError::Syntax("cljs.user/DEBUG".into()),
        ),
        (
            "cljs.user/plain=false",
            DefineError::Unknown("cljs.user/plain".into()),
        ),
        (
            "other/DEBUG=true",
            DefineError::Unknown("other/DEBUG".into()),
        ),
        (
            "cljs.user/DEBUG=1",
            DefineError::Kind {
                name: "cljs.user/DEBUG".into(),
                expected: "boolean",
                value: "1".into(),
            },
        ),
    ] {
        assert_eq!(Define::parse(&program, text), Err(error), "{text}");
    }
    for level in OptimizationLevel::ALL {
        let prepared = prepare_application(
            &program,
            options(
                level,
                &[
                    define("cljs.user/DEBUG", DefineValue::Number(1.0)),
                    define("cljs.user/plain", DefineValue::Boolean(false)),
                ],
            ),
        );
        assert_eq!(
            prepared.rejected_defines(),
            [
                DefineError::Kind {
                    name: "cljs.user/DEBUG".into(),
                    expected: "boolean",
                    value: "1".into(),
                },
                DefineError::Unknown("cljs.user/plain".into()),
            ]
        );
        assert_eq!(
            execute(&format!("console.log({})", prepared.emit_js())),
            "[false true]\n",
            "{level}"
        );
    }
    for (source, message) in [
        ("(goog-define X (inc 1))", "a string, number or boolean"),
        ("(goog-define X :k)", "a string, number or boolean"),
        ("(goog-define X)", "a name and a default value"),
        ("(goog-define X 1) (set! X 2)", "Can't set! a constant"),
        ("(goog-define X 1) (def X 2)", "Can't redefine a constant"),
    ] {
        let error = analyze(source, &parse(source).unwrap()).unwrap_err();
        assert!(error.to_string().contains(message), "{source}: {error}");
    }
}

/// The ratchet fixture `debug-logging`: with `DEBUG` false the logging path
/// vanishes completely at `Advanced`, so the output is exactly that of the
/// program written without it; with `DEBUG` true it logs at every level.
#[test]
fn debug_logging_vanishes_completely() {
    let fixture = FIXTURES
        .iter()
        .find(|fixture| fixture.name == "debug-logging")
        .expect("the debug-logging fixture")
        .source;
    let (text, stats) = every_level(fixture, "12");
    assert_eq!(
        text, "(() => {\n  return ((\"abc\")[\"length\"] * 4);\n})()\n",
        "{text}"
    );
    assert_eq!(stats.inlined_calls, 2, "{stats:?}");
    let release = r#"(defn area [w h] (* w h)) (area (.-length "abc") 4)"#;
    assert_eq!(
        text,
        prepare_application(&program(release), readable(OptimizationLevel::Advanced)).emit_js()
    );
    defined_at_every_level(
        fixture,
        &[define("cljs.user/DEBUG", DefineValue::Boolean(true))],
        "[debug] [\"startup\" {:debug true}]\n[debug] [\"area\" 3 4 {:w 3, :h 4}]\n12",
    );
}

/// A function whose only call is the one being inlined moves into it, with
/// its effects in order, and its definition goes.
#[test]
fn single_use_functions_move_into_their_only_call() {
    let source = r#"
        (defn step [x] (.push js/trace "step" x) (let [y (* x 2)] (.push js/trace y) y))
        (defn run [] (.push js/trace "run") (step 21))
        (run)
        (pr-str (vec js/trace))"#;
    let (text, stats) = every_level(source, r#"["run" "step" 21 42]"#);
    assert!(!text.contains("function"), "{text}");
    assert_eq!(stats.inlined_calls, 2, "{stats:?}");
    assert!(stats.removed_definitions >= 2, "{stats:?}");
    // `no_inlining` keeps every call, as Closure's `inlineFunctions` option
    // does; folding still runs.
    let mut options = readable(OptimizationLevel::Advanced);
    options.no_inlining = true;
    let prepared = prepare_application(&program(source), options);
    assert_eq!(prepared.stats().inlined_calls, 0);
    assert_eq!(
        program_text(&prepared.emit_js())
            .matches("= (function(")
            .count(),
        2
    );
}

/// A small pure function is inlined at every call when that makes the
/// program smaller; a larger one, or one with an effect, stays a function.
#[test]
fn small_pure_functions_inline_where_that_shrinks_the_program() {
    let source = r#"
        (defn sq [x] (* x x))
        (defn tagged [x] [:tag x (inc x) (dec x) (* 2 x) (str x)])
        (defn noisy [x] (.push js/trace x) x)
        (defn use-all [a] [(sq a) (sq 3) (tagged a) (tagged 2) (noisy a) (noisy 4)])
        (pr-str [(use-all (.-length "abcde")) (sq 4) (vec js/trace)])"#;
    let (text, stats) = every_level(
        source,
        r#"[[25 9 [:tag 5 6 4 10 "5"] [:tag 2 3 1 4 "2"] 5 4] 16 [5 4]]"#,
    );
    assert!(text.contains(" * l"), "{text}");
    assert!(stats.inlined_calls >= 3, "{stats:?}");
    // `use-all`, whose call needs its argument bound in value position,
    // `tagged` and `noisy` keep their definitions; `sq` goes.
    assert_eq!(text.matches("= (function(").count(), 3, "{text}");
}

/// Arguments that are not literals or locals are bound before the body, once
/// and in order, also for an unread parameter; in value position such a call
/// stays a call.
#[test]
fn inlined_arguments_run_once_and_in_order() {
    let source = r#"
        (defn t [x] (.push js/trace x) x)
        (defn three [a b c] [c b a b])
        (defn ignore [a b] b)
        (defn run []
          (three (t 1) (t 2) (t 3))
          (ignore (t 4) 5)
          (let [v (three (t 6) 7 8)] v))
        (pr-str [(run) (vec js/trace)])"#;
    let (text, _) = every_level(source, "[[8 7 6 7] [1 2 3 4 6]]");
    assert!(text.contains("const l"), "{text}");
    // One argument with an effect moves to its parameter's only read when
    // nothing before that read in the body can have an effect.
    let source = r#"
        (defn t [x] (.push js/trace x) x)
        (defn wrap [x] (str "<" x ">"))
        (defn later [x] (.push js/trace :later) x)
        (defn twice [x] [x x])
        (pr-str [(wrap (t 1)) (later (t 2)) (twice (t 3)) (wrap (t 4)) (vec js/trace)])"#;
    let (text, stats) = every_level(source, r#"["<1>" 2 [3 3] "<4>" [1 2 :later 3 4]]"#);
    assert!(text.contains("rt$str(\"<\", g"), "{text}");
    assert_eq!(stats.inlined_calls, 2, "{stats:?}");
}

/// Bodies that read their function's `arguments` or `this`, hold raw text,
/// `recur` to their method, loop, close over locals, throw or catch stay
/// functions, and so does a `^boolean` function, whose calls trust the tag.
#[test]
fn bodies_that_depend_on_their_function_stay_functions() {
    let source = r#"
        (defn args [x] (alength (js-arguments)))
        (defn self [x] (this-as t (some? t)))
        (defn raw [x] (js* "~{} + 1" x))
        (defn countdown [n] (if (pos? n) (recur (dec n)) :done))
        (defn looping [n] (loop [i 0] (if (< i n) (recur (inc i)) i)))
        (defn closure [x] (fn [] x))
        (defn fails [x] (throw (js/Error. (str x))))
        (defn ^boolean flag [x] (= x 0))
        (pr-str [(args 1) (self 1) (raw 1) (countdown 3) (looping 3) ((closure 4))
                 (try (fails "e") (catch :default e (.-message e))) (if (flag 0) :t :f)])"#;
    let (text, stats) = every_level(source, r#"[1 true 2 :done 3 4 "e" :t]"#);
    assert_eq!(stats.inlined_calls, 0, "{stats:?}");
    assert_eq!(text.matches("= (function(").count(), 8, "{text}");
}

/// A call that may run before its callee's definition (from a function
/// defined earlier, across a `declare`) stays a call, which throws as before.
#[test]
fn calls_that_may_run_before_the_definition_stay_calls() {
    let source = r#"
        (declare later)
        (defn early [] (later 1))
        (def caught (try (early) (catch :default e :not-yet)))
        (defn later [x] (inc x))
        (pr-str [caught (early) (later 2)])"#;
    let (text, _) = every_level(source, "[:not-yet 2 3]");
    assert!(text.contains("(l"), "{text}");
}

/// A mutable field passed as an argument is read when the call runs, not
/// where the body reads its parameter after a method has assigned it.
#[test]
fn mutable_fields_are_bound_before_the_body_runs() {
    let source = r#"
        (defprotocol P (step [this]) (peek-n [this]))
        (defn keep-value [c v] (step c) v)
        (deftype Counter [^:mutable n]
          P (step [_] (set! n (inc n))) (peek-n [this] (keep-value this n)))
        (let [c (Counter. 1)] (pr-str [(peek-n c) (peek-n c)]))"#;
    every_level(source, "[1 2]");
}

/// A variadic method is inlined only when its body does not read the rest
/// parameter; rest arguments with an effect still run.
#[test]
fn rest_parameters_inline_only_when_unread() {
    let source = r#"
        (defn t [x] (.push js/trace x) x)
        (defn ignore-rest [a & more] (* a 2))
        (defn count-rest [a & more] (+ a (count more)))
        (defn run [] (ignore-rest 1 (t :a) 2) (t :b))
        (pr-str [(ignore-rest 3 4 5) (count-rest 1 2 3) (run) (vec js/trace)])"#;
    let (text, _) = every_level(source, "[6 3 :b [:a :b]]");
    assert!(text.contains("rt$fnRest"), "{text}");
}

/// A literal the program builds at each evaluation stays built when folding
/// and inlining make its items constants, so it is never the shared pooled
/// constant, and map keys and set members stay as written, so equal keys
/// computed at run time are still found where they were.
#[test]
fn built_literals_stay_built_and_keys_stay_as_written() {
    let source = r#"
        (defn mk [x] [x 1])
        (defn pair [x y] {:x x :y y})
        (def ^:const K :k)
        (defn keyed [v] {K v})
        (pr-str [(identical? (mk 1) (mk 1)) (identical? (pair 1 2) (pair 1 2))
                 (let [x 1] (identical? [x] [x])) (identical? (keyed 1) (keyed 1))
                 (let [a 1 b 1] (try #{a b} (catch :default e :duplicate)))
                 (let [a :x b :x] (count {a 1 b 2}))
                 (mk 2) (pair 1 2) (keyed 3)])"#;
    let (text, stats) = every_level(
        source,
        "[false false false false :duplicate 1 [2 1] {:x 1, :y 2} {:k 3}]",
    );
    assert!(stats.inlined_calls >= 4, "{stats:?}");
    assert!(stats.folded_expressions >= 2, "{stats:?}");
    // The source constants are one pooled value; the built ones are not.
    assert!(text.contains("(k"), "{text}");
}

/// Recursive and mutually recursive functions inline to a fixed point within
/// the round limit and keep their results.
#[test]
fn recursion_reaches_a_fixed_point() {
    let source = r#"
        (declare odd-step)
        (defn even-step [n] (if (zero? n) true (odd-step (dec n))))
        (defn odd-step [n] (if (zero? n) false (even-step (dec n))))
        (defn fact [n] (if (zero? n) 1 (* n (fact (dec n)))))
        (defn once [n] (fact n))
        (pr-str [(even-step 10) (odd-step 7) (once 5)])"#;
    every_level(source, "[true true 120]");
}
