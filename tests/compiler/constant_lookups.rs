//! Folding lookups, counts and destructuring against pooled constants
//! (light-cljs-221; docs/advanced-optimization.md, "Constant collection
//! literals", tier 5).
//!
//! At `Advanced` the folder (`src/folding.rs`) evaluates `get`, keyword and
//! collection calls, `get-in`, `nth`, `count`, `empty?`, `contains?`, `seq`,
//! `first`, `rest` and the type predicates on constant collections, including
//! the locals and `^:const` globals bound to one, so destructuring a constant
//! binds its locals statically. A call that would throw stays and throws.
//! Lookups in maps bound to locals whose keys are constants read their entry
//! by index, or fold when the key is absent; an `if` on membership in a small
//! set of keywords is a `switch` on the tested value's spelling.
//!
//! Every program runs at every level in Node and in QuickJS and must give
//! what `None` gives; the `Advanced` text is checked for what folded and what
//! stayed.
use super::optimization::{execute, readable};
use light_cljs::evaluator::{Evaluator, EvaluatorOptions};
use light_cljs::{
    ApplicationOptions, OptimizationLevel, Program, analyze, parse, prepare_application,
};
use std::time::Duration;

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn emit(program: &Program, level: OptimizationLevel) -> String {
    prepare_application(program, ApplicationOptions::new(level)).emit_js()
}

/// The value of the JavaScript string expression `expression` in a fresh
/// QuickJS evaluator.
fn quickjs(expression: &str) -> String {
    let evaluator = Evaluator::new(EvaluatorOptions {
        memory_limit: 1 << 30,
        timeout: Duration::from_secs(60),
        ..EvaluatorOptions::default()
    })
    .unwrap();
    let value = evaluator
        .evaluate_js(expression)
        .unwrap_or_else(|error| panic!("{error:?}"));
    evaluator.string(value).unwrap()
}

/// The program, whose value is a string, gives `expected` at every level in
/// Node and in QuickJS; returns the program part of the readable `Advanced`
/// output (what follows the runtime slice and the constant table).
fn every_level(source: &str, expected: &str) -> String {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let js = emit(&program, level);
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n"),
            "Node at {level}: {source}"
        );
        assert_eq!(quickjs(&js), expected, "QuickJS at {level}: {source}");
    }
    let advanced = prepare_application(&program, readable(OptimizationLevel::Advanced)).emit_js();
    let start = advanced
        .find("  const host = ")
        .or_else(|| advanced.rfind("\n  const k"))
        .unwrap_or(0);
    advanced[start..].to_owned()
}

/// Runtime operations that a folded site no longer calls.
const LOOKUPS: [&str; 14] = [
    "rt$get(",
    "rt$getIn(",
    "rt$nth(",
    "rt$vectorNth(",
    "rt$count(",
    "rt$isEmpty(",
    "rt$contains(",
    "rt$seq(",
    "rt$first(",
    "rt$rest(",
    "rt$next(",
    "rt$isSeq(",
    "rt$notEmpty(",
    ".call(null, ",
];

fn assert_folded(text: &str) {
    for call in LOOKUPS {
        assert!(!text.contains(call), "{call} stayed:\n{text}");
    }
}

