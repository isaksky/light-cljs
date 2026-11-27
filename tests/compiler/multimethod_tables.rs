//! Multimethods at every level, and where an `Advanced` program keeps their
//! tables (light-cljs-239; docs/advanced-optimization.md, "Phase 2
//! multimethod extension points").
//!
//! At `Advanced` a multimethod keeps its method table and cache as arrays of
//! the entries their array maps would hold, unless the program could see its
//! tables or tell arrays from maps: then `MultiFn.tables` keeps them in the
//! atoms the multimethod was built with, as every other level does. These tests run programs at every level, require every level to
//! print what `None` prints, and check which way the `Advanced` slice keeps
//! the tables.
use super::optimization::execute;
use light_cljs::analyzer::AnalysisOptions;
use light_cljs::compiler::analyze_namespace_entry;
use light_cljs::runtime_manifest::RuntimeManifest;
use light_cljs::{
    ApplicationOptions, MacroHost, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use std::path::PathBuf;

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

/// The runtime units the `Advanced` output of `program` embeds.
fn units(program: &Program) -> Vec<&'static str> {
    prepare_application(
        program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .retention()
    .units
    .iter()
    .map(|unit| RuntimeManifest::embedded().unit(unit.unit).name)
    .collect()
}

/// The program prints `expected` through `console.log` at every level, and
/// its `Advanced` slice keeps the tables in atoms exactly when `tables`; the
/// units of that slice.
fn every_level(source: &str, expected: &str, tables: bool) -> Vec<&'static str> {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("{js};")),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
    let units = units(&program);
    assert_eq!(
        units.contains(&"MultiFn.tables"),
        tables,
        "tables kept in atoms: {source}"
    );
    units
}

#[test]
fn methods_dispatch_from_the_entries_of_an_array_map() {
    // The program of the pellet, which the `defmulti` ratchet fixture measures.
    let units = every_level(
        "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}))",
        "1",
        false,
    );
    // The arrays need no map operations, atom writes, option parsing or set
    // of supers.
    for absent in [
        "assoc",
        "multiState",
        "resetMultiCache",
        "hash",
        "supers",
        "seqToMap",
    ] {
        assert!(!units.contains(&absent), "`{absent}` kept");
    }
    for (source, expected) in [
        // Defaults, a falsy method, a replaced method keeping its first key,
        // and keys that the array map compares by value.
        (
            "(defmulti m identity) (defmethod m :default [x] [:default x]) (js/console.log (pr-str [(m 5) (m nil)]))",
            "[[:default 5] [:default nil]]",
        ),
        (
            "(defmulti m identity) (defmethod m :a [_] nil) (defmethod m :default [_] 3) (js/console.log (pr-str (m :a)))",
            "nil",
        ),
        (
            "(defmulti m identity) (defmethod m :a [_] 1) (defmethod m :a [_] 2) (js/console.log (m :a))",
            "2",
        ),
        (
            "(defmulti m identity) (defmethod m (keyword \"a/b\") [_] 1) (defmethod m [1 \"x\"] [_] 2) (defmethod m nil [_] 3) (js/console.log (m (keyword \"a\" \"b\")) (m (vector 1 (str \"x\"))) (m nil))",
            "1 2 3",
        ),
        (
            "(defmulti m (fn [x y] [x y])) (defmethod m [:a :b] [_ _] 1) (js/console.log (m :a :b) (apply m [:a :b]))",
            "1 1",
        ),
        // Missing methods: the message, and `get-method`.
        (
            "(defmulti m identity) (defmethod m :a [_] 1) (js/console.log (try (m 5) (catch :default e (.-message e))) (pr-str [(get-method m :a) (get-method m :b) (-get-method m :b)]))",
            "No method in multimethod 'cljs.user/m' for dispatch value: 5 [#object[Function] nil nil]",
        ),
        (
            "(defmulti m identity) (defmethod m ##NaN [_] 1) (js/console.log (try (m ##NaN) (catch :default e (.-message e))))",
            "No method in multimethod 'cljs.user/m' for dispatch value: ##NaN",
        ),
        // Host constructors, which `isa?` relates by prototype: the later of
        // two methods that each dominate the other wins, in definition order.
        (
            "(defmulti m identity) (defmethod m js/Object [_] :object) (defmethod m js/Function [_] :function) (js/console.log (pr-str [(m js/Error) (m inc)]))",
            "[:function :function]",
        ),
        // Eight methods, which an array map holds.
        (
            "(defmulti m identity) (defmethod m 1 [_] 1) (defmethod m 2 [_] 2) (defmethod m 3 [_] 3) (defmethod m 4 [_] 4) (defmethod m 5 [_] 5) (defmethod m 6 [_] 6) (defmethod m 7 [_] 7) (defmethod m 8 [_] 8) (js/console.log (m 1) (m 8))",
            "1 8",
        ),
        // A `MultiFn` built as `defmulti` builds one.
        (
            "(def mm (MultiFn. 'mm identity :default (get-global-hierarchy) (atom {}) (atom {}) (atom {}) (atom nil))) (-add-method mm :a (fn [_] 1)) (js/console.log (mm :a) (instance? MultiFn mm) (ifn? mm) (fn? mm))",
            "1 true true false",
        ),
    ] {
        every_level(source, expected, false);
    }
}

