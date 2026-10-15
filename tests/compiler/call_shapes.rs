//! Direct call shapes at `Advanced` (light-cljs-212; docs/advanced-optimization.md,
//! Phase 2 step 5 and "Phase 2 call shape extension points").
//!
//! Every program runs at every level and must print what `None` prints, both in
//! a sloppy script and in strict code, where `this` may differ. The `Advanced`
//! program text is checked for the shapes: calls of known functions name their
//! method (`g3_1(a)`) without `.call(null, ...)`, functions whose value never
//! escapes have no `arguments.length` dispatcher, and a `do` in value position
//! is a comma expression. Generic shapes must stay where the function is
//! redefined or assigned. Local and named functions with several methods
//! (light-cljs-228) call their methods' entries, hoisted to the constant table
//! when they capture nothing and bound next to their `let` otherwise, and keep
//! their dispatcher when their value escapes. The programs run as the terse
//! `Advanced` output; their shapes are read from the readable output
//! (`pseudo_names`, light-cljs-215).
use super::optimization::{Sources, execute, readable};
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
    let prepared = prepare_application(program, readable(OptimizationLevel::Advanced));
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

/// The output name of the first local named `name` (`l5`): the binding of a
/// `let`, or of a `letfn`, whose functions are named after their bindings.
fn local(program: &Program, name: &str) -> String {
    let id = program
        .locals
        .iter()
        .position(|binding| binding.name == name)
        .unwrap_or_else(|| panic!("no local {name}"));
    format!("l{id}")
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
/// the binding at the time of the call. A function that does not escape may be
/// defined anywhere: its entries are rebound together. One whose value escapes
/// from a definition that may run more than once binds entries too
/// (light-cljs-228), but its dispatcher holds the entries of its own
/// evaluation, so `made` keeps calling the first definition's methods.
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
    for name in ["redefined", "assigned", "dynamic", "redefs", "once", "raw"] {
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
    // `js*` turns on the hygiene renames.
    let text = text.replace("__light0_", "");
    let [outer, made] = ["outer", "made"].map(|name| global(&program, name));
    for shape in [
        format!("{outer}_0()"),
        format!("{outer} = (() => {{"),
        format!("const m0 = {outer}_0;"),
        format!("({made}).call(null, 10)"),
    ] {
        assert!(text.contains(&shape), "{shape}: {text}");
    }
}

/// Positional and map factories and protocol method dispatchers that only
/// their `defrecord`, `deftype` or `defprotocol` defines are called plainly
/// in later top-level forms (light-cljs-262): a factory is an arrow and a
/// dispatcher reads no `this`. Assigned or redefined ones keep
/// `.call(null, ...)`, and so does a type's own method, which inference does
/// not place after its definition.
#[test]
fn factories_and_dispatchers_are_called_plainly() {
    let source = r#"
        (defprotocol Area (area [s]) (scale [s k]))
        (defrecord Rect [w h] Area (area [_] (* w h)) (scale [_ k] (->Rect (* k w) h)))
        (deftype Sq [s] Area (area [_] (* s s)) (scale [_ k] (->Sq (* k s))))
        (defn pick [i] (if (even? i) (->Rect i 2) (->Sq i)))
        (defrecord Assigned [x])
        (defn reassign [] (set! ->Assigned (fn [x] [:assigned x])))
        (defrecord Redefined [x])
        (def ->Redefined (fn [x] [:redefined x]))
        (defprotocol Q (q [x]))
        (extend-type number Q (q [x] (inc x)))
        (defn requalify [] (set! q (fn [x] [:q x])))
        (pr-str [(area (pick 3)) (area (pick 4)) (:w (map->Rect {:w 5 :h 1}))
                 (area (scale (pick 5) 2)) (.-x (->Assigned 1)) (do (reassign) (->Assigned 2))
                 (->Redefined 3) (q 4) (do (requalify) (q 5))])"#;
    let program = program(source);
    let (text, _) = every_level(
        &program,
        source,
        "[9 8 5 100 1 [:assigned 2] [:redefined 3] 5 [:q 5]]",
    );
    let [rect, sq, map_rect, area, scale] =
        ["->Rect", "->Sq", "map->Rect", "area", "scale"].map(|name| global(&program, name));
    for shape in [
        format!("{rect}(l"),
        format!("{sq}(l"),
        format!("{map_rect}(k"),
        format!("{area}({scale}("),
        format!("({rect}).call(null, ("),
        format!("({sq}).call(null, ("),
    ] {
        assert!(text.contains(&shape), "{shape}: {text}");
    }
    for name in [&map_rect, &area, &scale] {
        assert!(!text.contains(&format!("{name}).call(")), "{name}: {text}");
    }
    for name in ["->Assigned", "->Redefined", "q"] {
        let name = global(&program, name);
        assert!(
            text.contains(&format!("({name}).call(null, ")),
            "{name}: {text}"
        );
    }
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
    // A factory or dispatcher called before its definition runs throws before
    // evaluating the arguments, as `.call(null, ...)` does (light-cljs-262):
    // only a read in a later top-level form than a top-level definition is a
    // plain call. A `declare` is a second definition, and a definition that
    // is not a top-level form is not known to run before any read.
    let source = r#"
        (declare ->Late map->Late q)
        (def log (atom []))
        (defn note [x] (swap! log conj x) x)
        (defn early []
          [(try (->Late (note :factory)) (catch js/TypeError e :type-error))
           (try (map->Late (note {:x :map})) (catch js/TypeError e :type-error))
           (try (q (note 1)) (catch js/TypeError e :type-error))])
        (def result (early))
        (defrecord Late [x])
        (defprotocol Q (q [x]))
        (extend-type number Q (q [x] (inc x)))
        (when (some? log) (defrecord Nested [x]))
        (defrecord Now [x])
        (pr-str [result @log (early) (:x (->Nested 4)) (:x (->Now 3))])"#;
    let program = program(source);
    let (text, _) = every_level(
        &program,
        source,
        "[[:type-error :type-error :type-error] [] [#cljs.user.Late{:x :factory} #cljs.user.Late{:x :map} 2] 4 3]",
    );
    for name in ["->Late", "map->Late", "q", "->Nested"] {
        let name = global(&program, name);
        assert!(
            text.contains(&format!("({name}).call(null, ")),
            "{name}: {text}"
        );
    }
    let now = global(&program, "->Now");
    assert!(text.contains(&format!("{now}(3)")), "{text}");
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
        std::slice::from_ref(root.0.path()),
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
            no_folding: false,
            pseudo_names: true,
        },
    );
    let text = prepared.emit_js();
    assert!(text.contains(&format!("g{id}_1(2)")), "{text}");
    assert!(text.contains("switch (arguments.length)"), "{text}");
    assert_eq!(execute(&format!("console.log({text});")), "2\n");
}