/// Lookups, counts, items and predicates of constant collections are their
/// runtime results: missing keys and defaults, `nil`, string, symbol,
/// number, boolean and namespaced keyword keys, a positional map whose keys
/// are reader-distinct but `=` (first match), composite keys that are never
/// `=` to a scalar, truncated and negative vector indexes, list indexes that
/// must be exact, out-of-range `nth` with a default, a vector called with an
/// ignored argument, counts of merging and positional collections, and
/// metadata on the collection. A default with an effect runs once, whatever
/// the result. Nothing of it is left to the runtime.
#[test]
fn lookups_of_constants_fold_to_the_runtime_results() {
    let source = r#"
      (def log (atom []))
      (defn note [x] (swap! log conj x) x)
      (def ^:const C {:a 1 :b [2 3] :m ^{:k 1} [4] nil :nil "s" :str 'sym :sym 0 :zero :ns/k :nsk true :t})
      (pr-str
       [(get C :a) (get C :z) (get C :z 9) (get C nil) (get C "s") (get C 'sym) (get C 0) (get C :ns/k)
        (get C :k) (get C true) (get C 1) (:b C) (:z C :d) (C :a) (C :z :d) ('sym C) (:a nil)
        (get nil :a :d) (:a 5) (get "abc" :a :d) (get-in nil [:a])
        (get C :a (note 5)) (get C :z (note 6)) (:a C (note 7)) (C :q (note 8)) (nth [1 2] 0 (note 9))
        (nth [1 2] 4 (note 10))
        (get-in C [:b 1]) (get-in C [:b 5] :nf) (get-in C [:z :y]) (get-in C [:a :q] :nf) (get-in C [:a :q])
        (get-in C [] :nf) (get-in [[1 [2]]] '(0 1 0))
        (nth [1 2 3] 1) (nth [1 2 3] 1.5) (nth [1 2 3] -0.0) (nth [1 2 3] 5 :d) (nth [1 2 3] -1 :d)
        (nth [1 2 3] ##NaN :d) (nth [1 2 3] ##Inf :d) (nth '(1 2 3) 2) (nth '(1 2 3) 1.5 :d)
        (nth '(1 2 3) -0.0) (nth '(1 2 3) 3 :d) (nth '(1 2 3) -1 :d) (nth [] 0 :d) (nth () 0 :d)
        ([1 2 3] 1.9) ([1 2 3] 0 :ignored) (#{:a :b} :a) (#{:a :b} :c) (#{:a :b} :c :d) (get #{1 2} 2)
        (get #{1 2} 3) (get #{'a "b"} "b") (get #{'a "b"} 'a) (get {1 :one 1.0 :onef} 1)
        (get {[1] :a '(1) :b} :x) (get [10 20] 1) (get [10 20] 2) (get [10 20] :a) (get '(1 2) 0)
        (contains? [1 2] 1) (contains? [1 2] 1.5) (contains? [1 2] 2) (contains? [1 2] -0.0)
        (contains? [1 2] "1") (contains? #{:a} :a) (contains? #{:a} :b) (contains? C nil) (contains? {} nil)
        (count [1 2]) (count '(1 2 3)) (count #{1 2}) (count {}) (count []) (count nil) (count "abc")
        (count ^:m [1]) (count {:a 1 :b 2}) (count {1 :one 1.0 :onef}) (count '#{1 1.0})
        (empty? []) (empty? [1]) (empty? '()) (empty? #{}) (empty? nil) (empty? "") (empty? {:a 1})
        (seq []) (seq '()) (seq '(1 2)) (seq {}) (not-empty []) (not-empty [1 2]) (not-empty {:a 1})
        (first [1 2]) (first []) (first '(3 4)) (first '()) (second [1 2]) (second '(1)) (last [1 2 3])
        (last '(1 2)) (last []) (peek [1 2]) (peek '(1 2)) (peek [])
        (rest [1]) (rest []) (rest '(1 2 3)) (rest '(1)) (rest ()) (next [1]) (next '(1 2)) (next '(1))
        (next []) (next ())
        (seq? {:a 1}) (seq? '(1)) (seq? '()) (vector? [1]) (map? {:a 1}) (set? #{1}) (list? '(1))
        (list? '()) (coll? []) (sequential? '(1)) (sequential? {}) (counted? #{}) (associative? [1])
        (associative? '(1)) (seqable? {}) (symbol? 'a) (nil? [1]) (some? {}) (not [1]) (boolean #{})
        (map? ^{:m 1} {:a 1}) (count ^{:m 1} #{:a}) (= 'a 'a) (= 'a 'b)
        @log])"#;
    let text = every_level(
        source,
        r#"[1 nil 9 :nil :str :sym :zero :nsk nil :t nil [2 3] :d 1 :d :sym nil :d nil :d nil 1 6 1 8 1 10 3 :nf nil :nf nil {nil :nil, 0 :zero, "s" :str, :m [4], :ns/k :nsk, true :t, sym :sym, :b [2 3], :a 1} 2 2 2 1 :d :d :d :d 3 :d 1 :d :d :d :d 2 1 :a nil :d 2 nil "b" a :one nil 20 nil nil nil true false false true false true false true false 2 3 2 0 0 0 3 1 2 2 2 true false true true true true false nil nil (1 2) nil nil [1 2] {:a 1} 1 nil 3 nil 2 nil 3 2 nil 2 1 nil () () (2 3) () () nil (2) nil nil nil false true true true true true true true true true false true true false true true false true false true true 1 true false [5 6 7 8 9 10]]"#,
    );
    assert_folded(&text);
    // The defaults with effects run before the value they do not give.
    assert!(
        text.contains("(g1(5), 1)") && text.contains("g1(6)"),
        "{text}"
    );
}

/// What may throw, or what the folder cannot decide, stays a call and
/// behaves as below `Advanced`: `nth` out of range or with an index that is
/// not a number or on a map or set, a vector called out of range, with a
/// keyword or with no index, a map called with too many arguments; keys that
/// are `NaN` or zeros of the other sign, a collection key, and string
/// receivers whose characters a number or numeric string key reads.
#[test]
fn calls_that_may_throw_or_are_undecided_stay() {
    let source = r#"
      (defn attempt [f] (try (f) (catch :default e (str "threw: " (.-message e)))))
      (pr-str
       [(attempt #(nth [1 2 3] 5)) (attempt #(nth [1 2 3] "a")) (attempt #(nth {:a 1} 0))
        (attempt #(nth #{1} 0)) (attempt #([1 2] 5)) (attempt #([1 2] :a)) (attempt #([1 2]))
        (attempt #(nth '(1 2) 5)) (attempt #(nth '(1 2) -1)) (attempt #({:a 1} :a :b :c))
        (get {0 :zero} -0.0) (get {##NaN :nan} ##NaN :d) (count {0 :a 1 :b}) (get {:a 1} [1] :d)
        (get {[1] :v} [1]) (count {[1] :a '(1) :b}) (get {[1] :a '(1) :b} '(1)) (get "abc" 1)
        (get-in {:s "abc"} [:s 1]) (get "abc" "1") (= [1] '(1))])"#;
    let text = every_level(
        source,
        r#"["threw: Index out of bounds" "threw: Index argument to nth must be a number" "threw: nth not supported on this type" "threw: nth not supported on this type" "threw: Index out of bounds" "threw: Key must be integer" "threw: Key must be integer" "threw: Index out of bounds" "threw: Index out of bounds" "threw: Invalid arity: 3" :zero :d 2 :d :v 1 :b "b" "b" "b" true]"#,
    );
    for call in [
        "rt$vectorNth(rt$vectors.get(k0), 5, rt$missing)",
        "rt$nth(k0, \"a\")",
        "rt$nth(k2, 0)",
        "rt$nth(k3, 0)",
        "(k4).call(null, 5)",
        "(k4).call(null, k1)",
        "(k4).call(null)",
        "rt$nth(k5, 5)",
        "rt$nth(k5, -1)",
        ".call(null, k1, k6, k7)",
        ", -0)",
        ", NaN, k",
        "rt$count(k",
        "rt$get(\"abc\", 1)",
        "rt$getIn(",
        "rt$get(\"abc\", \"1\")",
    ] {
        assert!(text.contains(call), "{call}:\n{text}");
    }
}

/// Destructuring a constant binds its locals statically: vectors with
/// nested vectors and missing items, a quoted list, maps with `:keys`,
/// `:strs`, `:syms`, namespaced keys, `:or` defaults of present and absent
/// keys, `:as`, nested maps and a `^:const` global. The map destructuring's
/// `seq?` test folds, and so does every `nth` and `get` it expands to; only
/// the `:as` locals keep their constants.
#[test]
fn destructuring_constants_binds_locals_statically() {
    let source = r#"
      (def ^:const POINT {:px 1 :py 2})
      (pr-str
        (let [{:keys [a b] :as all} {:a 1 :b 2 :c 3}
              {:strs [s t] :or {s 7 t 8}} {"t" 1}
              {:syms [q] :or {q :none}} {'q 5}
              {:keys [ns/k]} {:ns/k :nsk}
              {:keys [missing] :or {missing :default}} {:present 1}
              [x [y z] w] [10 [20 30]]
              [p r] '(1 2)
              {{:keys [n]} :inner [i j] :pair} {:inner {:n 99} :pair [3 4]}
              {:keys [px py]} POINT
              [[e f] :as pair] [[5 6]]]
          [a b all s t q k missing x y z w p r n i j px py e f pair]))"#;
    let text = every_level(
        source,
        "[1 2 {:a 1, :b 2, :c 3} 7 1 5 :nsk :default 10 20 30 nil 1 2 99 3 4 1 2 5 6 [[5 6]]]",
    );
    assert_folded(&text);
    // Only the `:as` values are read: the map, and the vector of the pair.
    assert_eq!(text.matches(" = k").count(), 2, "{text}");
}

/// A folded item of a constant is the pool entry the constant holds it as,
/// with its metadata: `identical?` holds between two lookups of it, and
/// between a list's rest and the rest the runtime gives. An item of a
/// literal that is built at each evaluation, because folding made its items
/// constant, is not copied, and an inlined function's argument of that kind
/// is one value (light-cljs-214's inliner reads it once). A `^:const`
/// collection folds only where it is read after its definition.
#[test]
fn folded_items_keep_identity_and_metadata() {
    let source = r#"
      (def ^:const X 1)
      (def ^:const C {:v [1 2] :m ^{:k 1} [4] :l '(1 2 3)})
      (defn same [v] (identical? v v))
      (declare K)
      (defn early [] (get K :a))
      (def e (early))
      (def ^:const K {:a 1 :b [2]})
      (defn late [] (get K :a))
      (pr-str
        (let [c [[X] 2] d [[1] 2] items [^{:a 1} [1] '^{:b 2} (1)]]
          [(identical? (get C :v) (get C :v)) (identical? (nth c 0) (nth c 0))
           (identical? (nth d 0) (nth d 0)) (meta (get C :m)) (meta (nth items 0))
           (meta (rest '(1 2))) (= (rest (:l C)) '(2 3)) (list? (rest (:l C)))
           (identical? (rest (:l C)) (rest (:l C))) (identical? (seq (:l C)) (:l C))
           (same [X 2]) (same {:a X}) (same [[X]]) e (late) (get K :a) (count K)
           (identical? (:b K) (:b K))]))"#;
    let text = every_level(
        source,
        "[true true true {:k 1} {:a 1} nil true true true true true true true nil 1 1 2 true]",
    );
    // The built vector's item is read from it, twice.
    assert_eq!(
        text.matches("rt$vectorNth(rt$vectors.get(l").count(),
        2,
        "{text}"
    );
    // `early` reads `K` before its definition runs.
    assert!(text.contains("rt$get(g"), "{text}");
}

/// Folding needs the runtime's own operations: with a core protocol extended
/// to a runtime type nothing folds, and an operation the program replaces
/// stays a call to the replacement.
#[test]
fn folds_need_the_runtime_operations() {
    let extended = r#"
      (extend-type PersistentVector
        ICounted (-count [_] 42)
        ILookup (-lookup ([_ k] [:looked k]) ([_ k nf] [:looked k nf])))
      (def ^:const C [1 2 3])
      (pr-str [(count [1 2]) (count C) (get [1 2] 0) (get C 5 :nf) (nth [1 2] 1) (:a {:a 1})
               (count {:a 1})])"#;
    let text = every_level(extended, "[42 42 [:looked 0] [:looked 5 :nf] 2 1 1]");
    assert!(
        text.contains("rt$count(") && text.contains("rt$get("),
        "{text}"
    );
    let replaced = r#"
      (set! cljs.core/first (fn [_] :replaced))
      (pr-str [(first [1 2]) (second [1 2]) (count [1 2])])"#;
    let text = every_level(replaced, "[:replaced 2 2]");
    assert!(text.contains("(core$first).call(null, k"), "{text}");
}

/// An `if` on membership in a set literal of at most 8 keywords, `(s x)`,
/// `(contains? s x)` or `(get s x)`, also through a local, is a `switch` on
/// the tested value's spelling: keywords made at run time are members,
/// strings, symbols, collections, instances and lazy sequences are not, and
/// nothing realizes. A larger set, a set with another member, and a program
/// type that implements `IEquiv` (whose instance may be `=` to a keyword)
/// keep the lookup.
#[test]
fn membership_tests_switch_on_spellings() {
    let source = r#"
      (deftype T [])
      (def realized (atom 0))
      (defn kind [x] (if (#{:a :b} x) "ab" (if (contains? #{:a :b :ns/c} x) "abc" "other")))
      (defn kind2 [x] (let [s #{:x :y}] (cond (s x) "xy" (get #{:z} x) "z" :else "none")))
      (defn many [x] (if (#{:a :b :c :d :e :f :g :h :i} x) "nine" "no"))
      (defn mixed [x] (if (#{:a "a"} x) "mixed" "no"))
      (pr-str
        [(mapv kind [:a :b :ns/c :c nil "a" 'a (keyword "a") (keyword "ns" "c") [:a] {:a 1} (T.)
                     (lazy-seq (swap! realized inc) [:a])])
         (mapv kind2 [:x :y :z :w nil (keyword "z")]) (mapv many [:i :j]) (mapv mixed [:a "a" 'a])
         @realized])"#;
    let text = every_level(
        source,
        r#"[["ab" "ab" "abc" "other" "other" "other" "other" "ab" "abc" "other" "other" "other" "other"] ["xy" "xy" "z" "none" "none" "z"] ["nine" "no"] ["mixed" "mixed" "no"] 0]"#,
    );
    assert_eq!(text.matches("switch (rt$keywordFqn(").count(), 4, "{text}");
    assert_eq!(text.matches(".call(null, l").count(), 2, "{text}");
    let equiv = r#"
      (deftype K [] IEquiv (-equiv [_ o] (= o :a)) IHash (-hash [_] (hash :a)))
      (defn kind [x] (if (#{:a :b} x) "member" "no"))
      (pr-str [(kind (K.)) (kind :a) (kind :c)])"#;
    let text = every_level(equiv, r#"["member" "member" "no"]"#);
    assert!(!text.contains("switch ("), "{text}");
}

/// Membership in a set literal of at most 8 keywords, or of strings and
/// numbers, outside an `if` statement is a conditional expression over its
/// members (light-cljs-231): `contains?` gives a boolean, a call of the set
/// and `get` the member or nil. The tested value is evaluated once, read in
/// place where reading it again is reading it once (one member, or a local
/// compared with strings and numbers), else passed to an arrow function. An
/// `if` on membership in a set of strings and numbers is a `switch` on the
/// value.
#[test]
fn membership_in_value_position_compares_with_each_member() {
    let source = r#"
      (def ticks (atom 0))
      (defn tick [x] (swap! ticks inc) x)
      (defn kw [x] [(contains? #{:a :b} x) (#{:a :b} x) (get #{:a} x) (contains? #{:a :ns/c} (tick x))])
      (defn st [x] [(contains? #{"a" "b"} x) (get #{"a" "b"} x) (#{1 2} (tick x)) (if (#{1 -0.5} x) "in" "out")])
      (defn sw [x] (if (contains? #{"a" 2} x) "in" "out"))
      (pr-str [(mapv kw [:a :b :c (keyword "a") (keyword "ns" "c") "a" nil])
               (mapv st ["a" "b" 1 2 -0.5 :a nil]) (mapv sw ["a" 2 2.0 "2" :a]) @ticks])"#;
    let text = every_level(
        source,
        r#"[[[true :a :a true] [true :b nil false] [false nil nil false] [true :a :a true] [false nil nil true] [false nil nil false] [false nil nil false]] [[true "a" nil "out"] [true "b" nil "out"] [false nil 1 "in"] [false nil 2 "out"] [false nil nil "in"] [false nil nil "out"] [false nil nil "out"]] ["in" "in" "in" "out" "out"] 14]"#,
    );
    for call in ["rt$contains(", "rt$get(", ".call(null, l"] {
        assert!(!text.contains(call), "{call}: {text}");
    }
    assert_eq!(text.matches("((caseValue) => ").count(), 4, "{text}");
    assert!(text.contains("(rt$keywordFqn(l1) === \"a\" ? k"), "{text}");
    assert!(text.contains("(l2 === \"a\" || l2 === \"b\")"), "{text}");
    assert!(text.contains("switch (l3) {"), "{text}");
}

/// A lookup of a constant key in a map literal whose keys are distinct
/// constants, or in a local a `let` binds to one: an absent key is `nil` or
/// the default, and `count`, `contains?`, `empty?` and `map?` fold; a value
/// that is a constant or a read of a local is that, where the lookup is in
/// the function the map was built in; another value is read from its
/// entry, `rt$maps.get(m)[i].value`, in array maps and hash maps, also in a
/// closure and from a literal whose other values have effects, which still
/// run. A literal whose other values have no effect is its value. A map with
/// a key that is not a constant, or with two collection keys that may
/// merge, keeps its lookups.
#[test]
fn lookups_in_maps_with_constant_keys_read_their_entries() {
    let source = r#"
      (def log (atom []))
      (defn note [x] (swap! log conj x) x)
      (defn read-map [x y]
        (let [m {:x x :y y :w (inc x) :z nil nil :n "s" :s 0 :zero 'q :q}
              b {:a x :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9 :j (inc x) nil :nil}
              alias m]
          [(:x m) (:y m :d) (:w m) (get m :z :d) (get m nil) (m "s") (alias :w) (get m 0) ('q m)
           (:q m) (:q m :d) (get m :q :d) (m :q) (count m) (contains? m :x) (contains? m :q)
           (empty? m) (map? m) (:a b) (:j b) (get b nil) (b :i) (:zz b)
           (:x {:x x :y (note y)}) (:y {:x x :y (note y)}) (:x {:x (note x) :y (note y)})
           (:y {:x y :y (inc x)} (note :d))]))
      (defn closures []
        (loop [i 0 acc []]
          (if (< i 3)
            (let [m {:x i :y (inc i)}]
              (recur (inc i) (conj acc (fn [] [(:x m) (:y m)]))))
            (mapv #(%) acc))))
      (defn unshaped [x y]
        (let [k (keyword x) m {k 1 :y 2} n {[x] 1 [y] 2 :a 3}]
          [(:y m) (:a n) (count n)]))
      (pr-str [(read-map 1 2) (read-map nil [1]) (closures) (unshaped "x" 1) (unshaped "y" 1)
               @log])"#;
    let text = every_level(
        source,
        r#"[[1 2 2 nil :n :s 2 :zero :q nil :d :d nil 8 true false false true 1 2 :nil 9 nil 1 2 1 2] [nil [1] 1 nil :n :s 1 :zero :q nil :d :d nil 8 true false false true nil 1 :nil 9 nil nil [1] nil 1] [[0 1] [1 2] [2 3]] [2 3 3] [2 3 3] [2 2 1 2 :d [1] [1] nil [1] :d]]"#,
    );
    let function = |name: &str| {
        let start = text.find(&format!("{name} = (function(")).unwrap();
        &text[start..start + text[start..].find("})));").unwrap()]
    };
    let read_map = function("g2");
    for call in ["rt$count(", "rt$contains(", "rt$isEmpty(", "rt$isMap("] {
        assert!(!read_map.contains(call), "{call}: {read_map}");
    }
    // `(:w m)`, `(alias :w)`, `(:j b)` and the two literals with effects in
    // their other values; the default with an effect keeps its lookup, a
    // `get` (light-cljs-370).
    assert_eq!(read_map.matches("rt$maps.get(").count(), 5, "{read_map}");
    assert_eq!(read_map.matches("rt$get(").count(), 1, "{read_map}");
    assert!(!read_map.contains(".call(null, "), "{read_map}");
    assert_eq!(function("g3").matches("rt$maps.get(l").count(), 2, "{text}");
    // `unshaped` keeps its lookups and its count.
    let unshaped = function("g4");
    assert!(
        unshaped.contains("rt$count(") && unshaped.matches("rt$get(l").count() == 2,
        "{unshaped}"
    );
}