#[test]
fn hierarchies_order_methods_as_the_array_map_does() {
    // A program that can give a hierarchy ancestors (`derive`, a hierarchy
    // of its own, `get-global-hierarchy`) keeps the tables in atoms; the
    // arrays' steps walk no hierarchy.
    for (source, expected, tables) in [
        (
            "(derive ::rect ::shape) (derive ::square ::rect) (defmulti m identity) (defmethod m ::shape [_] :shape) (defmethod m ::rect [_] :rect) (js/console.log (pr-str [(m ::square) (m ::rect) (m ::shape)]))",
            "[:rect :rect :shape]",
            true,
        ),
        // A diamond: neither method dominates, in either order of the walk.
        (
            "(derive ::a ::x) (derive ::a ::y) (defmulti m identity) (defmethod m ::x [_] :x) (defmethod m ::y [_] :y) (js/console.log (try (m ::a) (catch :default e (.-message e))))",
            "Multiple methods in multimethod 'cljs.user/m' match dispatch value: :cljs.user/a -> :cljs.user/y and :cljs.user/x, and neither is preferred",
            true,
        ),
        // Vectors of related values, and a hierarchy changed after a call.
        (
            "(defmulti m identity) (defmethod m [::shape ::shape] [_] :shapes) (js/console.log (try (m [::rect ::rect]) (catch :default e (.-message e)))) (derive ::rect ::shape) (js/console.log (pr-str (m [::rect ::rect])))",
            "No method in multimethod 'cljs.user/m' for dispatch value: [:cljs.user/rect :cljs.user/rect]\n:shapes",
            true,
        ),
        // A hierarchy of its own, through a Var.
        (
            "(def h (derive (make-hierarchy) ::a ::b)) (defmulti m identity :hierarchy #'h) (defmethod m ::b [_] :b) (js/console.log (pr-str [(m ::a) (isa? h ::a ::b) (isa? ::a ::b)]))",
            "[:b true false]",
            true,
        ),
        // `isa?` of a constructor reads the ancestors of its prototypes.
        (
            "(deftype T []) (def h {:parents {} :ancestors {(.-prototype js/Function) #{::thing}} :descendants {}}) (derive js/Function ::fn) (defmulti m identity :hierarchy #'h) (defmethod m ::thing [_] :thing) (js/console.log (pr-str [(isa? h T ::thing) (isa? T ::fn) (isa? h inc ::thing) (isa? h ::thing T) (m T)]))",
            "[true false true false :thing]",
            true,
        ),
        // The cache keeps an answer until the methods or the hierarchy
        // change, even when a constructor's prototypes do.
        (
            "(def C (fn [])) (defmulti m identity) (defmethod m js/Function [_] 1) (defmethod m :default [_] 2) (js/console.log (m C)) (js/Object.setPrototypeOf C nil) (js/console.log (m C) (m (fn [])))",
            "1\n1 1",
            false,
        ),
        // Vectors and map entries as dispatch values, compared item by item
        // where no vector can be built.
        (
            "(defmulti m identity) (defmethod m [:a js/Function] [_] :fn) (defmethod m :default [_] :none) (js/console.log (pr-str (map m (seq {:a inc :b inc}))))",
            "(:fn :none)",
            false,
        ),
        // The global hierarchy read other than by `defmulti`.
        (
            "(defmulti m identity) (defmethod m ::b [_] :b) (swap! (get-global-hierarchy) (fn [h] (derive h ::a ::b))) (js/console.log (pr-str (m ::a)))",
            ":b",
            true,
        ),
        // Protocol implementations can see the global hierarchy's `atom` look
        // its options up, and `isa?` build `(supers child)`.
        (
            "(extend-type PersistentArrayMap ILookup (-lookup ([m k] 7) ([m k nf] 7))) (js/console.log (pr-str (meta (get-global-hierarchy))))",
            "7",
            false,
        ),
    ] {
        every_level(source, expected, tables);
    }
}