/// Local functions with several methods or rest arguments (light-cljs-228)
/// whose local and name are read only as the callee of calls that select a
/// method have no dispatcher, and their calls name their methods' entries. A
/// method that captures nothing, or only hoisted methods (itself, a sibling, a
/// `letfn` peer), is a function entry of the constant table, `k3`, created
/// once; one that reads its scope is a `const` next to the binding,
/// `const l5_1 = ...`, created at each evaluation of the scope, so closures
/// made in a loop see their own iteration. Methods no call selects are left
/// out.
#[test]
fn local_functions_call_the_entries_of_their_methods() {
    let source = r#"
        (defn locals [n]
          (let [pure (fn ([x] (* x 2)) ([x y] (* x y)))
                mixed (fn ([x] (pure x 1)) ([x y] (+ x y n)))
                rest-only (fn [& xs] (count xs))
                varied (fn ([] :none) ([a & more] [a more]))
                named (fn self ([x] (self x 0)) ([x acc] (if (zero? x) acc (self (dec x) (+ acc x)))))
                wrapped (do (fn ([] n) ([x] (+ x n)) ([x y] :unused)))]
            [(pure 3) (pure 3 4) (mixed 1) (mixed 1 2) (rest-only) (rest-only 1 2 3) (varied) (varied 1)
             (varied 1 2 3) (named 4) (wrapped) (wrapped 1)]))
        (defn mutual [n]
          (letfn [(ev? ([x] (if (zero? x) true (od? (dec x)))) ([x y] (ev? (+ x y))))
                  (od? ([x] (if (zero? x) false (ev? (dec x)))) ([x y] (od? (+ x y n))))]
            [(ev? 4) (od? 3) (ev? 1 1) (od? 1 1)]))
        (defn closures []
          (loop [i 0 acc []]
            (if (< i 3)
              (let [per-turn (fn ([] (fn [] i)) ([x] (+ x i)))]
                (recur (inc i) (conj acc (per-turn) (per-turn 10))))
              (mapv #(if (fn? %) (%) %) acc))))
        (pr-str [(locals 5) (mutual 1) (closures)])"#;
    let program = program(source);
    let (text, stats) = every_level(
        &program,
        source,
        "[[6 12 1 8 0 3 :none [1 nil] [1 (2 3)] 10 5 6] [true true true true] [0 10 1 11 2 12]]",
    );
    assert!(!text.contains("switch (arguments.length)"), "{text}");
    let [
        pure,
        mixed,
        rest_only,
        varied,
        named,
        wrapped,
        even,
        odd,
        turn,
    ] = [
        "pure",
        "mixed",
        "rest-only",
        "varied",
        "named",
        "wrapped",
        "ev?",
        "od?",
        "per-turn",
    ]
    .map(|name| local(&program, name));
    // Methods that read `n` or `i` stay in their scope, under their local.
    for shape in [
        format!("const {mixed}_1 = "),
        format!("{mixed}_1(1, 2)"),
        format!("const {wrapped}_0 = "),
        format!("{wrapped}_1(1)"),
        format!("const {odd}_1 = "),
        format!("{odd}_1(1, 1)"),
        format!("const {turn}_0 = "),
        format!("{turn}_1(10)"),
    ] {
        assert!(text.contains(&shape), "{shape}: {text}");
    }
    // The rest are hoisted, and no local binds the function itself.
    for name in [&pure, &rest_only, &varied, &named, &even] {
        assert!(!text.contains(&format!("{name}_")), "{name}: {text}");
    }
    for name in [
        &pure, &mixed, &rest_only, &varied, &named, &wrapped, &even, &odd, &turn,
    ] {
        assert!(
            !text.contains(&format!("const {name} = ")),
            "{name}: {text}"
        );
        assert!(!text.contains(&format!("let {name};")), "{name}: {text}");
    }
    assert!(!text.contains(&format!("{mixed}_0")), "{text}");
    assert!(!text.contains(&format!("{wrapped}_2")), "{text}");
    assert!(!text.contains(":unused"), "{text}");
    for arguments in [
        "3, 4",
        "null",
        "rt$fnRest([1, 2, 3], 0)",
        "1, rt$fnRest([2, 3], 0)",
        "4",
    ] {
        assert!(calls_entry(&text, arguments), "{arguments}: {text}");
    }
    assert_eq!(stats.dispatchers_removed, 9, "{stats:?}");
    assert_eq!(stats.hoisted_methods, 11, "{stats:?}");
    let pool = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .retention()
    .constants;
    assert_eq!(pool.functions, 11, "{pool:?}");
}

/// Whether `text` calls a constant table entry with these arguments,
/// `k3(1, 2)`.
fn calls_entry(text: &str, arguments: &str) -> bool {
    text.match_indices(&format!("({arguments})"))
        .any(|(at, _)| {
            let before = text[..at].trim_end_matches(|c: char| c.is_ascii_digit());
            before.len() < at
                && before.strip_suffix('k').is_some_and(|rest| {
                    !rest.ends_with(|c: char| c.is_alphanumeric() || "_$".contains(c))
                })
        })
}

/// Hoisted methods are table entries even when the output reads no runtime,
/// and so has no runtime slice for the table to follow.
#[test]
fn hoisted_methods_need_no_runtime() {
    let source = "(let [f (fn ([] 1) ([x] (* 2 x)))] (+ (f) (f 3)))";
    let (text, stats) = source_at_every_level(source, "7");
    assert!(!text.contains("rt$"), "{text}");
    assert!(calls_entry(&text, "3"), "{text}");
    assert_eq!(stats.hoisted_methods, 2, "{stats:?}");
}

/// A local function keeps its dispatcher when its value escapes: a
/// higher-order argument, `apply`, a value in a collection, an alias, a read
/// of its name as a value, a `reify` in its scope (whose object holds every
/// visible local), a method of `extend-type` that captures it, a `loop`
/// binding (which `recur` rebinds), a call whose count selects no method at
/// compile time (an invalid arity, or fewer arguments than the variadic
/// method's fixed parameters), and a `fn` literal called in place.
#[test]
fn local_functions_whose_value_escapes_keep_their_dispatcher() {
    let source = r#"
        (deftype Box [v])
        (defn escapes [k]
          (let [mapped (fn ([x] (inc x)) ([x y] (+ x y)))
                applied (fn ([] 0) ([x & more] (apply + x more)))
                returned (fn ([] k) ([x] x))
                aliased (fn ([] 1) ([x] x))
                alias aliased
                self-read (fn self ([] self) ([x] x))
                reified (fn ([] k) ([x] (+ x k)))
                r (reify Object (toString [_] (str (reified) (reified 1))))
                extended (fn ([] 1) ([x] (+ x k)))
                invalid (fn ([] 0) ([a b] 1))
                under (fn ([a] [a]) ([a b c & r] [a b c r]))]
            (extend-type Box IFn (-invoke ([this n] (extended (+ n (.-v this))))))
            [(mapv mapped [1 2]) (mapped 1 2) (apply applied 1 [2 3]) (applied 4) ((first [returned]) 7)
             (alias) (alias 2) (fn? (self-read)) (self-read 3) (str r) (reified 5) ((Box. 5) 3) (extended)
             (try (invalid 1) (catch :default e (.-message e))) (invalid) (under 1 2) (under 1)
             ((fn ([] 1) ([x] x)) 2)
             (loop [f (fn ([] 1) ([x] x)) i 0] (if (< i 2) (recur f (inc i)) [(f) (f i)]))]))
        (pr-str (escapes 10))"#;
    let (text, stats) = source_at_every_level(
        source,
        "[[2 3] 3 6 4 7 1 2 true 3 \"1011\" 15 18 1 \"Invalid arity: 1\" 0 [1 2 nil nil] [1] 2 [1 2]]",
    );
    assert_eq!(
        text.matches("switch (arguments.length)").count(),
        11,
        "{text}"
    );
    assert_eq!(stats.dispatchers_removed, 0, "{stats:?}");
    assert_eq!(stats.hoisted_methods, 0, "{stats:?}");
}

/// A method stays in its scope when it reads a local from outside itself
/// (also from a nested function), calls a function there with one method or a
/// scoped method, or holds `js*`, which cannot move; a method that holds `js*`
/// keeps `.call(null, ...)` on its entry. `this-as` and `(js-arguments)` read
/// the method's own receiver and arguments, so those methods are hoisted. A
/// `reify` in a method holds every visible local, which then escapes.
#[test]
fn methods_that_read_their_scope_or_hold_raw_text_stay_in_it() {
    let source = r#"
        (defn scopes [k]
          (let [triple (fn [x] (* 3 x))
                raw (fn ([] (js* "this === null || this === globalThis")) ([x] x))
                this-as-fn (fn ([] (this-as t (identical? t js/globalThis))) ([x] x))
                args (fn ([a] (alength (js-arguments))) ([a b & r] (alength (js-arguments))))
                nested (fn ([] (fn [] k)) ([x] x))
                single (fn ([] (triple 2)) ([x] x))
                reads-k (fn ([] k) ([x] (+ x k)))
                through (fn ([] (reads-k)) ([x] (reads-k 1)))
                hoisted (fn ([] (through 1)) ([x] (inc x)))]
            [(raw) (raw 1) (this-as-fn) (this-as-fn 1) (args 1) (args 1 2) (args 1 2 3 4) ((nested))
             (nested 1) (single) (single 1) (reads-k) (reads-k 1) (through) (through 1) (hoisted)
             (hoisted 1)]))
        (defn reified [k]
          (let [reifies (fn ([] (str (reify Object (toString [_] (str "r" k))))) ([x] x))
                alone (fn ([] (str (reify Object (toString [_] "s")))) ([x] x))]
            [(reifies) (reifies 1) (alone) (alone 1)]))
        (pr-str [(scopes 10) (reified 2)])"#;
    let program = program(source);
    let (text, stats) = every_level(
        &program,
        source,
        "[[true 1 true 1 1 3 3 10 1 6 1 10 11 10 11 11 2] [\"r2\" 1 \"s\" 1]]",
    );
    // `js*` turns on the hygiene renames.
    let text = text.replace("__light0_", "");
    let [
        raw,
        this_as,
        args,
        nested,
        single,
        reads_k,
        through,
        hoisted,
        reifies,
        alone,
    ] = [
        "raw",
        "this-as-fn",
        "args",
        "nested",
        "single",
        "reads-k",
        "through",
        "hoisted",
        "reifies",
        "alone",
    ]
    .map(|name| local(&program, name));
    for shape in [
        format!("{raw}_0.call(null)"),
        format!("const {nested}_0 = "),
        format!("const {single}_0 = "),
        format!("const {reads_k}_1 = "),
        format!("const {through}_0 = "),
        format!("const {through}_1 = "),
        // Calls `through`'s scoped method.
        format!("const {hoisted}_0 = "),
        format!("const {alone}_0 = "),
        format!("({reifies}).call(null)"),
    ] {
        assert!(text.contains(&shape), "{shape}: {text}");
    }
    for name in [&this_as, &args] {
        assert!(!text.contains(&format!("{name}_")), "{name}: {text}");
    }
    for name in [&raw, &nested, &single, &hoisted, &alone] {
        assert!(!text.contains(&format!("{name}_1")), "{name}: {text}");
    }
    assert!(calls_entry(&text, "1, 2, null"), "{text}");
    // `this-as` in a table entry, at the top level.
    assert!(text.contains(" = (function() {\n    { const l"), "{text}");
    assert_eq!(stats.dispatchers_removed, 9, "{stats:?}");
    assert_eq!(stats.hoisted_methods, 9, "{stats:?}");
}

/// A named global function with several methods (light-cljs-228) calls its
/// entries through its name when its `def` is a top-level form and the name
/// is read only as the callee of calls that select a method; the global's
/// dispatcher stays only when the global escapes. A name read as a value, a
/// call through the name that selects no method, and a `def` that may run
/// again (whose name is that evaluation's function) keep the generic shape.
#[test]
fn named_functions_call_their_entries_through_the_name() {
    let source = r#"
        (def named (fn self ([x] (self x 0)) ([x acc] (if (zero? x) acc (self (dec x) (+ acc x))))))
        (def escaping (fn self ([] 1) ([x] (+ x (self)))))
        (def self-value (fn self ([] self) ([x] x)))
        (def bad-count (fn self ([] (self 1 2 3)) ([x] x)))
        (declare inner)
        (defn make [n] (def inner (fn self ([] (self n 0)) ([x y] (+ x y)))) nil)
        (make 1)
        (def first-inner (inner))
        (make 2)
        (pr-str [(named 4) (mapv escaping [1 2]) (escaping) (fn? (self-value)) (self-value 2)
                 (try (bad-count) (catch :default e (.-message e))) first-inner (inner) (inner 3 4)])"#;
    let program = program(source);
    let (text, stats) = every_level(
        &program,
        source,
        "[10 [2 3] 1 true 2 \"Invalid arity: 3\" 1 2 7]",
    );
    let [named, escaping, self_value, bad_count, inner] =
        ["named", "escaping", "self-value", "bad-count", "inner"]
            .map(|name| global(&program, name));
    for shape in [
        format!("return {named}_1(l"),
        format!("{named}_0(4)"),
        format!("({escaping}_0 = "),
        format!("+ {escaping}_0())"),
        format!(", {escaping} = function("),
        format!("({self_value}).call(null, 2)"),
        format!("({bad_count}).call(null)"),
        format!("({inner}).call(null, 3, 4)"),
    ] {
        assert!(text.contains(&shape), "{shape}: {text}");
    }
    assert!(!text.contains(&format!("let {named};")), "{text}");
    for name in [&self_value, &bad_count, &inner] {
        assert!(!text.contains(&format!("{name}_0")), "{name}: {text}");
    }
    assert_eq!(stats.dispatchers_removed, 1, "{stats:?}");
}

/// Below `Advanced` nothing changes.
#[test]
fn levels_below_advanced_keep_the_generic_shapes() {
    let program = program(
        "(defn f ([] 0) ([x] x)) (def x (do 1 2)) (let [g (fn ([] 0) ([y] y))] [(f 1) (g 2)])",
    );
    for level in [OptimizationLevel::None, OptimizationLevel::Prepare] {
        let prepared = prepare_application(&program, ApplicationOptions::new(level));
        let js = prepared.emit_js();
        assert!(js.contains("switch (arguments.length)"), "{level}");
        assert!(js.contains("(function () {"), "{level}");
        let g = local(&program, "g");
        assert!(js.contains(&format!("({g}).call(null, 2)")), "{level}");
        assert!(prepared.call_shapes().is_none(), "{level}");
        assert_eq!(prepared.stats().direct_calls, 0, "{level}");
        assert_eq!(prepared.stats().hoisted_methods, 0, "{level}");
    }
}
