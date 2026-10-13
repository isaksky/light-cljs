//! Direct call shapes at `Advanced` (light-cljs-212; docs/advanced-optimization.md,
//! Phase 2 step 5 and "Phase 2 call shape extension points").
//!
//! Every program runs at every level and must print what `None` prints, both in
//! a sloppy script and in strict code, where `this` may differ. The `Advanced`
//! program text is checked for the shapes: calls of known functions name their
//! method (`g3_1(a)`) without `.call(null, ...)`, functions whose value never
//! escapes have no `arguments.length` dispatcher, and a `do` in value position
//! is a comma expression. Generic shapes must stay where the function is
//! redefined, assigned or escapes from a definition that may run twice.
use super::optimization::{Sources, execute};
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::{
    ApplicationOptions, ApplicationStats, GlobalId, MacroHost, OptimizationLevel, Program, analyze,
    parse, prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

/// The output of `program` at `level`, as a script that logs its value, in
/// strict mode when `strict`.
fn script(program: &Program, level: OptimizationLevel, strict: bool) -> String {
    let js = prepare_application(program, ApplicationOptions::new(level)).emit_js();
    let prologue = if strict { "\"use strict\";\n" } else { "" };
    format!("{prologue}globalThis.trace = [];\nconsole.log({js});")
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level, in a sloppy script and in strict code; returns the program part of
/// the `Advanced` output and its statistics.
fn every_level(program: &Program, source: &str, expected: &str) -> (String, ApplicationStats) {
    for level in OptimizationLevel::ALL {
        for strict in [false, true] {
            assert_eq!(
                execute(&script(program, level, strict)),
                format!("{expected}\n"),
                "at {level}, strict {strict}: {source}"
            );
        }
    }
    let prepared = prepare_application(
        program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    );
    (
        program_text(&prepared.emit_js()).to_owned(),
        prepared.stats().clone(),
    )
}

fn source_at_every_level(source: &str, expected: &str) -> (String, ApplicationStats) {
    every_level(&program(source), source, expected)
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

/// The output name of the global `name` (`g3`).
fn global(program: &Program, name: &str) -> String {
    let id = program
        .globals
        .iter()
        .position(|binding| binding.name == name)
        .unwrap_or_else(|| panic!("no global {name}"));
    format!("g{id}")
}

/// Known functions are called through the method their call selects, with no
/// `.call(null, ...)` and, when nothing else sees their value, no dispatcher:
/// one fixed method, fixed arities, variadic methods with and without rest
/// arguments, self and mutual recursion across a `declare`, calls from other
/// functions and from nested closures.
#[test]
fn known_functions_are_called_through_the_selected_method() {
    // The host calls keep `add2` and the first method of `multi` from being
    // inlined (light-cljs-214), which would replace the calls checked here.
    let source = r#"
        (defn add2 [a b] (js/Math.abs (+ a b)))
        (defn multi ([] (js/Math.abs 0)) ([x] (multi x 1)) ([x y] (add2 x y)) ([x y & more] (reduce + (+ x y) more)))
        (defn rest-only [& xs] (count xs))
        (declare odd-step)
        (defn even-step [n] (if (zero? n) true (odd-step (dec n))))
        (defn odd-step [n] (if (zero? n) false (even-step (dec n))))
        (defn fact [n] (if (zero? n) 1 (* n (fact (dec n)))))
        (defn user [] (let [f (fn [x] (multi x))] (f 41)))
        (pr-str [(add2 1 2) (multi) (multi 5) (multi 1 2) (multi 1 2 3 4) (rest-only) (rest-only 1 2 3)
                 (even-step 10) (odd-step 7) (fact 10) (user)])"#;
    let program = program(source);
    let (text, stats) = every_level(&program, source, "[3 0 6 3 10 0 3 true true 3628800 42]");
    let [add2, multi, rest_only, even_step, odd_step, fact] = [
        "add2",
        "multi",
        "rest-only",
        "even-step",
        "odd-step",
        "fact",
    ]
    .map(|name| global(&program, name));
    assert!(!text.contains("switch (arguments.length)"), "{text}");
    assert!(!text.contains(".call(null"), "{text}");
    for call in [
        format!("{add2}(1, 2)"),
        format!("{multi}_0()"),
        format!("{multi}_1(5)"),
        format!("{multi}_2(1, 2)"),
        format!("{multi}_3(1, 2, rt$fnRest([3, 4], 0))"),
        format!("{rest_only}_0(null)"),
        format!("{rest_only}_0(rt$fnRest([1, 2, 3], 0))"),
        format!("{even_step}(10)"),
        format!("{odd_step}(7)"),
        format!("{fact}(10)"),
        // Calls inside methods and nested functions name the method too.
        format!("{multi}_2(l"),
        format!("{multi}_1(l"),
    ] {
        assert!(text.contains(&call), "{call}: {text}");
    }
    // The entries are bound once, at the definition; the global is not.
    assert!(text.contains(&format!("let {multi}_3;")), "{text}");
    assert!(!text.contains(&format!("let {multi};")), "{text}");
    assert!(text.contains(&format!("({multi}_0 = ")), "{text}");
    assert_eq!(stats.dispatchers_removed, 2, "{stats:?}");
    assert!(stats.direct_calls >= 14, "{stats:?}");
}

/// `recur`, `arguments` and the parameters it aliases behave as through the
/// dispatcher: the same values reach the same method.
#[test]
fn methods_keep_recur_arguments_and_parameters() {
    let source = r#"
        (defn fib ([n] (fib n 0 1)) ([n a b] (if (zero? n) a (recur (dec n) b (+ a b)))))
        (defn sum [acc & xs] (if (seq xs) (recur (+ acc (first xs)) (next xs)) acc))
        (defn arity ([a] (alength (js-arguments))) ([a b] (alength (js-arguments)))
          ([a b & r] (alength (js-arguments))))
        (defn aliased ([n] (if (pos? n) (recur (dec n)) (aget (js-arguments) 0))) ([a b] b))
        (pr-str [(fib 30) (sum 0 1 2 3 4) (sum 5) (arity 1) (arity 1 2) (arity 1 2 3 4 5) (aliased 3)])"#;
    let (text, _) = source_at_every_level(source, "[832040 10 5 1 2 3 0]");
    assert!(!text.contains("switch (arguments.length)"), "{text}");
}

/// A plain call passes `undefined` as `this` where `.call(null, ...)` passed
/// `null`, which strict raw JavaScript can see, so a method that holds `js*`
/// keeps `.call(null, ...)` on its entry. `this-as` maps both to the global
/// object, so methods that only use it are called plainly.
#[test]
fn methods_that_may_read_this_keep_call_null() {
    let source = r#"
        (defn raw-multi ([] (js* "this === null || this === globalThis")) ([x] x))
        (defn raw-single [] (js* "this === null || this === globalThis"))
        (defn this-as-multi ([] (this-as t (identical? t js/globalThis))) ([x] x))
        (defn this-as-single [] (this-as t (identical? t js/globalThis)))
        (def raw-local (let [f (fn [] (js* "this === null || this === globalThis"))] (f)))
        (def this-as-local (let [f (fn [] (this-as t (identical? t js/globalThis)))] (f)))
        (pr-str [(raw-multi) (raw-single) (this-as-multi) (this-as-single) raw-local this-as-local])"#;
    let program = program(source);
    let (text, _) = every_level(&program, source, "[true true true true true true]");
    let [raw_multi, raw_single, this_as_multi, this_as_single] =
        ["raw-multi", "raw-single", "this-as-multi", "this-as-single"]
            .map(|name| global(&program, name));
    for call in [
        format!("{raw_multi}_0.call(null)"),
        format!("{raw_single}.call(null)"),
        format!("{this_as_multi}_0()"),
        format!("{this_as_single}()"),
    ] {
        assert!(text.contains(&call), "{call}: {text}");
    }
}

/// A function whose value escapes keeps its dispatcher and arity properties,
/// over the same method entries its direct calls name: `apply`, higher-order
/// arguments, `var`, the value of its `def`, and calls whose count selects no
/// method at compile time (an invalid arity, or fewer arguments than the
/// variadic method's fixed parameters).
#[test]
fn escaping_functions_keep_their_dispatcher_over_the_same_entries() {
    // The host calls keep the methods whose calls are checked here from being
    // inlined (light-cljs-214).
    let source = r#"
        (defn multi ([] 0) ([x] (js/Math.abs x)) ([x y & more] [x y more]))
        (defn mapped ([x] (inc x)) ([x y] (js/Math.abs (+ x y))))
        (defn held ([] :held) ([x] (js/Math.abs x)))
        (defn holes ([] 0) ([a b] 1))
        (defn under ([a] [(js/Math.abs a)]) ([a b c & r] [a b c r]))
        (def v (var held))
        (declare result)
        (def returned (defn result ([] :result) ([x] x)))
        (pr-str [(apply multi 1 2 [3 4]) (multi 5) (multi 1 2 3) (mapv mapped [1 2]) (mapped 1 2)
                 (v) (held 3) (returned) (result 4)
                 (try (holes 1) (catch :default e (.-message e))) (under 1 2) (under 1)
                 (.-cljs$lang$maxFixedArity multi)])"#;
    let program = program(source);
    let (text, stats) = every_level(
        &program,
        source,
        "[[1 2 (3 4)] 5 [1 2 (3)] [2 3] 3 :held 3 :result 4 \"Invalid arity: 1\" [1 2 nil nil] [1] 2]",
    );
    let [multi, mapped, held, holes, under] =
        ["multi", "mapped", "held", "holes", "under"].map(|name| global(&program, name));
    for (function, call) in [
        (&multi, format!("{multi}_1(5)")),
        (&multi, format!("{multi}_2(1, 2, rt$fnRest([3], 0))")),
        (&mapped, format!("{mapped}_1(1, 2)")),
        (&held, format!("{held}_1(3)")),
        (&under, format!("{under}_0(1)")),
    ] {
        assert!(text.contains(&call), "{call}: {text}");
        assert!(
            text.contains(&format!("{function}.cljs$core$IFn$_invoke$arity$")),
            "{function}: {text}"
        );
    }
    assert!(text.contains(&format!(", {holes} = function(")), "{text}");
    assert!(text.contains(&format!("({holes}).call(null, 1)")), "{text}");
    assert!(
        text.contains(&format!("({under}).call(null, 1, 2)")),
        "{text}"
    );
    assert_eq!(stats.dispatchers_removed, 0, "{stats:?}");
}

/// Redefinition, assignment and dynamic binding keep generic calls, which read
/// the binding at the time of the call, and so does a function whose value
/// escapes from a definition that may run more than once. A function that
/// does not escape may be defined anywhere: its entries are rebound together.
#[test]
fn redefined_assigned_and_rerun_definitions() {
    let source = r#"
        (defn redefined ([] 1) ([x] x))
        (def before (redefined))
        (defn redefined ([] 2) ([x] (* 2 x)))
        (defn assigned ([] 1) ([x] x))
        (defn reassign [] (set! assigned (fn ([] 3) ([x] (* 3 x)))))
        (defn ^:dynamic dynamic ([] 1) ([x] x))
        (defn redefs ([] 1) ([x] x))
        (defonce once (fn ([] 1) ([x] x)))
        (defn raw ([] 1) ([x] x))
        (js* "~{} = function () { return 7; }" raw)
        (declare inner)
        (defn make [n] (defn inner ([] n) ([x] (+ x n))) nil)
        (declare outer)
        (defn make-outer [n] (defn outer ([] n) ([x] (+ x n))))
        (def first-inner (do (make 1) [(inner) (inner 10)]))
        (def second-inner (do (make 2) [(inner) (inner 10)]))
        (def made (make-outer 1))
        (make-outer 2)
        (pr-str [before (redefined) (redefined 4) (assigned 2) (do (reassign) (assigned 2))
                 (binding [dynamic (fn ([] 5) ([x] x))] (dynamic)) (dynamic)
                 (with-redefs [redefs (fn ([] 6) ([x] x))] (redefs)) (redefs) (once) (raw)
                 first-inner second-inner (made) (outer) (made 10)])"#;
    let program = program(source);
    let (text, _) = every_level(
        &program,
        source,
        "[1 2 8 2 6 5 1 6 1 1 7 [1 11] [2 12] 1 2 11]",
    );
    for name in [
        "redefined",
        "assigned",
        "dynamic",
        "redefs",
        "once",
        "raw",
        "outer",
    ] {
        let name = global(&program, name);
        assert!(
            text.contains(&format!("{name}).call(null")),
            "{name}: {text}"
        );
        assert!(!text.contains(&format!("{name}_0")), "{name}: {text}");
    }
    let inner = global(&program, "inner");
    assert!(text.contains(&format!("{inner}_0()")), "{text}");
    assert!(text.contains(&format!("{inner}_0 = ")), "{text}");
}

/// A call that runs before the definition still throws a `TypeError`.
#[test]
fn calls_before_the_definition_still_throw_type_errors() {
    let source = r#"
        (declare later later-multi)
        (defn early [] [(try (later 1) (catch js/TypeError e :type-error))
                        (try (later-multi 1) (catch js/TypeError e :type-error))])
        (def result (early))
        (defn later [x] x)
        (defn later-multi ([] 0) ([x] x))
        (pr-str [result (early)])"#;
    source_at_every_level(source, "[[:type-error :type-error] [1 1]]");
}

/// A variadic call builds the rest sequence the dispatcher builds, after
/// evaluating every argument in order.
#[test]
fn variadic_calls_build_the_dispatchers_rest_sequence() {
    let source = r#"
        (defn trace [x] (.push js/trace x) x)
        (defn rest-of ([] :none) ([a & r] [(seq? r) (counted? r) (count r) (vec r) (meta r) (nil? r)]))
        (defn applied [a & r] r)
        (pr-str [(rest-of 1) (rest-of (trace 1) (trace 2) (trace 3))
                 (= (applied 1 2 3) (apply applied 1 [2 3]))
                 (= (type (applied 1 2)) (type (apply applied 1 [2]))) (vec js/trace)])"#;
    let (text, _) = source_at_every_level(
        source,
        "[[false true 0 [] nil true] [true true 2 [2 3] nil false] true true [1 2 3]]",
    );
    assert!(text.contains("rt$fnRest(["), "{text}");
}

/// A `do` in value position is a comma expression, without forms that have
/// no effect, unless a form reads the function scope that a function around
/// the `do` would replace, holds a definition with `:test` metadata (whose
/// test the return position skips), or the `do` holds one form.
#[test]
fn value_position_do_is_a_comma_expression() {
    let source = r#"
        (def sequence (do (.push js/trace "a") :ignored [1 2] nil (.push js/trace "b") 1))
        (def single (do 2))
        (defn arguments-in-do [a] (let [x (do (js-arguments))] (alength x)))
        (defn this-in-do [a] (let [x (do (js* "this !== 0"))] x))
        (pr-str [sequence single (arguments-in-do 1) (this-in-do 1) (vec js/trace)])"#;
    let (text, _) = source_at_every_level(source, "[1 2 0 true [\"a\" \"b\"]]");
    assert!(text.contains("[\"push\"](\"a\"), ("), "{text}");
    assert!(text.contains("g1 = 2))"), "{text}");
    assert_eq!(text.matches("(function () {").count(), 2, "{text}");
}

/// Calls of a local bound to a function with one fixed method are plain calls
/// too, also through a closure's captured local or `letfn`.
#[test]
fn local_functions_with_one_method_are_called_plainly() {
    let source = r#"
        (defn locals [n]
          (let [triple (fn [x] (* 3 x))]
            (letfn [(ev? [n] (if (zero? n) true (od? (dec n))))
                    (od? [n] (if (zero? n) false (ev? (dec n))))]
              [(triple n) (ev? n) (od? n) ((fn [] (triple 2)))])))
        (pr-str (locals 4))"#;
    let (text, _) = source_at_every_level(source, "[12 true false 6]");
    assert!(!text.contains(".call(null"), "{text}");
}

/// Known functions of one namespace are called directly from another.
#[test]
fn calls_across_namespaces_are_direct() {
    let root = Sources::new();
    // The host call keeps `scale` from being inlined (light-cljs-214).
    root.write(
        "tools.cljs",
        "(ns tools) (defn scale ([x] (scale x 2)) ([x k] (js/Math.abs (* x k)))) (defn plus [a b] (+ a b))",
    );
    root.write(
        "app.cljs",
        "(ns app (:require [tools :refer [scale plus]])) (pr-str [(scale 3) (scale 3 3) (plus 1 2)])",
    );
    let program = analyze_namespace_entry(
        "app",
        std::slice::from_ref(&root.0),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap();
    let (text, stats) = every_level(&program, "app", "[6 9 3]");
    assert!(!text.contains("switch (arguments.length)"), "{text}");
    assert!(!text.contains(".call(null"), "{text}");
    assert_eq!(stats.dispatchers_removed, 1, "{stats:?}");
}

/// An explicit export escapes: its dispatcher stays.
#[test]
fn exports_keep_their_dispatcher() {
    // The host call keeps the method from being inlined (light-cljs-214).
    let source = "(defn exported ([] 1) ([x] (js/Math.abs x))) (exported 2)";
    let program = program(source);
    let id = program
        .globals
        .iter()
        .position(|binding| binding.name == "exported")
        .unwrap();
    let prepared = prepare_application(
        &program,
        ApplicationOptions {
            optimize: OptimizationLevel::Advanced,
            exports: vec![GlobalId(id)],
            defines: Vec::new(),
            no_inlining: false,
        },
    );
    let text = prepared.emit_js();
    assert!(text.contains(&format!("g{id}_1(2)")), "{text}");
    assert!(text.contains("switch (arguments.length)"), "{text}");
    assert_eq!(execute(&format!("console.log({text});")), "2\n");
}

/// Below `Advanced` nothing changes.
#[test]
fn levels_below_advanced_keep_the_generic_shapes() {
    let program = program("(defn f ([] 0) ([x] x)) (def x (do 1 2)) (f 1)");
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let prepared = prepare_application(&program, ApplicationOptions::new(level));
        let js = prepared.emit_js();
        assert!(js.contains("switch (arguments.length)"), "{level}");
        assert!(js.contains("(function () {"), "{level}");
        assert!(prepared.call_shapes().is_none(), "{level}");
        assert_eq!(prepared.stats().direct_calls, 0, "{level}");
    }
}