#[test]
fn programs_that_could_see_the_tables_keep_them_in_atoms() {
    for (source, expected) in [
        // Operations that read or write the tables keep them themselves.
        (
            "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (count (methods m)) (count (prefers m)))",
            "1 0",
        ),
        (
            "(derive ::a ::x) (derive ::a ::y) (defmulti m identity) (defmethod m ::x [_] :x) (defmethod m ::y [_] :y) (prefer-method m ::y ::x) (js/console.log (pr-str (m ::a)))",
            ":y",
        ),
        (
            "(defmulti m identity) (defmethod m :a [_] 1) (remove-method m :a) (js/console.log (try (m :a) (catch :default e (.-message e))))",
            "No method in multimethod 'cljs.user/m' for dispatch value: :a",
        ),
        (
            "(defmulti m identity) (defmethod m :a [_] 1) (remove-all-methods m) (js/console.log (pr-str (get-method m :a)))",
            "nil",
        ),
        (
            "(def mm (->MultiFn 'mm identity :default (get-global-hierarchy) (atom {:a (fn [_] 1)}) (atom {}) (atom {:a (fn [_] 1)}) (atom nil))) (js/console.log (mm :a))",
            "1",
        ),
        // Fields named as a property, a string or in `js*` text.
        (
            "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (count @(.-method-table m)))",
            "1",
        ),
        (
            "(defmulti m :k) (defmethod m :default [_] 0) (m {:k :b}) (defmethod m :a [_] 1) (m {:k :a}) (js/console.log (count @(aget m \"method_cache\")))",
            "2",
        ),
        (
            "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (count @(js* \"~{}.prefer_table\" m)))",
            "0",
        ),
        // The hierarchy, which the arrays' steps read without `deref` as
        // `defmulti` gives it, replaced by a delay.
        (
            "(defmulti m identity) (defmethod m ::b [_] :b) (set! (.-hierarchy m) (delay (derive (make-hierarchy) ::a ::b))) (js/console.log (pr-str (m ::a)))",
            ":b",
        ),
        // A `MultiFn` with atoms the program holds, or built otherwise.
        (
            "(def t (atom {})) (def mm (MultiFn. 'mm identity :default (get-global-hierarchy) t (atom {}) (atom {}) (atom nil))) (-add-method mm :a (fn [_] 1)) (js/console.log (count @t))",
            "1",
        ),
        (
            "(def mm (MultiFn. 'mm identity :default (get-global-hierarchy) (atom {:a (fn [_] 1)}) (atom {}) (atom {}) (atom nil))) (js/console.log (mm :a))",
            "1",
        ),
        (
            "(def C MultiFn) (def mm (C. 'mm identity :default (get-global-hierarchy) (atom {}) (atom {}) (atom {}) (atom nil))) (-add-method mm :a (fn [_] 1)) (js/console.log (mm :a))",
            "1",
        ),
        (
            "(def h (atom (make-hierarchy))) (defmulti m identity :hierarchy h) (defmethod m :a [_] 1) (js/console.log (m :a))",
            "1",
        ),
        // Replaced core operations, through which the program can reach the
        // atoms `defmulti` builds.
        (
            "(def original-atom atom) (def refs #js []) (set! cljs.core/atom (fn [x] (let [a (original-atom x)] (.push refs a) a))) (defmulti m identity) (defmethod m :a [_] 1) (js/console.log (count @(aget refs 0)))",
            "1",
        ),
        // Core protocols extended to runtime types, whose implementations the
        // operations on the tables run: map entries are read lazily.
        (
            "(extend-type MapEntry IIndexed (-nth ([e i] (if (zero? i) (.call e nil 0) (throw (js/Error. \"unused-value\")))) ([e i nf] (if (zero? i) (.call e nil 0) nf)))) (defmulti m identity) (defmethod m :a [_] 1) (js/console.log (try (m :b) (catch :default e (.-message e))))",
            "No method in multimethod 'cljs.user/m' for dispatch value: :b",
        ),
        (
            "(extend-type PersistentArrayMap IAssociative (-assoc [m k v] {:x (fn [_] 2)}) (-contains-key? [m k] false)) (defmulti m identity) (defmethod m :a [_] 1) (js/console.log (try (m :a) (catch :default e (.-message e))))",
            "No method in multimethod 'cljs.user/m' for dispatch value: :a",
        ),
        // Dispatch values whose hashes the program chose, which a cache past
        // eight entries, a hash map, tells apart where they are equal: a
        // public constructor's cached hash, or an assigned `_hash`.
        (
            "(def k1 (Keyword. nil \"a\" \"same\" 1)) (def k2 (Keyword. nil \"b\" \"same\" 2)) (def h {:parents {} :ancestors (hash-map k1 #{::left} k2 #{::right}) :descendants {}}) (defmulti m identity :hierarchy #'h) (defmethod m ::left [_] :left) (defmethod m ::right [_] :right) (defmethod m :default [_] :default) (doseq [i (range 6)] (m i)) (js/console.log (pr-str [(m k1) (m k2)]))",
            "[:left :right]",
        ),
        // A vector's and a subvector's, which they hold in their own field
        // (light-cljs-1026).
        (
            "(def v1 (PersistentVector. nil 1 5 nil #js [:x] 1)) (def v2 (PersistentVector. nil 1 5 nil #js [:x] 2)) (def h {:parents {} :ancestors (hash-map v1 #{::left} v2 #{::right}) :descendants {}}) (defmulti m identity :hierarchy #'h) (defmethod m ::left [_] :left) (defmethod m ::right [_] :right) (defmethod m :default [_] :default) (doseq [i (range 6)] (m i)) (js/console.log (pr-str [(m v1) (m v2)]))",
            "[:left :right]",
        ),
        (
            "(def s1 (Subvec. nil [:x :y] 0 1 1)) (def s2 (Subvec. nil [:x :z] 0 1 2)) (def h {:parents {} :ancestors (hash-map s1 #{::left} s2 #{::right}) :descendants {}}) (defmulti m identity :hierarchy #'h) (defmethod m ::left [_] :left) (defmethod m ::right [_] :right) (defmethod m :default [_] :default) (doseq [i (range 6)] (m i)) (js/console.log (pr-str [(m s1) (m s2)]))",
            "[:left :right]",
        ),
        (
            "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}) (some? (aget #js {} \"_hash\")))",
            "1 false",
        ),
        // Dispatch values with equality of their own, which the cache's
        // lookups run on more keys than the method table's.
        (
            "(def calls (atom 0)) (deftype K [x] IEquiv (-equiv [_ o] (swap! calls inc) (and (instance? K o) (= x (.-x o))))) (defmulti m identity) (defmethod m :default [_] 0) (m (K. 1)) (m (K. 2)) (js/console.log (m (K. 3)) @calls)",
            "0 15",
        ),
        // A program type implementing `IMultiFn`, whose `-add-method` the
        // protocol's dispatcher calls.
        (
            "(defmulti m identity) (defmethod m :a [_] 1) (def r (reify IMultiFn (-add-method [this d f] (js/console.log \"added\" (pr-str d)) this))) (defmethod r :b [_] 2) (js/console.log (m :a))",
            "added :b\n1",
        ),
        // Slots of `IMultiFn` and protocol masks named in the program, with
        // which host code gives a multimethod implementations.
        (
            "(defmulti m identity) (defmethod m :a [_] 1) (set! (.-cljs$lang$protocol_mask$partition0$ m) 0) (set! (.-cljs$core$IMultiFn$_get_method$arity$2 m) (fn [_ _] (fn [_] 2))) (js/console.log (m :a))",
            "2",
        ),
        (
            "(defmulti m identity) (set! (.-cljs$lang$protocol_mask$partition0$ m) 0) (set! (.-cljs$core$IMultiFn$_add_method$arity$3 m) (fn [this d f] (js/console.log \"slot\" (pr-str d)) this)) (defmethod m :a [_] 1) (js/console.log (pr-str (get-method m :a)))",
            "slot :a\nnil",
        ),
        // Methods added more than eight times, or other than at top level.
        (
            "(defmulti m identity) (defmethod m 1 [_] 1) (defmethod m 2 [_] 2) (defmethod m 3 [_] 3) (defmethod m 4 [_] 4) (defmethod m 5 [_] 5) (defmethod m 6 [_] 6) (defmethod m 7 [_] 7) (defmethod m 8 [_] 8) (defmethod m 9 [_] 9) (js/console.log (m 1) (m 9))",
            "1 9",
        ),
        (
            "(defmulti m identity) (defn add [k] (defmethod m k [_] k)) (doseq [k (range 12)] (add k)) (js/console.log (m 0) (m 11))",
            "0 11",
        ),
        (
            "(defmulti m identity) (apply -add-method [m :a (fn [_] 1)]) (js/console.log (m :a))",
            "1",
        ),
    ] {
        every_level(source, expected, true);
    }
}

