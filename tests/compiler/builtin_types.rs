//! Rapid type analysis for the runtime's builtin types at `Advanced`
//! (light-cljs-227; docs/advanced-optimization.md, "Phase 2 builtin types
//! extension points").
//!
//! A generic operation reaches protocol machinery only through
//! `protocolHooks`, which `installMethods` fills in, and each kind's branch
//! only through `kindHooks`, which the units building the kind fill in. A
//! program that builds a vector therefore keeps no map, set or protocol
//! machinery. These tests run programs at every level, require every level
//! to give what `None` gives, and check what the `Advanced` slice keeps:
//! - each kind's branch of the generic operations, with and without protocol
//!   implementations in the program (the hooks closed and open);
//! - the host-facing methods of builtin prototypes (`equiv`, `indexOf`,
//!   iteration, `call`) on the kinds a small program builds;
//! - seqs of host iterables, which the generic operations know natively;
//! - the collection corpora of the other suites, printed with `pr-str`.
//!
//! Since light-cljs-230 ("Phase 2 builtin types, second pass") core
//! dispatchers and protocol tests answer natively, vectors and maps answer
//! their host-facing methods from their own items, the printer reads options
//! only from its public entries, and `str`, `atom` and map literals keep no
//! printer, option parsing or hash map of their own. Since light-cljs-231
//! sets do the same, a small set literal builds an array set, and a
//! membership test of a small set literal compares the tested value with its
//! members in value position too.
use super::optimization::execute;
use light_cljs::runtime_manifest::RuntimeManifest;
use light_cljs::{
    ApplicationOptions, OptimizationLevel, PreparedApplication, Program, analyze, parse,
    prepare_application,
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
/// level.
fn every_level(source: &str, expected: &str) {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
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

/// The protocol machinery a program without protocol implementations drops.
const PROTOCOL_MACHINERY: [&str; 7] = [
    "installMethods",
    "builtin",
    "publicMethod",
    "satisfies",
    "implementsProtocol",
    "builtinCoreImplementation",
    "protocolHooks.install",
];

fn assert_keeps_none(source: &str, absent: &[&str]) -> Vec<&'static str> {
    let units = units(source);
    for unit in absent {
        assert!(!units.contains(unit), "{source} keeps `{unit}`: {units:?}");
    }
    units
}

#[test]
fn a_vector_keeps_no_map_set_or_protocol_machinery() {
    let source = "(js/console.log [1 2 3])";
    let units = assert_keeps_none(
        source,
        &[
            "makeMap",
            "makeSet",
            "PersistentArrayMapValue",
            "PersistentHashMapValue",
            "PersistentHashSetValue",
            "treeMapConstructor",
            "hash",
            "makeMap.seq",
            "makeMap.equiv",
            "makeTransient.nth",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    // Its host-facing methods read its items (light-cljs-230), so it keeps
    // no seq and none of the seq types a vector builds.
    for unit in [
        "seq",
        "first",
        "next",
        "count",
        "nth",
        "makeVector.seq",
        "IndexedSeqValue",
        "ChunkedSeqValue",
        "equivSeqs",
    ] {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    for unit in ["makeVector.equiv", "itemIterator", "vectorIndexOf"] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
}

#[test]
fn a_keyword_keeps_no_collection() {
    let units = assert_keeps_none(
        "(js/console.log :a)",
        &[
            "makeVector",
            "makeMap",
            "makeSet",
            "view",
            "seq",
            "count",
            "hash",
            "emptyList",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    assert!(units.contains(&"get") && units.contains(&"objectEquiv"));
}

#[test]
fn an_empty_map_keeps_only_its_own_map_type() {
    let units = assert_keeps_none(
        "(js/console.log {})",
        &[
            "PersistentHashMapValue",
            "treeMapConstructor",
            "PersistentHashMapValue.traversal",
            "PersistentHashMapValue.put",
            "hashNodeAssoc",
            "hash",
            "makeSet",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    // It iterates its entries itself (light-cljs-230), and keeps no seq.
    for unit in ["seq", "makeMap.seq", "mapProjection", "es6Iterator"] {
        assert!(!units.contains(&unit), "{unit}: {units:?}");
    }
    assert!(units.contains(&"makeMap.lookup") && units.contains(&"mapIterator"));
}

#[test]
fn a_program_type_opens_the_protocol_hooks() {
    // `installMethods` keeps the protocol machinery and its install, so the
    // generic operations dispatch to program types.
    let units = units("(deftype T [] ICounted (-count [_] 7)) (count (T.))");
    for unit in ["installMethods", "publicMethod", "protocolHooks.install"] {
        assert!(units.contains(&unit), "{unit}: {units:?}");
    }
    every_level("(deftype T [] ICounted (-count [_] 7)) (count (T.))", "7");
}

#[test]
fn host_facing_methods_of_builtin_prototypes() {
    every_level(
        "(let [v [1 2 3 2]]
           (array (.indexOf v 2) (.lastIndexOf v 2) (.indexOf v 9) (.equiv v [1 2 3 2])
                  (.equiv v '(1 2 3 2)) (.equiv v [1 2]) (.call v nil 1) (.-length (js/Array.from v))
                  (.join (js/Array.from v) \"\")))",
        "[\n  1,    3,    -1,\n  true, true, false,\n  2,    4,    '1232'\n]",
    );
    every_level(
        "(let [v (vec (range 40))]
           (array (.indexOf v 33) (.join (.slice (js/Array.from v) 30 35) \",\")
                  (.equiv v (range 40)) (.equiv v (range 41))))",
        "[ 33, '30,31,32,33,34', true, false ]",
    );
    every_level(
        "(array (.equiv :a :a) (.equiv :a :b) (.call :a nil {:a 1}) (.call :b nil {:a 1} 5)
                (.call :a nil nil) (.call :a nil #js [1]) (.equiv 'x 'x) (.call 'x nil {'x 2}))",
        "[\n  true, false, 1,\n  5,    null,  null,\n  true, 2\n]",
    );
    every_level(
        "(let [m {:a 1 :b 2}]
           (array (.get m :a) (.get m :c 3) (.has m :b) (.has m :c) (.equiv m {:b 2 :a 1})
                  (.-length (js/Array.from (.keys m))) (.-length (js/Array.from (.values m)))
                  (.join (.map (js/Array.from (.entries m)) (fn [e] (aget e 1))) \"\")
                  (.call m nil :b) (.equiv m {:a 1})))",
        "[\n  1,     3,    true,\n  false, true, 0,\n  0,     '',   2,\n  false\n]",
    );
    every_level(
        "(let [s #{1 2}]
           (array (.has s 1) (.has s 3) (.equiv s #{2 1}) (.call s nil 2) (.call s nil 3 :none)
                  (.-length (js/Array.from s))))",
        "[ true, false, true, 2, KeywordValue {}, 2 ]",
    );
}

#[test]
fn each_kind_branch_of_the_generic_operations() {
    // seq, first, rest, next, count, nth and equality over every kind.
    every_level(
        "(let [kinds [[1 2 3] (vec (range 40)) '(1 2 3) (cons 0 [1 2]) (lazy-seq [1 2])
                      (range 3) (range 0.5 3) (seq (vec (range 40))) {:a 1} (into {} (map vector (range 20) (range 20)))
                      (sorted-map 2 :b 1 :a) #{1 2} (sorted-set 3 1 2) (repeat 3 :x) (take 3 (cycle [1 2]))
                      (take 3 (iterate inc 0)) (subvec [1 2 3 4] 1 3) (conj cljs.core.PersistentQueue.EMPTY 1 2)
                      (keys {:a 1 :b 2}) (vals {:a 1 :b 2}) (rseq [1 2 3]) (drop 1 (vec (range 40)))]]
           (array (pr-str (map first kinds)) (pr-str (map count kinds)) (pr-str (map (comp first rest) kinds))
                  (pr-str (map #(nth (seq %) 1 :none) kinds)) (pr-str (map #(= % (seq %)) kinds))
                  (pr-str (map #(= (hash %) (hash (into (empty %) %))) kinds))))",
        "[\n  '(1 0 1 0 1 0 0.5 0 [:a 1] [0 0] [1 :a] 1 1 :x 1 0 2 1 :a 1 3 1)',\n  '(3 40 3 3 2 3 3 40 1 20 2 2 3 3 3 3 2 2 2 2 3 39)',\n  '(2 1 2 1 2 1 1.5 1 nil [1 1] [2 :b] 2 2 :x 2 1 3 2 :b 2 2 2)',\n  '(2 1 2 1 2 1 1.5 1 :none [1 1] [2 :b] 2 2 :x 2 1 3 2 :b 2 2 2)',\n  '(true true true true true true true true false false false false false true true true true true true true true true)',\n  '(true true false false false false false false true true true true true true true false true true false false false false)'\n]",
    );
    // Equality across kinds and with host values.
    every_level(
        "(array (= [1 2] '(1 2)) (= [1 2] (lazy-seq [1 2])) (= (range 3) [0 1 2]) (= {:a 1} (hash-map :a 1))
                (= {:a 1} (sorted-map :a 1)) (= #{1} (sorted-set 1)) (= [1 2] #js [1 2]) (= :a :a) (= 'x 'x)
                (= [1 [2 {:a #{3}}]] [1 [2 {:a #{3}}]]) (= {:a 1} {:a 2}) (= #{1} #{2}) (= [] '()) (= {} []))",
        "[\n  true,  true,  true,\n  true,  true,  true,\n  false, true,  true,\n  true,  false, false,\n  true,  false\n]",
    );
}

#[test]
fn transients_chunks_and_entries_through_the_generic_operations() {
    every_level(
        "(let [tv (transient [1 2 3]) tm (transient {:a 1}) chunk (chunk-first (seq (vec (range 40))))]
           (array (nth tv 1) (count tv) (get tm :a) (count tm) (nth chunk 3) (count chunk)
                  (key (first {:a 1})) (val (first {:a 1})) (pr-str (keys {:a 1})) (pr-str (find {:a 1} :a))
                  (count (persistent! (reduce conj! (transient {}) (map vector (range 9) (range 9)))))))",
        "[ 2, 3, 1, 1, 3, 32, KeywordValue {}, 1, '(:a)', '[:a 1]', 9 ]",
    );
    // A projection of entries that are not map entries fails as `-key` does.
    every_level(
        "(try (first (keys [[1 2]])) (catch :default e (.-message e)))",
        "No protocol method IMapEntry/-key defined for this value",
    );
}

#[test]
fn seqs_of_host_iterables() {
    // An ES6IteratorSeq: ISeqable and ISeq, not ISequential, equal only to
    // itself, reprobing a nil tail.
    every_level(
        "(let [s (seq (js/Set. #js [1 2 3])) t (seq (.values #js [4 5]))]
           (array (first s) (count s) (nth s 2) (seq? s) (seqable? s) (sequential? s) (counted? s)
                  (= s [1 2 3]) (= s s) (pr-str s) (pr-str (rest t)) (pr-str (next (rest t)))
                  (identical? s (seq s)) (pr-str (cons 0 s)) (reduce + s)))",
        "[\n  1,        3,\n  3,        true,\n  true,     false,\n  false,    false,\n  true,     '#js {}',\n  '#js {}', 'nil',\n  true,     '(0 1 2 3)',\n  6\n]",
    );
    // Small programs keep the native path: no protocol machinery.
    every_level(
        "(let [s (seq (js/Set. #js [1 2 3]))] (array (first s) (first (rest s)) (count s)))",
        "[ 1, 2, 3 ]",
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(
            !units(
                "(let [s (seq (js/Set. #js [1 2 3]))] (array (first s) (first (rest s)) (count s)))"
            )
            .contains(&unit),
            "{unit}"
        );
    }
}

#[test]
fn protocol_extensions_of_builtin_and_host_types_reach_the_generic_operations() {
    every_level(
        "(extend-type PersistentVector ICounted (-count [_] 42))
         (extend-type object ISeqable (-seq [o] (list :obj)))
         (extend-type number IHash (-hash [n] 5))
         (array (count [1 2]) (pr-str (seq #js {})) (= (hash 1) (hash 2)) (count '(1 2)))",
        "[ 42, '(:obj)', false, 2 ]",
    );
    every_level(
        "(deftype Pair [a b] ISeqable (-seq [_] (list a b)) ICounted (-count [_] 2)
           IIndexed (-nth [_ i] (if (zero? i) a b)) (-nth [_ i d] (case i 0 a 1 b d)))
         (let [p (Pair. :x :y)] (array (pr-str (seq p)) (count p) (nth p 1) (pr-str (into [] p)) (= [:x :y] (vec p))))",
        "[ '(:x :y)', 2, KeywordValue {}, '[:x :y]', true ]",
    );
    every_level(
        "(def s (seq (js/Set. #js [1 2])))
         (specify! s ICounted (-count [_] 10))
         (array (count s) (first s) (seq? s))",
        "[ 10, 1, true ]",
    );
}

#[test]
fn conj_assoc_and_with_meta_over_every_kind() {
    every_level(
        "(defrecord R [a])
         (let [kinds [[1] (subvec [1 2 3] 1) (cljs.core/Subvec. nil [1 2 3] 0 2 nil) {:a 1} (sorted-map :a 1) #{1}
                      (sorted-set 1) (cljs.core/PersistentHashSet. nil {1 nil} nil) '(1) (cons 1 nil) (lazy-seq [1])
                      (conj cljs.core.PersistentQueue.EMPTY 1) (range 2) (seq [1 2]) nil (->R 1)]
               item #(if (or (map? %) (record? %)) [:k 9] 5)
               metas (map #(pr-str (meta (with-meta % {:m 1}))) (remove nil? kinds))]
           (array (pr-str (map #(conj % (item %)) kinds)) (pr-str (map #(count (conj % (item %) (item %))) kinds))
                  (pr-str metas)
                  (pr-str (map #(meta (with-meta % {:m 2})) [(range 2.5) (repeat 2 :x) (iterate inc 0) (cycle [1])
                                                               (rest (seq (vec (range 40)))) (chunk-cons (chunk-first (seq (vec (range 40)))) nil)]))
                  (pr-str (assoc [1 2] 2 3)) (pr-str (assoc (cljs.core/Subvec. nil [1 2 3] 0 2 nil) 0 :x))
                  (pr-str (assoc (cljs.core/PersistentArrayMap. nil 8 (to-array (range 16)) nil) :new 1))
                  (pr-str (assoc (->R 1) :a 2 :b 3)) (pr-str (assoc nil :a 1)) (pr-str (into {} [[:a 1] {:b 2}]))
                  (try (conj {} [1]) (catch :default e (.-message e)))))",
        "[\n  '([1 5] [2 3 5] [1 2 5] {:a 1, :k 9} {:a 1, :k 9} #{1 5} #{1 5} #{1 5} (5 1) (5 1) (5 1) #queue [1 5] (5 0 1) (5 1 2) (5) #cljs.user.R{:a 1, :k 9})',\n  '(3 4 4 2 2 2 2 2 3 3 3 3 4 4 2 2)',\n  '(\"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\" \"{:m 1}\")',\n  '({:m 2} {:m 2} {:m 2} {:m 2} {:m 2} {:m 2})',\n  '[1 2 3]',\n  '[:x 2]',\n  '{0 1, 2 3, 4 5, 6 7, 8 9, :new 1, 10 11, 12 13, 14 15}',\n  '#cljs.user.R{:a 2, :b 3}',\n  '{:a 1}',\n  '{:a 1, :b 2}',\n  'Index out of bounds'\n]",
    );
    every_level(
        "(let [f (with-meta (fn [& xs] (apply + xs)) {:m 1}) g (with-meta f {:m 2})]
           (array (f 1 2) (g 3 4) (pr-str (meta g)) (apply f 1 [2 3]) (apply + 1 2 [3 4 5])
                  (apply + (range 30)) (apply max (repeat 3 7)) (apply str (seq (js/Set. #js [\"a\" \"b\"])))))",
        "[ 3, 7, '{:m 2}', 6, 15, 435, 7, 'ab' ]",
    );
}

/// Drains a host iterator through `.next`, with the final `value`.
const DRAIN: &str = "(defn drain [it] (loop [acc []] (let [r (.next it)] (if (.-done r) (conj acc (.-value r)) (recur (conj acc (.-value r)))))))";

#[test]
fn vectors_answer_host_facing_methods_from_their_items() {
    // light-cljs-230: without protocol implementations a vector iterates,
    // counts, searches and compares its own items, as `seq` walks them,
    // including the tail its public constructor received and a subvec's
    // source.
    every_level(
        &format!(
            "{DRAIN}
             (let [pv (cljs.core/PersistentVector. nil 3 5 (js-obj) #js [1 2 3] nil)
                   odd (cljs.core/PersistentVector. nil 2 5 (js-obj) #js [7 8 9] nil)
                   none (cljs.core/PersistentVector. nil 0 5 (js-obj) #js [5] nil)
                   sv (subvec [1 2 3 4 5] 1 4) big (vec (range 40)) e (first {{:a 1}})
                   it (fn [v] (drain (.call (aget v js/Symbol.iterator) v)))]
               (pr-str [(it pv) (it odd) (it none) (it sv) (count (it big)) (it e) (it [])
                        (.indexOf pv 2) (.lastIndexOf odd 8) (.indexOf sv 4) (.lastIndexOf sv 2 -1)
                        (.indexOf e 1) (.indexOf big 33 -10) (.lastIndexOf [1 2 1] 1 js/undefined)
                        (.indexOf [1 2] 2 0.5) (.equiv pv [1 2 3]) (.equiv odd [7 8]) (.equiv odd odd)
                        (.equiv sv [2 3 4]) (.equiv e [:a 1]) (.equiv [] none) (= pv [1 2 3])
                        (= sv (subvec [0 2 3 4] 1)) (= [1 2] '(1 2)) (.-cnt pv) (.-cnt [1 2]) (.-cnt sv)]))"
        ),
        "[[1 2 3 nil] [7 8 9 nil] [nil] [2 3 4 nil] 41 [:a 1 nil] [nil] 1 1 2 0 1 33 2 1.5 true false true true true true true true true 3 2 nil]",
    );
}

#[test]
fn maps_answer_host_facing_methods_from_their_entries() {
    // Array, hash and sorted maps, a public constructor's array map and a
    // tree map, which takes the generic iteration.
    every_level(
        &format!(
            "{DRAIN}
             (let [it (fn [m] [(mapv vec (js/Array.from m)) (drain (.keys m)) (drain (.values m))
                               (mapv #(if (array? %) (vec %) %) (drain (.entries m)))])]
               (pr-str [(it {{:a 1 :b 2}}) (it (into {{}} (map vector (range 10) (range 10))))
                        (it (sorted-map 2 :b 1 :a)) (it (cljs.core/PersistentArrayMap. nil 2 #js [:x 1 :y 2] nil))
                        (it (cljs.core/PersistentTreeMap. compare nil 0 nil nil)) (it {{}})
                        (.equiv {{:a 1 :b 2}} {{:b 2 :a 1}}) (.-cnt {{:a 1}})
                        (.-cnt (cljs.core/PersistentArrayMap. nil 2 #js [:x 1 :y 2] nil))]))"
        ),
        "[[[[:a 1] [:b 2]] [:a :b nil] [1 2 nil] [[:a 1] [:b 2] nil]] [[[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9]] [0 1 2 3 4 5 6 7 8 9 nil] [0 1 2 3 4 5 6 7 8 9 nil] [[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil]] [[[1 :a] [2 :b]] [1 2 nil] [:a :b nil] [[1 :a] [2 :b] nil]] [[[:x 1] [:y 2]] [:x :y nil] [1 2 nil] [[:x 1] [:y 2] nil]] [[] [nil] [nil] [nil]] [[] [nil] [nil] [nil]] true 1 2]",
    );
    // With protocol implementations the generic methods dispatch to them,
    // and they take receivers of another type.
    every_level(
        &format!(
            "{DRAIN}
             (extend-type PersistentVector ICounted (-count [_] 7))
             (extend-type PersistentArrayMap ISeqable (-seq [_] (list (cljs.core/MapEntry. :z 26 nil))))
             (let [v [1 2 3] m {{:a 1}}]
               (pr-str [(drain (.call (aget v js/Symbol.iterator) v)) (try (.lastIndexOf v 3) (catch :default e (.-message e)))
                        (.-cnt v) (.equiv v [1 2 3]) (mapv vec (js/Array.from m)) (drain (.keys m)) (.-cnt m)
                        (drain (.call (aget v js/Symbol.iterator) '(4 5))) (.call (.-indexOf v) '(4 5) 5)]))"
        ),
        "[[1 2 3 nil] \"Index out of bounds\" 7 true [[:z 26]] [:z nil] 1 [4 5 nil] 1]",
    );
}

#[test]
fn sets_answer_host_facing_methods_from_their_members() {
    // light-cljs-231: without protocol implementations a set iterates
    // (`keys`, `values`, `entries`) and searches (`has`) its own members in
    // the order `seq` walks them, compares its count without `count`, and
    // builds its `hash-map` field as a map of its kind: array, hash and
    // sorted sets, sets the public constructors built from maps of every
    // kind and from other values, and quoted literals.
    every_level(
        &[
            DRAIN,
            r#"
             (let [it (fn [s] [(vec (js/Array.from s)) (drain (.keys s)) (drain (.values s))
                               (mapv #(if (array? %) (vec %) %) (drain (.entries s)))
                               (.has s 1) (.has s 7)])
                   big (into #{} (range 10))
                   pub (cljs.core/PersistentHashSet. nil {1 nil :a nil} nil)
                   pubh (cljs.core/PersistentHashSet. nil (into {} (map vector (range 10) (range 10))) nil)
                   tree (cljs.core/PersistentTreeSet. nil (sorted-map 3 nil 1 nil) nil)
                   vsrc (cljs.core/PersistentHashSet. nil [1 2 3] nil)
                   sorted (cljs.core/PersistentHashSet. nil (sorted-map 1 nil 2 nil) nil)
                   none (cljs.core/PersistentHashSet. nil nil nil)]
               (pr-str [(it #{1 :a}) (it big) (it (sorted-set 3 1 2)) (it pub) (it pubh) (it tree) (it #{})
                        (it #{nil 1}) (it (disj big 3)) (it (conj #{1} 2)) (it '#{1 "a"})
                        (.equiv #{1 2} #{2 1}) (.equiv #{1 2} #{1}) (= #{1 2} pub) (= pub #{1 :a})
                        (= big (set (range 10))) (= vsrc #{}) (= #{} vsrc) (= tree #{1 3}) (= #{1 3} tree)
                        (count vsrc) (.-hash-map #{1 :a}) (.-hash-map big) (.-hash-map pub)
                        (.-hash-map (persistent! (transient pub))) (.call #{1 2} nil 2) (.call #{1 2} nil 3 :none)
                        (contains? sorted 1) (contains? sorted 3) (count sorted) (conj sorted 5) (disj sorted 1)
                        (contains? none 1) (conj none 1) (persistent! (conj! (transient none) 3))
                        (contains? (persistent! (conj! (transient none) 3)) 3)]))"#,
        ]
        .concat(),
        "[[[1 :a] [1 :a nil] [1 :a nil] [[1 1] [:a :a] nil] true false] \
         [[0 1 2 3 4 5 6 7 8 9] [0 1 2 3 4 5 6 7 8 9 nil] [0 1 2 3 4 5 6 7 8 9 nil] \
         [[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil] true true] \
         [[1 2 3] [1 2 3 nil] [1 2 3 nil] [[1 1] [2 2] [3 3] nil] true false] \
         [[1 :a] [1 :a nil] [1 :a nil] [[1 1] [:a :a] nil] true false] \
         [[0 1 2 3 4 5 6 7 8 9] [0 1 2 3 4 5 6 7 8 9 nil] [0 1 2 3 4 5 6 7 8 9 nil] \
         [[0 0] [1 1] [2 2] [3 3] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil] true true] \
         [[1 3] [1 3 nil] [1 3 nil] [[1 1] [3 3] nil] true false] [[] [nil] [nil] [nil] false false] \
         [[nil 1] [nil 1 nil] [nil 1 nil] [[nil nil] [1 1] nil] true false] \
         [[0 1 2 4 5 6 7 8 9] [0 1 2 4 5 6 7 8 9 nil] [0 1 2 4 5 6 7 8 9 nil] \
         [[0 0] [1 1] [2 2] [4 4] [5 5] [6 6] [7 7] [8 8] [9 9] nil] true true] \
         [[1 2] [1 2 nil] [1 2 nil] [[1 1] [2 2] nil] true false] \
         [[1 \"a\"] [1 \"a\" nil] [1 \"a\" nil] [[1 1] [\"a\" \"a\"] nil] true false] \
         true false false true true true false true true 3 {1 nil, :a nil} \
         {0 nil, 1 nil, 2 nil, 3 nil, 4 nil, 5 nil, 6 nil, 7 nil, 8 nil, 9 nil} {1 nil, :a nil} \
         {1 nil, :a nil} 2 :none true false 2 #{1 2 5} #{2} false #{1} #{3} true]",
    );
    // With protocol implementations the generic methods dispatch to them,
    // and they take receivers of another type.
    every_level(
        &[
            DRAIN,
            r#"
             (extend-type PersistentHashSet ICounted (-count [_] 7))
             (extend-type PersistentTreeSet ISeqable (-seq [_] (list 9 8)))
             (let [s #{1 2} t (sorted-set 1 2)]
               (pr-str [(drain (.call (aget s js/Symbol.iterator) s)) (drain (.keys t)) (vec (js/Array.from t))
                        (mapv vec (drain (.entries s))) (.has s 1) (.has t 9) (= s (hash-set 1 2))
                        (= (hash-set 2 1) s) (= s (conj #{1} 2)) (count s) (drain (.call (.-keys s) '(4 5)))
                        (.call (.-has s) {:a 1} :a) (mapv vec (drain (.call (.-entries s) [6 7])))]))"#,
        ]
        .concat(),
        "[[1 2 nil] [9 8 nil] [9 8] [[1 1] [2 2] []] true false false false false 7 [4 5 nil] true \
         [[6 6] [7 7] []]]",
    );
}

/// A set literal's `has` and iteration through the host.
const SET_SOURCE: &str = "(js/console.log (.has #{1 2} 2) (.join (js/Array.from #{1 2}) \" \"))";

#[test]
fn a_set_keeps_no_seq_hash_map_or_hashing() {
    // light-cljs-231: it iterates and searches its members itself, compares
    // them without `count`, builds an array map for its `hash-map` field, and
    // its literal is an array set, which no promotion makes a hash set, so it
    // keeps neither seqs, nor a hash map, nor hashing.
    let kept = assert_keeps_none(
        SET_SOURCE,
        &[
            "seq",
            "first",
            "next",
            "count",
            "nth",
            "es6Iterator",
            "es6EntriesIterator",
            "makeSet.seq",
            "IndexedSeqValue",
            "mapFromArray",
            "PersistentHashMapValue",
            "hash",
            "hashNodeAssoc",
            "ensureHashRoot",
            "PersistentHashMapValue.traversal",
            "PersistentHashMapValue.entryIndex",
            "PersistentHashMapValue.put",
            "putMember",
            "setFromArray",
            "publicHashSet",
        ],
    );
    for unit in PROTOCOL_MACHINERY {
        assert!(!kept.contains(&unit), "{unit}: {kept:?}");
    }
    for unit in [
        "arraySetFromArray",
        "setIterator",
        "makeSet.equiv",
        "setCount",
    ] {
        assert!(kept.contains(&unit), "{unit}: {kept:?}");
    }
    every_level(SET_SOURCE, "true 1 2\nundefined");
    // A program naming the type keeps its public constructor, and a larger
    // literal keeps promotion and hashing.
    for source in [
        "(js/console.log (cljs.core/PersistentHashSet. nil {1 nil} nil))",
        "(js/console.log (instance? PersistentHashSet #{1}))",
        "(js/console.log #{1 2 3 4 5 6 7 8 9})",
    ] {
        let units = units(source);
        assert!(units.contains(&"hash"), "{source}: {units:?}");
    }
    assert!(!units("(js/console.log #{} (.-EMPTY (type #{})))").contains(&"publicHashSet"));
}

#[test]
fn membership_in_value_position_keeps_no_set() {
    // light-cljs-231: `(contains? s x)`, `(s x)` and `(get s x)` on a set
    // literal of at most 8 keywords, strings or numbers compare the value
    // with its members wherever the result goes, so no set is built.
    let source = r#"
      (def ^:const modes #{:fast :safe})
      (defn kw [k] [(contains? #{:fast :safe :a/b} k) (#{:fast :safe :a/b} k) (get #{:fast :safe :a/b} k)
                    (contains? modes k) (modes k) (let [s #{:x}] (s k)) (contains? #{} k) (get #{:only} k)
                    (if (contains? #{:fast :safe} k) :in :out) (if (#{:fast} k) 1 2)
                    (and (contains? #{:fast :safe} k) :both)])
      (defn st [s] [(contains? #{"fast" "safe"} s) (#{"fast" "safe"} s) (get #{"fast" "safe"} s)
                    (if (contains? #{"fast" "safe"} s) :in :out) (contains? #{"x"} s)])
      (defn nm [n] [(contains? #{1 2 -1 1.5} n) (#{1 2 -1 1.5} n) (get #{0} n) (contains? #{##NaN} n)
                    (if (#{1 2} n) :in :out) (contains? #{1 "1"} n) (#{1 "1"} n)])
      (def calls (atom 0))
      (defn tick [x] (swap! calls inc) x)
      (set! (.-kw js/globalThis) kw)
      (set! (.-st js/globalThis) st)
      (set! (.-nm js/globalThis) nm)
      (pr-str [(map js/globalThis.kw [:fast :safe :slow :a/b :b "fast" 'fast nil (keyword "fast")
                                      (keyword "a" "b") {:fast 1} [:fast] :x :only])
               (map js/globalThis.st ["fast" "safe" "slow" :fast nil 1 "x"])
               (map js/globalThis.nm [1 2 -1 1.5 3 0 -0 ##NaN "1" nil :a])
               (contains? #{:a :b} (tick :a)) (#{1 2} (tick 2)) (get #{"a" "b"} (tick "c")) @calls
               (let [m (fn [x] (#{:a :b :c} (tick x)))] [(m :b) (m :d)]) @calls])"#;
    every_level(
        source,
        "[([true :fast :fast true :fast nil false nil :in 1 :both] \
         [true :safe :safe true :safe nil false nil :in 2 :both] [false nil nil false nil nil false nil :out 2 false] \
         [true :a/b :a/b false nil nil false nil :out 2 false] [false nil nil false nil nil false nil :out 2 false] \
         [false nil nil false nil nil false nil :out 2 false] [false nil nil false nil nil false nil :out 2 false] \
         [false nil nil false nil nil false nil :out 2 false] [true :fast :fast true :fast nil false nil :in 1 :both] \
         [true :a/b :a/b false nil nil false nil :out 2 false] [false nil nil false nil nil false nil :out 2 false] \
         [false nil nil false nil nil false nil :out 2 false] [false nil nil false nil :x false nil :out 2 false] \
         [false nil nil false nil nil false :only :out 2 false]) \
         ([true \"fast\" \"fast\" :in false] [true \"safe\" \"safe\" :in false] [false nil nil :out false] \
         [false nil nil :out false] [false nil nil :out false] [false nil nil :out false] [false nil nil :out true]) \
         ([true 1 nil false :in true 1] [true 2 nil false :in false nil] [true -1 nil false :out false nil] \
         [true 1.5 nil false :out false nil] [false nil nil false :out false nil] [false nil 0 false :out false nil] \
         [false nil 0 false :out false nil] [false nil nil false :out false nil] [false nil nil false :out true \"1\"] \
         [false nil nil false :out false nil] [false nil nil false :out false nil]) true 2 nil 3 [:b nil] 5]",
    );
    let units = assert_keeps_none(
        "(set! (.-f js/globalThis) (fn [k] [(contains? #{:fast :safe} k) (#{\"a\" \"b\"} k) (get #{1 2} k)]))",
        &[
            "makeSet",
            "PersistentHashSetValue",
            "contains",
            "get",
            "lookup",
        ],
    );
    assert!(units.contains(&"keywordFqn"), "{units:?}");
}

#[test]
fn small_programs_keep_only_what_they_use() {
    // A vector, a map literal, `str` of scalars, an atom without options and
    // the printer without options keep neither seqs nor hash maps nor the
    // protocol machinery (light-cljs-230).
    let absent = |source: &str, extra: &[&str]| {
        let units = assert_keeps_none(source, extra);
        for unit in PROTOCOL_MACHINERY {
            assert!(!units.contains(&unit), "{source} keeps `{unit}`");
        }
        units
    };
    absent(
        "(js/console.log {:a 1})",
        &["seq", "PersistentHashMapValue", "hash", "mapFromArray"],
    );
    absent(
        "(js/console.log (str (js/Date.now) :a))",
        &["print", "printWriterImpl", "makeVector"],
    );
    absent("(js/console.log (atom 1))", &["seqToMap", "into", "seq"]);
    absent(
        "(js/console.log (pr-str 1))",
        &["assoc", "PersistentHashMapValue", "printOptions.writer"],
    );
    absent(
        "(js/console.log (into [] (range 3)) (reduce + [1 2]) (seq? (list 1)) (coll? [1]))",
        &[
            "PersistentHashMapValue",
            "makeMap.persist",
            "makeSet.persist",
        ],
    );
}

#[test]
fn printing_with_options_dispatchers_and_into() {
    every_level(
        "(let [alt (fn [x w opts] (if (number? x) (-write w (str \"#\" x)) ((:fallback-impl opts) x w opts)))
               parts #js [] w (StringBufferWriter. (js-obj \"append\" (fn [t] (.push parts t) nil)))]
           (pr-sequential-writer w (fn [x w o] (-write w (pr-str x))) \"<\" \" \" \">\" {:print-length 2 :more-marker \"~\"} [1 2 3])
           (-flush w)
           (pr-str [(str 1 :a \"b\" nil [1 2] {:a 1} #{2} '(3) (range 2))
                    (pr-str-with-opts [[1 {:a 2}] \"x\"] {:alt-impl alt})
                    (prn-str-with-opts [[1]] {:print-length 1})
                    (print-meta? {:meta true} (with-meta [1] {:m 1}))
                    (pr-str-with-opts [(with-meta [1] {:m 1})] {:meta true})
                    (.join parts \"\") (satisfies? IWriter w) (implements? IWriter w)
                    (pr-str (atom 1 :meta {:a 1})) (meta (atom 1 :meta {:a 1})) (meta (atom 1 {:meta {:b 2}}))
                    (pr-str (into [] (range 3)) (into {} [[:a 1]]) (into #{} [1]) (into () [1 2])
                            (into (sorted-map) {:b 1 :a 2}) (into (with-meta [] {:m 1}) [1]))
                    (reduce + [1 2 3]) (reduce + (js/Set. #js [1 2])) (seq? '(1)) (coll? [1])
                    (counted? nil) (empty? []) (-conj [1] 2) (-count nil) (-lookup {:a 1} :a)
                    (try (-count 1) (catch :default e (.-message e)))]))",
        "[\"1:ab[1 2]{:a 1}#{2}(3)(0 1)\" \"[#1 {:a #2}] x\" \"[1]\\n\" true \"^{:m 1} [1]\" \"<1 2 ~>\" true true \"#object[cljs.core.Atom {:val 1}]\" {:a 1} {:b 2} \"[0 1 2] {:a 1} #{1} (2 1) {:a 2, :b 1} [1]\" 6 3 true true true true [1 2] 0 1 \"No protocol method ICounted/-count defined for this value\"]",
    );
}

#[test]
fn metadata_through_the_generic_operations() {
    every_level(
        "(defn ^{:test (fn [] :ok)} f [] 1)
         (array (pr-str (meta (with-meta [1] {:a 1}))) (some? (:test (meta #'f))) (.-meta (with-meta [1] {:a 2}))
                (pr-str (meta (conj (with-meta [1] {:b 1}) 2))) (nil? (.-meta [1])))",
        "[ '{:a 1}', true, PersistentArrayMapValue {}, '{:b 1}', true ]",
    );
}

/// Each row of a fixture corpus (`id<TAB>source<TAB>...`) prints, with
/// `pr-str` or as an error message, what it prints at `None`.
fn corpus(fixture: &str, range: std::ops::Range<usize>) {
    let rows: Vec<(&str, &str)> = fixture
        .lines()
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .map(|line| {
            let mut columns = line.split('\t');
            (columns.next().unwrap(), columns.next().unwrap())
        })
        .collect();
    for (id, source) in &rows[rows.len().min(range.start)..rows.len().min(range.end)] {
        let wrapped = format!(
            "(try (pr-str (do {source})) (catch :default e (str \"error: \" (.-message e))))"
        );
        let program = analyze(&wrapped, &parse(&wrapped).unwrap())
            .unwrap_or_else(|error| panic!("{id}: {error}"));
        let outputs = OptimizationLevel::ALL.map(|level| {
            let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
            execute(&format!("console.log({js});"))
        });
        for (level, output) in OptimizationLevel::ALL.iter().zip(&outputs).skip(1) {
            assert_eq!(output, &outputs[0], "{id} at {level}");
        }
    }
}

const EQUALITY: &str = include_str!("../fixtures/compiler-equality.tsv");
const SEQUENCE_CONTRACTS: &str = include_str!("../fixtures/compiler-sequence-contracts.tsv");
const SEQUENCE_COMPLETION: &str = include_str!("../fixtures/compiler-sequence-completion.tsv");
const TRANSIENTS: &str = include_str!("../fixtures/compiler-transients.tsv");
const SORTED: &str = include_str!("../fixtures/compiler-sorted-collections.tsv");
const OBJECT_EQUIV: &str = include_str!("../fixtures/compiler_object_equiv.tsv");
const SEQUENCE_OBJECT_EQUIV: &str = include_str!("../fixtures/compiler_sequence_object_equiv.tsv");
const REDUCE_KV: &str = include_str!("../fixtures/compiler-reduce-kv-entries.tsv");
const HASH_BRANCHES: &str = include_str!("../fixtures/compiler-hash-branch-effects.tsv");
const FIND_EFFECTS: &str = include_str!("../fixtures/compiler-find-effects.tsv");
const PUBLIC_CONSTRUCTORS: &str = include_str!("../fixtures/compiler-public-constructors.tsv");
const CORE_HELPERS: &str = include_str!("../fixtures/compiler-core-helpers.tsv");
const LANGUAGE: &str = include_str!("../fixtures/compiler-language.tsv");

row_tests!(corpus;
    equality_corpus(EQUALITY, 0..100),
    sequence_contracts_corpus(SEQUENCE_CONTRACTS, 0..100),
    sequence_completion_corpus(SEQUENCE_COMPLETION, 0..100),
    transients_corpus(TRANSIENTS, 0..100),
    sorted_collections_corpus(SORTED, 0..100),
    object_equiv_corpus(OBJECT_EQUIV, 0..100),
    sequence_object_equiv_corpus(SEQUENCE_OBJECT_EQUIV, 0..100),
    reduce_kv_corpus(REDUCE_KV, 0..100),
    hash_branches_corpus_00(HASH_BRANCHES, 0..35),
    hash_branches_corpus_35(HASH_BRANCHES, 35..100),
    find_effects_corpus(FIND_EFFECTS, 0..100),
    public_constructors_corpus(PUBLIC_CONSTRUCTORS, 0..100),
    core_helpers_corpus(CORE_HELPERS, 0..100),
    language_corpus_000(LANGUAGE, 0..100),
    language_corpus_100(LANGUAGE, 100..200),
    language_corpus_200(LANGUAGE, 200..300),
    language_corpus_300(LANGUAGE, 300..400),
    language_corpus_400(LANGUAGE, 400..500),
    language_corpus_500(LANGUAGE, 500..600),
    language_corpus_600(LANGUAGE, 600..700),
    language_corpus_700(LANGUAGE, 700..800),
);
