//! Rapid type analysis over program types and protocols at `Advanced`
//! (light-cljs-211; docs/advanced-optimization.md, Phase 2 step 4 and "Phase 2
//! rapid type analysis extension points").
//!
//! Every program runs at every level and must print what `None` prints. The
//! `Advanced` output is checked for what the analysis removes (methods of
//! types without instances, uncalled methods, unread protocols, dispatchers
//! and factories) and for devirtualized calls, so a removal or rewrite that
//! silently stops applying fails too. Sentinel strings mark code that must
//! leave the output.
use super::optimization::{Sources, execute};
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::runtime_manifest::RuntimeManifest;
use light_cljs::{
    ApplicationOptions, ApplicationStats, MacroHost, OptimizationLevel, PreparedApplication,
    Program, analyze, parse, prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn advanced(source: &str) -> PreparedApplication {
    prepare_application(
        &program(source),
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level; returns the program part of the `Advanced` output.
fn every_level(source: &str, expected: &str) -> String {
    program_at_every_level(&program(source), source, expected)
}

/// [`every_level`] for the namespace `entry` of `sources` (path, text).
fn entry_at_every_level(entry: &str, sources: &[(&str, &str)], expected: &str) -> String {
    let root = Sources::new();
    for (path, source) in sources {
        root.write(path, source);
    }
    let program = analyze_namespace_entry(
        entry,
        std::slice::from_ref(&root.0),
        &mut MacroHost::new(),
        AnalysisOptions::default(),
        &mut Vec::new(),
    )
    .unwrap_or_else(|error| panic!("{entry}: {error}"));
    program_at_every_level(&program, entry, expected)
}

fn program_at_every_level(program: &Program, source: &str, expected: &str) -> String {
    let mut output = String::new();
    for level in OptimizationLevel::ALL {
        let js = prepare_application(program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
        output = js;
    }
    program_text(&output).to_owned()
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

/// The runtime units an `Advanced` program embeds.
fn units(source: &str) -> Vec<&'static str> {
    advanced(source)
        .retention()
        .units
        .iter()
        .map(|unit| RuntimeManifest::embedded().unit(unit.unit).name)
        .collect()
}

/// Whether `unit` is one of the record methods (`recordMethods.hash`), each
/// kept only where an operation that dispatches it is.
fn record_method(unit: &str) -> bool {
    unit.starts_with("recordMethods.") && unit != "recordMethods.install"
}

fn stats(source: &str) -> ApplicationStats {
    advanced(source).stats().clone()
}

const SHAPES: &str = "(defprotocol Shape (area [s]) (perimeter [s]))
     (defrecord Square [side] Shape
       (area [_] (* side side)) (perimeter [_] (str \"SQUARE-PERIMETER\" side)))
     (defrecord Circle [r] Shape
       (area [_] (str \"CIRCLE-AREA\" r)) (perimeter [_] (str \"CIRCLE-PERIMETER\" r)))
     (deftype Tri [a b c] Shape
       (area [_] (str \"TRI-AREA\" a)) (perimeter [_] (str \"TRI-PERIMETER\" a)))";

#[test]
fn types_without_instances_and_uncalled_methods_leave_the_output() {
    let source = format!("{SHAPES} (area (->Square 3))");
    let text = every_level(&source, "9");
    for sentinel in [
        "SQUARE-PERIMETER",
        "CIRCLE-AREA",
        "CIRCLE-PERIMETER",
        "TRI-AREA",
        "TRI-PERIMETER",
        "Circle",
        "Tri",
    ] {
        assert!(!text.contains(sentinel), "{sentinel} in\n{text}");
    }
    // Nothing reads the protocol or a dispatcher: the one call reads the
    // implementation from its receiver.
    assert!(!text.contains("rt$protocol"), "{text}");
    assert!(
        text.contains("[\"cljs$user$Shape$area$arity$1\"]"),
        "{text}"
    );
    let stats = stats(&source);
    assert_eq!(stats.uninstantiated_types, 2);
    assert_eq!(stats.devirtualized_calls, 1);
    // Square's perimeter and both of Circle's and Tri's methods, and their
    // protocol markers.
    assert_eq!(stats.dropped_methods, 1 + 1 + 3 + 3);
    // Neither the record methods nor the generic core: the record is only
    // constructed and dispatched on.
    let units = units(&source);
    assert!(units.len() < 40, "{units:?}");
    assert!(!units.iter().any(|unit| record_method(unit)), "{units:?}");
    assert!(!units.contains(&"protocolMethod"));
}

#[test]
fn every_way_to_make_or_reach_an_instance_keeps_its_methods() {
    let check = |tail: &str, expected: &str| {
        every_level(&format!("{SHAPES} {tail}"), expected);
    };
    // Construction with `new`, the factories, through `apply`, and with the
    // constructor or a factory passed as a value.
    check("(area (Square. 2))", "4");
    check("(area (map->Square {:side 2}))", "4");
    check("(area (apply ->Square [2]))", "4");
    check("(defn make [f] (f 2)) (area (make ->Square))", "4");
    check(
        "(defn make [t] (new t 2)) (perimeter (make Circle))",
        "CIRCLE-PERIMETER2",
    );
    check("(count (.getBasis Tri))", "3");
    check(
        "(perimeter (js* \"new ~{}(1, 2, 3)\" Tri))",
        "TRI-PERIMETER1",
    );
    // A dispatcher passed as a value, and a method called only inside
    // another type's method.
    check(
        "(apply str (map area [(->Square 2) (->Circle 1)]))",
        "4CIRCLE-AREA1",
    );
    check(
        "(deftype Box [s] Shape (area [_] (perimeter s)) (perimeter [_] 0))
         (area (->Box (->Circle 5)))",
        "CIRCLE-PERIMETER5",
    );
    // `instance?` and `satisfies?` see instances; a type that never has any
    // keeps a bare constructor for `instance?`.
    check(
        "(array (instance? Square (->Square 1)) (instance? Circle (->Square 1))
                (satisfies? Shape (->Square 1)) (implements? Shape (->Square 1))
                (satisfies? Shape 1))",
        "[ true, false, true, true, false ]",
    );
}

#[test]
fn slots_the_program_names_keep_their_methods() {
    // Read with `aget`, called as a method, named in raw text: the method can
    // run although nothing calls its dispatcher.
    for (read, expected) in [
        ("(.call (aget t \"cljs$user$P$m$arity$1\") t t)", "7"),
        ("(.cljs$user$P$m$arity$1 t t)", "7"),
        ("(js* \"~{}.cljs$user$P$m$arity$1(~{})\" t t)", "7"),
        ("(some? (aget t \"cljs$user$P$\"))", "true"),
    ] {
        every_level(
            &format!("(defprotocol P (m [x])) (deftype T [v] P (m [_] v)) (let [t (T. 7)] {read})"),
            expected,
        );
    }
    every_level(
        "(defprotocol P (m [x])) (let [r (reify P (m [_] 7))] (.call (aget r \"cljs$user$P$m$arity$1\") r r))",
        "7",
    );
}

#[test]
fn reachable_methods_follow_construction_inside_methods_to_a_least_fixed_point() {
    // A's method makes a B and B's method an A, but nothing makes either.
    let source = "(defprotocol P (m [x]))
         (deftype A [] P (m [_] (str \"A-METHOD\" (m (B.)))))
         (deftype B [] P (m [_] (str \"B-METHOD\" (m (A.)))))
         (deftype C [] P (m [_] \"C-METHOD\"))
         (m (C.))";
    let text = every_level(source, "C-METHOD");
    assert!(
        !text.contains("A-METHOD") && !text.contains("B-METHOD"),
        "{text}"
    );
    // Once one is made, the other's method can run as well; a method nothing
    // calls stays out of both.
    let source = "(defprotocol P (m [x n]) (unused [x]))
         (deftype A [] P (m [_ n] (if (pos? n) (m (B.) (dec n)) \"A-DONE\")) (unused [_] \"A-UNUSED\"))
         (deftype B [] P (m [_ n] (if (pos? n) (m (A.) (dec n)) \"B-DONE\")) (unused [_] \"B-UNUSED\"))
         (m (A.) 3)";
    let text = every_level(source, "B-DONE");
    assert!(
        text.contains("A-DONE") && !text.contains("UNUSED"),
        "{text}"
    );
}

#[test]
fn protocols_and_dispatchers_are_defined_only_where_read() {
    let declared = "(defprotocol P (m [x]) (unused [x])) (extend-type string P (m [s] (str \"STRING-M\" s)) (unused [s] \"UNUSED\"))";
    // Unread: no protocol, no dispatcher, and the host extension goes too.
    let text = every_level(&format!("{declared} 1"), "1");
    assert!(
        !text.contains("rt$protocol") && !text.contains("STRING-M"),
        "{text}"
    );
    // Dispatched: the dispatcher and the extension stay, the unused method
    // does not.
    let text = every_level(&format!("{declared} (m \"x\")"), "STRING-Mx");
    assert!(
        text.contains("rt$protocolMethod") && !text.contains("UNUSED"),
        "{text}"
    );
    assert!(!text.contains("rt$protocol("), "{text}");
    // `satisfies?` reads the protocol and its markers.
    let text = every_level(&format!("{declared} (satisfies? P \"x\")"), "true");
    assert!(text.contains("rt$protocol("), "{text}");
    every_level(&format!("{declared} (implements? P \"x\")"), "false");
    // Other host types, a default extension and `nil`.
    every_level(
        "(defprotocol P (m [x]))
         (extend-protocol P number (m [n] (inc n)) nil (m [_] \"nil\") default (m [_] \"other\"))
         (array (m 1) (m nil) (m :k))",
        "[ 2, 'nil', 'other' ]",
    );
    // A `reify` keeps the methods something dispatches.
    let text = every_level(
        "(defprotocol P (m [x]) (n [x])) (m (reify P (m [_] 1) (n [_] \"REIFY-N\")))",
        "1",
    );
    assert!(!text.contains("REIFY-N"), "{text}");
}

#[test]
fn devirtualized_calls_keep_the_dispatcher_semantics() {
    // Arguments run in order, after the receiver, as for the dispatcher.
    let text = every_level(
        "(defprotocol P (m [x a b]))
         (deftype T [v] P (m [_ a b] (str v a b)))
         (def log (atom []))
         (defn note [x] (swap! log conj x) x)
         (str (m (do (note 0) (T. 1)) (note 2) (note 3)) (pr-str @log))",
        "123[0 2 3]",
    );
    assert!(text.contains("((r0, a1, a2) => r0["), "{text}");
    // A receiver in a local is read twice instead.
    let text = every_level(
        "(defprotocol P (m [x])) (deftype T [v] P (m [_] v)) (let [t (T. 7)] (m t))",
        "7",
    );
    assert!(text.contains(")[\"cljs$user$P$m$arity$1\"](l"), "{text}");
    // Narrowed by `instance?`; each arity has its own slot.
    let text = every_level(
        "(defprotocol P (m [x] [x y]))
         (deftype T [v] P (m [_] v) (m [_ y] (+ v y)))
         (defn f [x] (if (instance? T x) (+ (m x) (m x 10)) \"other\"))
         (array (f (T. 1)) (f 2))",
        "[ 12, 'other' ]",
    );
    assert!(
        text.contains("$arity$2\"]") && text.contains("$arity$1\"]"),
        "{text}"
    );
    // A later `extend-type` replaces the slot, and `specify!` shadows it on
    // one instance: the call reads the receiver's slot, so it sees both.
    every_level(
        "(defprotocol P (m [x]))
         (deftype T [v] P (m [_] :defined))
         (extend-type T P (m [_] :extended))
         (let [a (T. 1) b (specify! (T. 2) P (m [_] :specified))] (pr-str [(m a) (m b)]))",
        "[:extended :specified]",
    );
    // Munged names: a namespace with dashes and dots, punctuation in the
    // protocol and method names, a method named `/`.
    let text = entry_at_every_level(
        "my-app.core-ns",
        &[(
            "my_app/core_ns.cljs",
            "(ns my-app.core-ns)
             (defprotocol Proto-X (foo->bar! [x n]) (/ [x]))
             (deftype T [v] Proto-X (foo->bar! [_ n] (* v n)) (/ [_] (- v)))
             (let [t (T. 3)] (+ (foo->bar! t 2) (/ t)))",
        )],
        "3",
    );
    assert!(
        text.contains("[\"my_app$core_ns$Proto_X$foo__GT_bar_BANG_$arity$2\"]")
            && text.contains("[\"my_app$core_ns$Proto_X$_SLASH_$arity$1\"]"),
        "{text}"
    );
    // Not devirtualized: protocols extended through metadata, whose metadata
    // implementation wins, and variadic methods.
    let text = every_level(
        "(defprotocol P :extend-via-metadata true (m [x]))
         (defrecord R [v] P (m [_] :own))
         (pr-str [(m (->R 1)) (m (with-meta (->R 1) {`m (fn [_] :meta)}))])",
        "[:own :meta]",
    );
    assert!(!text.contains("$arity$1\"]("), "{text}");
    every_level(
        "(defprotocol P (m [x & more])) (deftype T [] P (m [_ & more] (count more))) (m (T.) 1 2)",
        "2",
    );
}

#[test]
fn records_keep_the_record_methods_their_operations_dispatch() {
    let record = "(defrecord R [a b])";
    for (tail, expected) in [
        ("(= (->R 1 2) (->R 1 2))", "true"),
        ("(= (hash (->R 1 2)) (hash (->R 1 2)))", "true"),
        ("(get (->R 1 2) :b)", "2"),
        ("(:c (assoc (->R 1 2) :c 3))", "3"),
        ("(count (assoc (->R 1 2) :c 3))", "3"),
        ("(count (seq (->R 1 2)))", "2"),
        ("(pr-str (->R 1 2))", "#cljs.user.R{:a 1, :b 2}"),
        ("(pr-str (into {} (->R 1 2)))", "{:a 1, :b 2}"),
        ("(record? (dissoc (->R 1 2) :a))", "false"),
        ("(:c (conj (->R 1 2) [:c 3]))", "3"),
        ("(contains? (->R 1 2) :b)", "true"),
        ("(reduce-kv (fn [acc k v] (+ acc v)) 0 (->R 1 2))", "3"),
        ("(:m (meta (with-meta (->R 1 2) {:m 1})))", "1"),
        ("(= (->R 1 2) (clone (->R 1 2)))", "true"),
        ("(.-a (map->R {:a 5}))", "5"),
        (
            "(let [s (atom 0)] (doseq [[_ v] (->R 1 2)] (swap! s + v)) @s)",
            "3",
        ),
        (
            "(let [it (-iterator (->R 1 2))] (loop [n 0] (if (.hasNext it) (recur (+ n (val (.next it)))) n)))",
            "3",
        ),
        ("(contains? #{(->R 1 2)} (->R 1 2))", "true"),
    ] {
        every_level(&format!("{record} {tail}"), expected);
    }
    // A record that is only constructed and read keeps no record method.
    let kept = units(&format!("{record} (.-a (->R 1 2))"));
    assert!(!kept.iter().any(|unit| record_method(unit)), "{kept:?}");
    // Counting keeps the count method.
    let kept = units(&format!("{record} (count (->R 1 2))"));
    assert!(kept.contains(&"recordMethods.count"), "{kept:?}");
}

#[test]
fn types_keep_get_basis_only_where_the_program_names_it() {
    let text = every_level("(deftype T [a b]) (.-a (T. 1 2))", "1");
    assert!(!text.contains("getBasis"), "{text}");
    let text = every_level("(deftype T [a b]) (count (.getBasis T))", "2");
    assert!(text.contains("getBasis"), "{text}");
    every_level("(deftype T [a b]) (count (js-invoke T \"getBasis\"))", "2");
}

#[test]
fn callable_types_and_reify_keep_their_invoke_machinery() {
    every_level(
        "(deftype F [] IFn (-invoke [_ x] (* 2 x)) (-invoke [_ x & more] (apply + x more)))
         (array ((F.) 4) (apply (F.) 1 2 [3]))",
        "[ 8, 6 ]",
    );
    every_level(
        "(defprotocol P (m [x])) (let [r (reify P (m [_] 1) IFn (-invoke [_ x] x))] (array (m r) (r 2)))",
        "[ 1, 2 ]",
    );
}