#[test]
fn multimethods_are_named_by_text_where_only_messages_read_names() {
    // Messages print the text of the name as they print the symbol, with
    // and without the printer; without it they keep no symbol type.
    let units = every_level(
        "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}) (try (m {:k :b}) (catch :default e (.-message e))))",
        "1 No method in multimethod 'cljs.user/m' for dispatch value: :b",
        false,
    );
    for absent in ["symbol", "SymbolValue"] {
        assert!(!units.contains(&absent), "`{absent}` kept");
    }
    every_level(
        "(defmulti m identity) (defmethod m :a [_] 1) (js/console.log (pr-str (try (m 2.5) (catch :default e (.-message e)))))",
        "\"No method in multimethod 'cljs.user/m' for dispatch value: 2.5\"",
        false,
    );
    every_level(
        "(derive ::a ::x) (derive ::a ::y) (defmulti m identity) (defmethod m ::x [_] :x) (defmethod m ::y [_] :y) (js/console.log (try (m ::a) (catch :default e (.-message e))))",
        "Multiple methods in multimethod 'cljs.user/m' match dispatch value: :cljs.user/a -> :cljs.user/y and :cljs.user/x, and neither is preferred",
        true,
    );
    // Code that can call `name` or `namespace`, or names the field, reads
    // the symbol.
    for (source, expected) in [
        (
            "(defmulti m :k) (js/console.log (name m) (namespace m))",
            "m cljs.user",
        ),
        (
            "(defmulti m :k) (js/console.log (-name m) (-namespace m))",
            "m cljs.user",
        ),
        (
            "(defmulti m :k) (js/console.log (pr-str (map name [m])))",
            "(\"m\")",
        ),
        (
            "(defmulti m :k) (js/console.log (pr-str (.-name m)))",
            "cljs.user/m",
        ),
        (
            "(defmulti m :k) (js/console.log (pr-str (aget m \"name\")))",
            "cljs.user/m",
        ),
        (
            "(defmulti m :k) (js/console.log (pr-str (js* \"~{}.name\" m)))",
            "cljs.user/m",
        ),
    ] {
        let units = every_level(source, expected, false);
        assert!(units.contains(&"SymbolValue"), "{source}");
    }
}

