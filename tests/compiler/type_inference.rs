//! Type inference and the lowerings it enables at `Advanced` (light-cljs-210;
//! docs/advanced-optimization.md, Phase 2 step 3 and "Phase 2 type inference
//! extension points").
//!
//! Every program runs at every level and must print what `None` prints, with
//! adversarial values: JavaScript-falsy numbers and strings, `NaN`, `-0`,
//! `undefined`, host objects, BigInts, keywords built at run time, records with
//! and without their own `ILookup`, and programs that extend core protocols,
//! which turn the typed lowerings off. The `Advanced` output is checked for
//! the lowered form, so a lowering that silently stops applying fails too.
use super::optimization::execute;
use light_cljs::core_operations::Type;
use light_cljs::inference::{Inferred, Known};
use light_cljs::{
    ApplicationOptions, OptimizationLevel, PreparedApplication, Program, analyze, parse,
    prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

fn prepared(source: &str, level: OptimizationLevel) -> PreparedApplication {
    prepare_application(&program(source), ApplicationOptions::new(level))
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level; returns the program part of the `Advanced` output.
fn every_level(source: &str, expected: &str) -> String {
    let program = program(source);
    let mut advanced = String::new();
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
        advanced = js;
    }
    program_text(&advanced).to_owned()
}

/// The program part of `Advanced` output: what follows the runtime slice.
/// Runtime units are indented, so the slice ends at its first unindented
/// closing line.
fn program_text(js: &str) -> &str {
    match js.find("  const [") {
        Some(start) => {
            let end = start + js[start..].find("\n})();\n").expect("the slice ends");
            &js[end..]
        }
        None => js,
    }
}

/// The inferred value of the last top-level form at `Advanced`.
fn last(source: &str) -> Inferred {
    let prepared = prepared(source, OptimizationLevel::Advanced);
    let inference = prepared.inference().expect("Advanced infers types");
    inference.value(prepared.program().root)
}

#[test]
fn inference_types_literals_calls_locals_and_definitions() {
    let number = Inferred::of(Type::NUMBER);
    assert_eq!(last("(+ 1 2)"), number);
    assert_eq!(last("(< 1 2)"), Inferred::of(Type::BOOLEAN));
    assert_eq!(last("(str 1 :a)"), Inferred::of(Type::STRING));
    assert_eq!(last("[1 2]"), Inferred::known(Type::VECTOR, Known::Vector));
    assert_eq!(last("#js [1 2]"), Inferred::known(Type::HOST, Known::Array));
    // Outside an operation's domain its result also includes the row's
    // `otherwise`.
    assert_eq!(last("(+ 1 \"a\")"), Inferred::ANY);
    // Loops reach a fixed point from their initializers and `recur`s.
    assert_eq!(
        last("(loop [i 0 acc []] (if (< i 3) (recur (inc i) (conj acc i)) i))"),
        number
    );
    // Sole definitions: a later top-level form reads the initializer's type,
    // also in a function it defines; a definition that is not a top-level form
    // may not have run, and the read sees nil then.
    assert_eq!(last("(def n 5) (+ n 1)"), number);
    assert_eq!(last("(def s \"a\") s"), Inferred::of(Type::STRING));
    assert_eq!(
        last("(def s \"a\") (defn f [] s) (f)"),
        Inferred::of(Type::STRING)
    );
    assert_eq!(
        last("(let [] (def s \"a\") s)"),
        Inferred::of(Type::STRING.union(Type::NIL))
    );
    assert_eq!(last("(def s \"a\") (def s 1) s"), Inferred::ANY);
    assert_eq!(last("(def s \"a\") (set! s 1) s"), Inferred::ANY);
    // Calls of known functions have their bodies' types, recursion included.
    assert_eq!(
        last("(defn f [x] (if (pos? x) (f (dec x)) :done)) (f 3)"),
        Inferred::of(Type::KEYWORD)
    );
    assert_eq!(
        last("(let [g (fn ([] 1) ([x] \"a\"))] (g))"),
        Inferred::of(Type::NUMBER.union(Type::STRING))
    );
    // Program types.
    let source = "(defrecord R [x]) (deftype T [x])";
    let record = last(&format!("{source} (->R 1)"));
    assert_eq!(record.ty, Type::RECORD);
    assert!(matches!(record.known, Known::Instance(_)));
    assert_eq!(last(&format!("{source} (map->R {{}})")), record);
    assert_eq!(last(&format!("{source} (R. 1)")), record);
    assert_eq!(last(&format!("{source} (T. 1)")).ty, Type::OBJECT);
    // `^boolean` tags are trusted, as upstream trusts them.
    assert_eq!(
        last("(defn ^boolean f [x] x) (f 1)"),
        Inferred::of(Type::BOOLEAN)
    );
    // Replaced operations and core protocols extended beyond the program's
    // types make calls unknown.
    assert_eq!(last("(set! cljs.core/+ str) (+ 1 2)"), Inferred::ANY);
    assert_eq!(
        last("(extend-type string ICounted (-count [_] \"n\")) (count \"ab\")"),
        Inferred::ANY
    );
    assert_eq!(
        last("(extend-type string ICounted (-count [_] \"n\")) (inc 1)"),
        number
    );
}

#[test]
fn inference_narrows_locals_by_the_tests_of_enclosing_ifs() {
    // `x` is a number or a string.
    let with_x = |body: &str| {
        last(&format!(
            "(let [x (if (pos? (js/Math.random)) 1 \"a\")] {body})"
        ))
        .ty
    };
    let [string, number, keyword] = [Type::STRING, Type::NUMBER, Type::KEYWORD];
    assert_eq!(with_x("(if (string? x) x :no)"), string.union(keyword));
    assert_eq!(with_x("(if (string? x) :yes x)"), number.union(keyword));
    assert_eq!(
        with_x("(if (not (number? x)) x :no)"),
        string.union(keyword)
    );
    assert_eq!(
        with_x("(if (nil? x) :nil x)"),
        number.union(string).union(keyword)
    );
    // Through the temporaries of `and` and `or`, and a local bound to a test.
    assert_eq!(with_x("(and (string? x) x)"), Type::BOOLEAN.union(string));
    assert_eq!(
        with_x("(if (or (number? x) (nil? x)) :n x)"),
        string.union(keyword)
    );
    assert_eq!(
        with_x("(let [s? (string? x)] (if s? x :no))"),
        string.union(keyword)
    );
    // `vector?` admits program types too, so it narrows only the then branch.
    assert_eq!(with_x("(if (vector? x) x :no)"), keyword);
    assert_eq!(
        with_x("(if (vector? x) :v x)"),
        number.union(string).union(keyword)
    );
    // A truthy local is not nil; `instance?` of a program type is an instance.
    assert_eq!(
        last("(let [x (if (pos? (js/Math.random)) nil \"a\")] (if x x \"b\"))").ty,
        string
    );
    let instance = last(
        "(deftype T [a])
         (let [x (if (pos? (js/Math.random)) (T. 1) 1)] (if (instance? T x) x nil))",
    );
    assert_eq!(instance.ty, Type::OBJECT.union(Type::NIL));
    assert!(matches!(instance.known, Known::Instance(_)));
    // `satisfies?` of a protocol only one record implements.
    let satisfied = last(
        "(defprotocol P (m [x])) (defrecord R [a] P (m [_] a))
         (let [x (if (pos? (js/Math.random)) (->R 1) 1)] (if (satisfies? P x) x nil))",
    );
    assert_eq!(satisfied.ty, Type::RECORD.union(Type::NIL));
    assert!(matches!(satisfied.known, Known::Instance(_)));
}

#[test]
fn truth_tests_drop_truth_when_javascript_truthiness_agrees() {
    // Booleans, sequences, collections, keywords and nil test as they are;
    // numbers, strings and unknown values keep `truth`.
    let source = r#"
      (defn lt [a b] (if (< a b) :lt :ge))
      (defn items [xs] (if (seq xs) :some :none))
      (defn named [x] (if (keyword x) :kw :not))
      (defn sized [xs] (if (count xs) :counted :never))
      (defn text [x] (if (str x) :text :never))
      (defn any [x] (if x :truthy :falsy))
      (def odd [0 -0.0 js/NaN "" false nil js/undefined [] {} :k #js {} #js [] (fn [])])
      (pr-str [(lt 1 2) (lt 2 1) (lt js/NaN 1) (items []) (items "") (items "a") (items nil)
               (named "a") (named 1) (sized []) (sized "") (text nil) (text "")
               (mapv any odd)])"#;
    let js = every_level(
        source,
        "[:lt :ge :ge :none :none :some :none :kw :not :counted :counted :text :text [:truthy :truthy :truthy :truthy :falsy :falsy :falsy :truthy :truthy :truthy :truthy :truthy :truthy]]",
    );
    assert!(js.contains("if ((l"), "{js}");
    for test in ["rt$seq(l", "rt$keywordCore(l"] {
        assert!(js.contains(&format!("if ({test}")), "{test}: {js}");
    }
    for kept in ["truth(rt$count(l", "truth(rt$str(l", "truth(l"] {
        assert!(js.contains(kept), "{kept}: {js}");
    }
    // A program that tests only booleans declares no `truth`.
    let program = program("(defn f [a b] (if (< a b) a b)) (f 1 2)");
    let advanced = prepare_application(
        &program,
        ApplicationOptions::new(OptimizationLevel::Advanced),
    )
    .emit_js();
    assert!(!advanced.contains("truth"), "{advanced}");
    assert!(
        prepare_application(
            &program,
            ApplicationOptions::new(OptimizationLevel::Prepare)
        )
        .emit_js()
        .contains("const truth = ")
    );
}

#[test]
fn and_or_become_short_circuits_when_the_first_operand_tests_as_it_is() {
    let source = r#"
      (defn between [x lo hi] (and (<= lo x) (< x hi)))
      (defn either [x] (or (nil? x) (= x :none) (keyword? x)))
      (defn first-of [x y] (or x y))
      (defn mixed [x] (and (number? x) x))
      (pr-str [(between 1 0 2) (between 2 0 2) (either nil) (either :none) (either :k)
               (either 0) (first-of 0 1) (first-of "" 1) (first-of false 1) (first-of nil nil)
               (mixed 0) (mixed "a") (mixed js/NaN) (when (and 0 "") :both)])"#;
    let js = every_level(
        source,
        "[true false true true true false 0 \"\" 1 nil 0 false ##NaN :both]",
    );
    assert!(js.contains("((l"), "{js}");
    assert!(js.contains(" && "), "{js}");
    assert!(js.contains(" || "), "{js}");
    // In value position the expansion's function scope goes away, so an
    // operand that reads `arguments` keeps it; in return position it stays.
    every_level(
        "(defn f [x] [(and true (aget (js-arguments) 0))])
         (defn g [x] (and true (aget (js-arguments) 0)))
         (pr-str [(f 5) (g 6)])",
        "[[nil] 6]",
    );
}

#[test]
fn arithmetic_comparisons_and_predicates_are_operators_for_every_operand() {
    // Each operation is the JavaScript operator its runtime definition uses,
    // so a lowered call (left) agrees with the runtime's (`apply`, right) on
    // every operand: nil, NaN, -0, strings, keywords, collections, host objects,
    // an object with `valueOf`, and a BigInt that makes mixing throw.
    let source = r#"
      (def values [nil true false 0 -0.0 1 1.5 -2 js/NaN js/Infinity "" "a" "1" :k [1] {}
                   #js [1 2] (js/Date. 0) #js {"valueOf" (fn [] 7)} (js/BigInt 1)])
      (defn run [f] (try [:ok (f)] (catch :default e [:throws (.-message e)])))
      (defn same? [f g]
        (let [[a x] (run f) [b y] (run g)]
          (and (= a b) (if (= a :ok) (js/Object.is x y) (= x y)))))
      (defn unary [a]
        [(same? #(+ a) #(apply + [a])) (same? #(* a) #(apply * [a]))
         (same? #(- a) #(apply - [a])) (same? #(/ a) #(apply / [a]))
         (same? #(< a) #(apply < [a])) (same? #(>= a) #(apply >= [a]))
         (same? #(inc a) #(apply inc [a])) (same? #(dec a) #(apply dec [a]))
         (same? #(nil? a) #(apply nil? [a])) (same? #(some? a) #(apply some? [a]))
         (same? #(zero? a) #(apply zero? [a])) (same? #(pos? a) #(apply pos? [a]))
         (same? #(neg? a) #(apply neg? [a])) (same? #(not a) #(apply not [a]))])
      (defn binary [a b]
        [(same? #(+ a b) #(apply + [a b])) (same? #(- a b) #(apply - [a b]))
         (same? #(* a b) #(apply * [a b])) (same? #(/ a b) #(apply / [a b]))
         (same? #(< a b) #(apply < [a b])) (same? #(<= a b) #(apply <= [a b]))
         (same? #(> a b) #(apply > [a b])) (same? #(>= a b) #(apply >= [a b]))
         (same? #(< a b a) #(apply < [a b a])) (same? #(>= b a b) #(apply >= [b a b]))
         (same? #(+ a b a) #(apply + [a b a])) (same? #(/ a b a) #(apply / [a b a]))
         (same? #(identical? a b) #(apply identical? [a b]))
         (same? #(= a b) #(apply = [a b])) (same? #(not= a b) #(apply not= [a b]))
         (same? #(= a :k) #(apply = [a :k])) (same? #(= :k a) #(apply = [:k a]))
         (same? #(= a 1) #(apply = [a 1])) (same? #(= "a" a) #(apply = ["a" a]))])
      (pr-str [(same? #(+) #(apply + [])) (same? #(*) #(apply * []))
               (vec (keep-indexed (fn [i a] (when-not (every? true? (unary a)) [i (unary a)]))
                                  values))
               (vec (for [a values b values :when (not (every? true? (binary a b)))]
                      [a b (binary a b)]))])"#;
    let js = every_level(source, "[true true [] []]");
    for operator in [
        "(l", " + ", " - ", " * ", " / ", " < ", " <= ", " > ", " >= ", " === ", " == null",
        " != null", " === 0", " > 0", " < 0",
    ] {
        assert!(js.contains(operator), "{operator}: {js}");
    }
    // A chain of plain operands compares neighbors.
    assert!(js.contains(" && "), "{js}");
    for call in ["rt$add(", "rt$less(", "rt$inc(", "rt$isNil("] {
        assert!(!js.contains(call), "{call}: {js}");
    }
}

#[test]
fn arithmetic_edge_cases_keep_the_runtime_results_and_evaluation_order() {
    let js = every_level(
        r#"(def log (atom []))
           (defn f [x] (swap! log conj x) x)
           (pr-str [(= 1 1.0) (/ 1 (- 0)) (/ 1 (- -0.0)) (/ 1 (* -1 0)) (/ 0 0) (/ 1 0)
                    (+ 0.1 0.2) (- 5) (/ 4) (+) (*) (< 1) (< 1 2 3) (< 1 3 2) (> 3 2 1)
                    (+ 1 "a") (+ nil 1) (< "a" 1) (< nil 1) (inc "a") (dec nil)
                    (< (f 3) (f 2) (f 1)) (+ (f 1) (f 2) (f 3)) @log])"#,
        r#"[true ##-Inf ##Inf ##-Inf ##NaN ##Inf 0.30000000000000004 -5 0.25 0 1 true true false true "1a" 1 false true "a1" -1 false 6 [3 2 1 1 2 3]]"#,
    );
    assert!(js.contains("(- -0)"), "{js}");
    // A chain with operands that run code keeps the runtime's single pass.
    assert!(js.contains("rt$less("), "{js}");
}

#[test]
fn equality_lowers_to_identity_for_primitives_and_spelling_for_keyword_literals() {
    let source = r#"
      (defrecord R [x])
      (defn kind? [x] (= x :k))
      (defn kind-first? [x] (= :ns/k x))
      (defn one? [x] (= x 1))
      (defn not-a? [x] (not= "a" x))
      (defn typed [m] (= (:type m) :circle))
      (def built [:k (keyword "k") (keyword nil "k") (read-string ":k") (Keyword. nil "k" "k" nil)
                  (keyword "ns" "k") (keyword "ns/k") (keyword nil "ns/k") (Keyword. "x" "y" "ns/k" nil)
                  'k "k" nil 1 1.0 "a" (->R 1) [:k] js/NaN])
      (pr-str [(mapv kind? built) (mapv kind-first? built) (mapv one? built) (mapv not-a? built)
               (typed {:type :circle}) (typed {:type (keyword "circle")}) (typed {}) (typed nil)
               (= 0 -0.0) (= ##NaN ##NaN) (= "" false) (not= 1 1.0)])"#;
    let js = every_level(
        source,
        "[[true true true true true false false false false false false false false false false false false false] \
         [false false false false false true true true true false false false false false false false false false] \
         [false false false false false false false false false false false false true true false false false false] \
         [true true true true true true true true true true true true true true false true true true] \
         true true false false true false false false]",
    );
    for lowered in [
        "(l",
        " === k",
        " || rt$keywordFqn(l",
        ") === \"k\")",
        ") === \"ns/k\")",
        " === 1)",
        "(\"a\" !== l",
        // An operand that cannot be read twice is tested by its spelling only.
        "(rt$keywordFqn((k",
    ] {
        assert!(js.contains(lowered), "{lowered}: {js}");
    }
    assert!(!js.contains("rt$equal("), "{js}");
    assert!(!js.contains("rt$notEqual("), "{js}");
    // A program type with its own `IEquiv` may equal anything, so `=` keeps
    // the runtime unless a primitive or keyword literal comes first.
    let js = every_level(
        r#"(deftype Any [] IEquiv (-equiv [_ _] true))
           (defn kind? [x] (= x :k))
           (defn one? [x] (= x 1))
           (pr-str [(kind? (Any.)) (one? (Any.)) (= :k (Any.)) (= 1 (Any.)) (kind? :k) (one? 1)])"#,
        "[true true false false true true]",
    );
    assert!(js.contains("rt$equal("), "{js}");
    assert!(js.contains("(1 === "), "{js}");
    // Extending a core protocol to a host type turns the lowering off.
    let js = every_level(
        r#"(extend-type number IEquiv (-equiv [_ _] true))
           (defn kind? [x] (= x :k))
           (pr-str [(= 1 2) (kind? 3) (= :k 3)])"#,
        "[true true false]",
    );
    assert!(!js.contains("(1 === 2)"), "{js}");
}

#[test]
fn count_reads_the_length_of_strings_arrays_and_flat_vectors() {
    let js = every_level(
        r#"(defn size [x] (if (string? x) (count x) -1))
           (pr-str [(count "aé") (count (str "ab" 1)) (count #js [1 2 3]) (count (array))
                    (count [1 2]) (count (vector)) (count (vector 1 2 3))
                    (mapv size ["" "abc" nil 5 [1]]) (count (js/String.fromCodePoint 128512))
                    (count (first {:a 1})) (count (subvec [1 2 3] 1)) (count nil)])"#,
        "[2 3 3 0 2 0 3 [0 3 -1 -1 -1] 2 2 2 0]",
    );
    for lowered in [").length", "rt$vectors.get("] {
        assert!(js.contains(lowered), "{lowered}: {js}");
    }
    // Map entries, subvectors and nil keep the runtime's `count`.
    assert!(js.contains("rt$count("), "{js}");
    // An extension of `ICounted` to strings turns it off.
    let js = every_level(
        r#"(extend-type string ICounted (-count [_] 42))
           (pr-str [(count "abc") (count [1])])"#,
        "[3 1]",
    );
    assert!(!js.contains(").length"), "{js}");
}

#[test]
fn nth_on_flat_vectors_uses_the_direct_index_path() {
    let js = every_level(
        r#"(defn safe [f] (try (f) (catch :default e (.-message e))))
           (let [v [10 20 30]]
             (pr-str [(nth v 0) (nth v 2) (nth v 1.7) (nth v -0.5 :nf) (nth v 3 :nf)
                      (nth v js/NaN :nf) (nth v -0.0) (safe #(nth v 3)) (safe #(nth v -1))
                      (safe #(nth v "1")) (safe #(nth v nil))
                      (let [[a b] v] (+ a b)) (nth (vector 1 2) 1)
                      (nth (first {:a 1}) 1) (safe #(nth (first {:a 1}) 2))
                      (nth (subvec v 1) 0) (nth #js [1 2] -0.5)]))"#,
        r#"[10 30 20 :nf :nf :nf 10 "Index out of bounds" "Index out of bounds" "Index argument to nth must be a number" "Index argument to nth must be a number" 30 2 1 "Index out of bounds" 20 1]"#,
    );
    assert!(js.contains("rt$vectorNth(rt$vectors.get(l"), "{js}");
    assert!(js.contains(", rt$missing)"), "{js}");
    // A string index, map entries, subvectors and arrays keep `nth`.
    assert!(js.contains("rt$nth("), "{js}");
}

#[test]
fn keyword_lookup_on_known_records_reads_the_field() {
    let js = every_level(
        r#"(defrecord P [x a-b c?])
           ;; The record's own `ILookup` wins over one in its body.
           (defrecord Q [x] ILookup (-lookup [_ k] [:own k]) (-lookup [_ k nf] [:own k nf]))
           (defrecord E [x])
           (extend-type E ILookup (-lookup ([_ k] :extended) ([_ k nf] :extended)))
           (defn width [s] (if (instance? P s) (:x s) :none))
           (let [p (->P 1 2 3) q (->Q 1) e (->E 1)]
             (pr-str [(:x p) (:a-b p) (:c? p) (get p :x) (:z p) (:x (assoc p :x 5))
                      (:x (map->P {:x 7})) (:x (P. 9 0 0)) (:x q) (:x e)
                      (width p) (width q) (width nil) (:x (merge p {:x 8}))]))"#,
        "[1 2 3 1 nil 5 7 9 1 :extended 1 :none :none 8]",
    );
    for field in ["[\"x\"]", "[\"a_b\"]", "[\"c_QMARK_\"]"] {
        assert!(js.contains(&format!("(l0){field}")), "{field}: {js}");
    }
    // Inside `instance?` the receiver is narrowed to the record.
    assert!(js.matches(")[\"x\"]").count() >= 4, "{js}");
    // A type defined twice is not known.
    let js = every_level(
        "(defrecord P [x]) (defrecord P [x y]) (pr-str (:x (->P 1 2)))",
        "1",
    );
    assert!(!js.contains(")[\"x\"]"), "{js}");
}

#[test]
fn parameters_that_code_may_assign_are_not_narrowed() {
    // Parameters are assignable: raw text may assign a `js*` hole, and the
    // arguments object aliases the (non-strict) parameters.
    let js = every_level(
        r#"(defn hole [x] (if (string? x) (do (js* "~{} = 5" x) (try (count x) (catch :default e :threw))) 0))
           (defn args [x] (if (string? x) (do (aset (js-arguments) 0 5) (try (count x) (catch :default e :threw))) 0))
           (defn plain [x] (if (string? x) (count x) 0))
           (pr-str [(hole "abc") (args "abc") (plain "abc")])"#,
        "[:threw :threw 3]",
    );
    assert_eq!(js.matches(").length").count(), 1, "{js}");
}

#[test]
fn unused_element_wise_calls_of_inert_functions_are_removed() {
    // Phase 2 step 6 left `invoke` rows to inference: `mapv` and friends are
    // inert when their function is inert on the elements of plain data.
    let source = r#"(def log (atom []))
       (def doubled (mapv (fn [x] (* 2 x)) [1 2 3]))
       (def flags (every? keyword? #{:a :b}))
       (def noisy (mapv (fn [x] (swap! log conj x)) [1 2]))
       (def throwing (try (mapv (fn ([] 0) ([a b] 1)) [1]) (catch :default _ :threw)))
       (def used (filterv pos? [-1 2 3]))
       (pr-str [used @log throwing])"#;
    every_level(source, "[[2 3] [1 2] :threw]");
    let removed =
        OptimizationLevel::ALL.map(|level| prepared(source, level).stats().removed_definitions);
    assert_eq!(removed, [0, 0, 2]);
}

#[test]
fn boolean_tags_are_trusted_like_upstream() {
    let source = "(do (defn ^boolean small? [x] (< x 10)) (if (small? 3) :small :large))";
    let js = every_level(&format!("(pr-str {source})"), ":small");
    assert!(!js.contains("truth("), "{js}");
    // The documented consequence: a tag that lies is believed at `Advanced`
    // only (docs/advanced-optimization.md, "Advanced-mode contract").
    let source = "(defn ^boolean lies [] 0) (pr-str (if (lies) :truthy :falsy))";
    let printed = OptimizationLevel::ALL.map(|level| {
        let js = prepared(source, level).emit_js();
        execute(&format!("console.log({js});"))
    });
    assert_eq!(printed, [":truthy\n", ":truthy\n", ":falsy\n"]);
}