/// `MultiFn`'s `call` passes 0 to 3 arguments as they came and more through
/// `apply` (light-cljs-1083): each reaches the dispatch function and the
/// method with its own count, which a dispatch function of fixed arities
/// and a method with a rest parameter read.
#[test]
fn calls_pass_each_count_of_arguments_to_the_dispatch_function_and_method() {
    every_level(
        "(defmulti m (fn [& xs] (count xs))) (defmethod m 0 [] :zero) (defmethod m 1 [a] [:one a]) (defmethod m 2 [a b] [:two a b]) (defmethod m 3 [a b c] [:three a b c]) (defmethod m :default [& xs] (vec xs)) \
         (defmulti n (fn ([a] :one) ([a b] :two) ([a b c] :three))) (defmethod n :one [a] [a]) (defmethod n :two [a b] [a b]) (defmethod n :three ([a b c] [a b c]) ([a b c d] :four)) \
         (js/console.log (pr-str [(m) (m 1) (m 1 2) (m 1 2 3) (m 1 2 3 4) (m 1 2 3 4 5) (apply m []) (apply m [1 2]) (apply m 1 2 3 [4 5 6]) ((identity m) 7) \
                                  (n :a) (n :a :b) (n :a :b :c) (try (n) (catch :default _ :arity)) (try (n 1 2 3 4) (catch :default _ :arity))]))",
        "[:zero [:one 1] [:two 1 2] [:three 1 2 3] [1 2 3 4] [1 2 3 4 5] :zero [:two 1 2] [1 2 3 4 5 6] [:one 7] [:a] [:a :b] [:a :b :c] :arity :arity]",
        false,
    );
}

/// Where the tables are in atoms, a call's `-get-method` takes the
/// dispatcher's steps for two arguments (light-cljs-1083): a program's own
/// `-get-method` of a multimethod still answers its calls, of each count,
/// and another multimethod's calls take its tables.
#[test]
fn a_programs_get_method_answers_calls_where_the_tables_are_in_atoms() {
    every_level(
        "(defmulti m identity) (defmethod m :a [x] [:a x]) (defmulti n (fn [x & _] x)) (defmethod n :a [x & more] [:n x more]) \
         (specify! m IMultiFn (-get-method [_ d] (fn [& xs] [:specified d xs]))) \
         (js/console.log (pr-str [(m :a) (m :b) (n :a) (n :a 1 2 3) (apply n [:a 1]) (fn? (get-method n :a))]))",
        "[[:specified :a (:a)] [:specified :b (:b)] [:n :a nil] [:n :a (1 2 3)] [:n :a (1)] true]",
        true,
    );
}

/// The arrays cache a walk's answer for a keyword, a symbol, a string, a
/// number or a vector of them, as upstream's cache does, found by identity
/// first; an equal value that is not identical finds it too, and a new
/// method clears it (light-cljs-1083).
#[test]
fn plain_dispatch_values_find_cached_answers_until_a_method_changes() {
    every_level(
        "(defmulti c identity) (defmethod c :a [_] 1) (defmethod c :default [x] [:default x]) \
         (defn all [] [(c :a) (c :b) (c :b) (c (keyword \"b\")) (c 'x) (c 'x) (c (symbol \"x\")) (c \"s\") (c (str \"s\")) (c [:a 'x 1]) (c (vector :a 'x 1)) (c 2) (c 2) (c nil) (c true)]) \
         (js/console.log (pr-str (all))) \
         (defmethod c :b [_] 2) (defmethod c 'x [_] 3) (defmethod c [:a 'x 1] [_] 4) (defmethod c 2 [_] 5) \
         (js/console.log (pr-str (all))) \
         (js/console.log (pr-str (mapv c (range 20))) (pr-str (mapv c (range 20))))",
        "[1 [:default :b] [:default :b] [:default :b] [:default x] [:default x] [:default x] [:default \"s\"] [:default \"s\"] [:default [:a x 1]] [:default [:a x 1]] [:default 2] [:default 2] [:default nil] [:default true]]\n\
         [1 2 2 2 3 3 3 [:default \"s\"] [:default \"s\"] 4 4 5 5 [:default nil] [:default true]]\n\
         [[:default 0] [:default 1] 5 [:default 3] [:default 4] [:default 5] [:default 6] [:default 7] [:default 8] [:default 9] [:default 10] [:default 11] [:default 12] [:default 13] [:default 14] [:default 15] [:default 16] [:default 17] [:default 18] [:default 19]] [[:default 0] [:default 1] 5 [:default 3] [:default 4] [:default 5] [:default 6] [:default 7] [:default 8] [:default 9] [:default 10] [:default 11] [:default 12] [:default 13] [:default 14] [:default 15] [:default 16] [:default 17] [:default 18] [:default 19]]",
        false,
    );
}

/// Past 16 entries the arrays cache a plain value's answer by its code
/// (`more`, light-cljs-1083): keywords, symbols, numbers, strings and
/// vectors of them answer as a walk does, equal values that are not
/// identical too, and a new method (including replacing `:default`) clears
/// both the first 16 entries and the overflow cache.
#[test]
fn plain_dispatch_values_past_sixteen_answers_find_them_until_a_method_changes() {
    every_level(
        "(defmulti d identity) (defmethod d :default [x] [:default x]) \
         (defn many [] [(mapv d (map #(keyword (str \"k\" %)) (range 20))) (mapv d (map #(vector % (symbol (str \"s\" %)) [(str %)]) (range 20))) (mapv d (range 20))]) \
         (js/console.log (= (many) (many)) (pr-str [(d :k0) (d :k19) (d [19 's19 [\"19\"]]) (d 19) (d \"19\") (d 'k19)])) \
         (defmethod d :k19 [_] 1) (defmethod d [19 's19 [\"19\"]] [_] 2) (defmethod d 19 [_] 3) \
         (js/console.log (pr-str [(d :k19) (d (keyword \"k19\")) (d [19 (symbol \"s19\") [(str 19)]]) (d 19) (d 18) (d \"19\") (d 'k19)])) \
         (many) \
         (defmethod d :default [x] [:new-default x]) \
         (js/console.log (pr-str [(d :k0) (d :k18) (d [18 's18 [\"18\"]]) (d 18) (d \"19\") (d 'k19) (d :k19) (d 19)]))",
        "true [[:default :k0] [:default :k19] [:default [19 s19 [\"19\"]]] [:default 19] [:default \"19\"] [:default k19]]\n\
         [1 1 2 3 [:default 18] [:default \"19\"] [:default k19]]\n\
         [[:new-default :k0] [:new-default :k18] [:new-default [18 s18 [\"18\"]]] [:new-default 18] [:new-default \"19\"] [:new-default k19] 1 3]",
        false,
    );
}

/// Nested plain vectors in the overflow cache serialize once at the root
/// (`leanCode`, light-cljs-1083), so a 30-level singleton vector keeps a
/// linear key instead of growing exponentially under repeated JSON escaping.
#[test]
fn nested_plain_dispatch_values_have_linear_overflow_keys() {
    every_level(
        "(defmulti nested identity) (defmethod nested :default [_] :old) \
         (defn wrap [leaf] (loop [i 0 v leaf] (if (< i 30) (recur (inc i) [v]) v))) \
         (doseq [i (range 20)] (nested i)) \
         (def a (wrap 0)) (def b (wrap 0)) (def c (wrap 1)) \
         (js/console.log (pr-str [(nested a) (nested b) (nested c)])) \
         (defmethod nested :default [_] :new) \
         (js/console.log (pr-str [(nested a) (nested b) (nested c)]))",
        "[:old :old :old]\n[:new :new :new]",
        false,
    );
}

/// The arrays keep a plain value's answer (above) in the cache only while
/// it holds fewer than 16 entries, past them in a `Map` by its code, and
/// others only where they can change while the methods do not (a
/// function's, also in a vector), so distinct dispatch values leave the
/// cache, a scan, short: 60,000 of them would take minutes to scan through
/// a cache holding every earlier one.
#[test]
fn distinct_dispatch_values_leave_the_cache_as_it_was() {
    every_level(
        "(defmulti m identity) (defmethod m :default [x] x) (defmethod m 7 [_] :seven) (js/console.log (loop [i 0 sum 0] (if (< i 60000) (recur (inc i) (+ sum (if (= i 7) 0 (m i)))) sum)) (pr-str (m 7)) (pr-str (m [1 1])))",
        "1799969993 :seven [1 1]",
        false,
    );
}

/// Functions are cached by identity, so 60,000 fresh closures as dispatch
/// values take no scan of a cache holding every earlier one, which would
/// take minutes (from the Sol review).
#[test]
fn distinct_function_dispatch_values_do_not_scan_a_growing_cache() {
    every_level(
        "(defmulti m identity) (defmethod m :default [_] 1) (js/console.log (loop [i 0 sum 0] (if (< i 60000) (recur (inc i) (+ sum (m (fn [] i)))) sum)))",
        "60000",
        false,
    );
}

/// A program that names the default dispatch value can change it after the
/// cache holds an answer for it, which the persistent cache keeps, so it
/// keeps the tables in atoms (from the Sol review).
#[test]
fn changing_default_dispatch_value_preserves_cached_answers() {
    every_level(
        "(defmulti m identity :default :a) (defmethod m :a [_] 1) (defmethod m :b [_] 2) (js/console.log (m :missing)) (set! (.-default-dispatch-val m) :b) (js/console.log (m :missing))",
        "1\n1",
        true,
    );
}

/// Host code can give a value equality or a hash by naming the protocol
/// slot, which the persistent cache's lookups run where the slice keeps the
/// lookup of such slots, so naming one keeps the tables in atoms (from the
/// Sol review). `Advanced` ignores the slot in this program, whose `=` and
/// map lookups did not run it there before light-cljs-239 either, so only
/// `None` and `Prepare` print what the slot counts.
#[test]
fn named_equivalence_slots_preserve_cache_effects() {
    for slot in [
        "cljs$core$IEquiv$_equiv$arity$2",
        "cljs$core$IHash$_hash$arity$1",
    ] {
        let source = format!(
            "(def calls (atom 0)) (def k #js {{}}) (set! (.-{slot} k) (fn [& _] (swap! calls inc) 0)) (defmulti m identity) (defmethod m :default [_] 0) (m k) (m k) (js/console.log (pos? @calls) (m k))"
        );
        let program = program(&source);
        let printed = [OptimizationLevel::None, OptimizationLevel::Prepare].map(|level| {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            execute(&format!("{js};"))
        });
        // An array map's lookups run equality only; the slot of `IHash` runs
        // in a cache of more than eight entries.
        let expected = if slot.contains("IEquiv") {
            "true 0\n"
        } else {
            "false 0\n"
        };
        assert_eq!(printed, [expected; 2], "{source}");
        assert!(units(&program).contains(&"MultiFn.tables"), "{source}");
    }
}

/// The multimethod fixtures of the pinned upstream test suite, each a
/// namespace whose value is its `run` function, pass at every level, with
/// the tables kept as each needs.
#[test]
fn upstream_multimethod_fixtures_pass_at_every_level() {
    let fixtures =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/compiler-multimethods");
    for (fixture, tables) in [
        ("test-cljs-1144", false),
        ("test-cljs-3367-backward-conflict-prefers", true),
        ("test-hierarchy", false),
        ("test-multimethods-1", true),
        ("test-multimethods-2", true),
    ] {
        let program = analyze_namespace_entry(
            "cljs.core-test",
            &[fixtures.join(fixture)],
            &mut MacroHost::new(),
            AnalysisOptions::default(),
            &mut Vec::new(),
        )
        .unwrap_or_else(|error| panic!("{fixture}: {error}"));
        for level in OptimizationLevel::ALL {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            assert_eq!(
                execute(&format!("console.log(({js})(null));")),
                "true\n",
                "{fixture} at {level}"
            );
        }
        assert_eq!(
            units(&program).contains(&"MultiFn.tables"),
            tables,
            "{fixture}: tables kept in atoms"
        );
    }
}

/// At `Advanced` a `defmulti` is inert when its dispatch is a keyword, a
/// `fn` literal, a core value or a function Var defined before it and
/// never assigned, it gives no `:hierarchy`, and its default is a literal
/// (light-cljs-300): unread and without methods it is removed, and the
/// runtime's `MultiFn` with it. Since light-cljs-1165 its top-level
/// `defmethod`s go with it; one that something else reads stays with its
/// methods, and so does one with a `:hierarchy`, a computed
/// dispatch, a collection default, or a dispatch Var defined later or
/// assigned. Every level prints what `None` prints.
#[test]
fn an_unread_defmulti_without_methods_is_removed() {
    for (source, removed) in [
        ("(defmulti m :k)", true),
        ("(defmulti m (fn [x] (:k x)) :default :none)", true),
        ("(defmulti m identity)", true),
        ("(defmulti m *print-fn*)", true),
        (
            "(defmulti m identity) (set! cljs.core/identity (fn [x] x))",
            true,
        ),
        ("(defn k [x] (:k x)) (defmulti m k)", true),
        ("(declare k) (defn k [x] (:k x)) (defmulti m k)", true),
        (
            "(declare k) (defn k [x] (:k x)) (defmulti m k) (defmethod m :a [_] 1)",
            true,
        ),
        ("(defmulti m :k) (defmethod m :a [_] 1)", true),
        (
            "(defmulti m :k) (defmethod m :a [_] 1) (js/console.log (m {:k :a}))",
            false,
        ),
        ("(defmulti m :k) (js/console.log (fn? m))", false),
        (
            "(def h (make-hierarchy)) (defmulti m :k :hierarchy #'h)",
            false,
        ),
        ("(defmulti m (comp :k identity))", false),
        ("(defmulti m :k :default [:none])", false),
        ("(declare k) (defmulti m k) (defn k [x] x)", false),
        ("(defn k [x] x) (defmulti m k) (set! k (fn [x] 2))", false),
        (
            "(declare k) (defn k [x] x) (defn k [x] 2) (defmulti m k)",
            false,
        ),
        (
            "(declare k) (defn k [x] x) (defmulti m k) (set! k (fn [x] 2))",
            false,
        ),
        ("(declare k) (defmulti m k)", false),
    ] {
        let source = format!("{source} (js/console.log 1)");
        let program = program(&source);
        let expected = execute(&format!(
            "{};",
            prepare_application(&program, ApplicationOptions::new(OptimizationLevel::None))
                .emit_js()
        ));
        for level in OptimizationLevel::ALL {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            assert_eq!(execute(&format!("{js};")), expected, "at {level}: {source}");
        }
        let js = prepare_application(
            &program,
            ApplicationOptions::new(OptimizationLevel::Advanced),
        )
        .emit_js();
        assert_eq!(
            !units(&program).contains(&"MultiFn") && !js.contains("cljs.user/m"),
            removed,
            "removed: {source}\n{js}"
        );
    }
}

/// An unread `defmulti` whose building the program can observe stays at
/// every level (light-cljs-300): where it replaces `atom`,
/// `get-global-hierarchy` or `MultiFn`, whose calls then run its code, or
/// extends a core protocol, whose implementations creating the global
/// hierarchy may run. Each program prints the effects of building it; the
/// lookups the hierarchy's creation makes are as many at `Advanced` as on
/// master (two, against ten below it), which this pellet does not change.
#[test]
fn unread_multimethods_preserve_initializer_effects() {
    for (source, expected) in [
        (
            "(def calls (volatile! 0)) (let [a atom] (set! atom (fn [x] (vswap! calls inc) (a x)))) (defmulti m :k) (js/console.log @calls)",
            "4",
        ),
        (
            "(def calls (volatile! 0)) (let [g get-global-hierarchy] (set! get-global-hierarchy (fn [] (vswap! calls inc) (g)))) (defmulti m :k) (js/console.log @calls)",
            "1",
        ),
        (
            "(def calls (volatile! 0)) (set! MultiFn (fn [& _] (vswap! calls inc) nil)) (defmulti m :k) (js/console.log @calls)",
            "1",
        ),
        (
            "(extend-type PersistentArrayMap ILookup (-lookup ([m k] (js/console.log \"lookup\") nil) ([m k nf] (js/console.log \"lookup\") nf))) (defmulti m :k) (js/console.log \"done\")",
            "lookup\n",
        ),
    ] {
        let program = program(source);
        for level in OptimizationLevel::ALL {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            let printed = execute(&format!("{js};"));
            if expected == "lookup\n" {
                assert!(
                    printed.starts_with(expected) && printed.ends_with("done\n"),
                    "at {level}: {source}\n{printed}"
                );
            } else {
                assert_eq!(printed, format!("{expected}\n"), "at {level}: {source}");
            }
        }
        assert!(units(&program).contains(&"MultiFn"), "removed: {source}");
    }
}
