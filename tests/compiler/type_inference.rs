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
//!
//! Since light-cljs-229 the parameters of closed functions have the types of
//! their calls' arguments: the lowerings apply inside such functions, while
//! callers that inference cannot see (`apply`, a function passed as a value, a
//! name or local that escapes) keep them `any`. A seeded generator of closed
//! functions called with adversarial arguments checks every level against
//! `None`.
use super::generated::{SEEDS, namespace_program};
use super::generated_optimization::analyze as analyze_generated;
use super::optimization::{execute, readable};
use light_cljs::core_operations::Type;
use light_cljs::inference::{Inferred, Known};
use light_cljs::{
    ApplicationOptions, ApplicationStats, LocalId, OptimizationLevel, PreparedApplication, Program,
    analyze, parse, prepare_application,
};

fn program(source: &str) -> Program {
    analyze(source, &parse(source).unwrap()).unwrap_or_else(|error| panic!("{source}\n{error}"))
}

/// Options at `level` without inlining (light-cljs-214), which would replace
/// the functions whose parameters and bodies this suite checks with their
/// bodies. Constant folding still runs.
fn options(level: OptimizationLevel) -> ApplicationOptions {
    let mut options = ApplicationOptions::new(level);
    options.no_inlining = true;
    options
}

/// [`options`] with readable `Advanced` output (`pseudo_names`), whose text
/// the tests read.
fn readable_options(level: OptimizationLevel) -> ApplicationOptions {
    let mut options = options(level);
    options.pseudo_names = true;
    options
}

fn prepared(source: &str, level: OptimizationLevel) -> PreparedApplication {
    prepare_application(&program(source), options(level))
}

/// The program prints `expected` (its value, through `console.log`) at every
/// level; returns the program part of the readable `Advanced` output.
fn every_level(source: &str, expected: &str) -> String {
    let program = program(source);
    for level in OptimizationLevel::ALL {
        let js = prepare_application(&program, options(level)).emit_js();
        assert_eq!(
            execute(&format!("console.log({js});")),
            format!("{expected}\n"),
            "at {level}: {source}"
        );
    }
    let advanced =
        prepare_application(&program, readable_options(OptimizationLevel::Advanced)).emit_js();
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

/// The inferred value of the first local named `name` at `Advanced`.
fn local(source: &str, name: &str) -> Inferred {
    let prepared = prepared(source, OptimizationLevel::Advanced);
    let local = prepared
        .program()
        .locals
        .iter()
        .position(|local| local.name == name)
        .unwrap_or_else(|| panic!("no local {name}"));
    let inference = prepared.inference().expect("Advanced infers types");
    inference.local(LocalId(local))
}

#[test]
fn inference_types_literals_calls_locals_and_definitions() {
    let number = Inferred::of(Type::NUMBER);
    assert_eq!(last("(+ 1 2)"), number);
    assert_eq!(last("(< 1 2)"), Inferred::of(Type::BOOLEAN));
    assert_eq!(last("(str 1 :a)"), Inferred::of(Type::STRING));
    // Since light-cljs-243 a collection literal also carries its elements.
    assert_eq!(
        last("[1 2]"),
        Inferred::known(Type::VECTOR, Known::Vector).with_elements(number)
    );
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
    // may not have run, and the read sees nil then. (A `let` whose bindings
    // fold away is a `do`, whose forms are top-level forms, so this one binds
    // a value with an effect; light-cljs-214.)
    assert_eq!(last("(def n 5) (+ n 1)"), number);
    assert_eq!(last("(def s \"a\") s"), Inferred::of(Type::STRING));
    assert_eq!(
        last("(def s \"a\") (defn f [] s) (f)"),
        Inferred::of(Type::STRING)
    );
    assert_eq!(
        last("(let [t (js/Date.now)] (def s \"a\") s)"),
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
    let advanced = prepare_application(&program, readable(OptimizationLevel::Advanced)).emit_js();
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
    // Constant operands fold to the runtime's results (light-cljs-214), and
    // operands outside an operation's domain keep the operator.
    assert!(
        js.contains(
            "[true, -Infinity, Infinity, -Infinity, NaN, Infinity, 0.30000000000000004, -5, \
             0.25, 0, 1, true, true, false, true, (1 + \"a\"), (null + 1), "
        ),
        "{js}"
    );
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

#[test]
fn parameters_of_closed_functions_join_the_arguments_of_their_calls() {
    let [number, string, nil] = [Type::NUMBER, Type::STRING, Type::NIL].map(Inferred::of);
    // A call of a closed function has its body's type, here a parameter's.
    assert_eq!(last("(defn f [x] x) (f 1)"), number);
    assert_eq!(
        last("(defn f [x] x) (f 1) (f \"a\")"),
        Inferred::of(Type::NUMBER.union(Type::STRING))
    );
    // A missing argument is nil (`undefined`); the rest parameter is the seq
    // `fnRest` builds, or nil without rest arguments.
    assert_eq!(last("(defn f [a b] b) (f 1)"), nil);
    assert_eq!(
        last("(defn f [a & xs] xs) (f 1 2 3)"),
        Inferred::of(Type::SEQ)
    );
    assert_eq!(last("(defn f [a & xs] xs) (f 1)"), nil);
    // Each method's parameters join the calls that select it.
    assert_eq!(last("(defn f ([x] (f x \"d\")) ([x y] y)) (f 1)"), string);
    // Arguments that depend on parameters: recursion, `recur` and mutual
    // recursion across a `declare`.
    assert_eq!(
        last("(defn sum [n acc] (if (pos? n) (sum (dec n) (+ acc n)) acc)) (sum 3 0)"),
        number
    );
    assert_eq!(
        last("(defn sum [n acc] (if (pos? n) (recur (dec n) (+ acc n)) acc)) (sum 3 0)"),
        number
    );
    let mutual = "(declare b) (defn a [x n] (if (pos? n) (b x (dec n)) x))
                  (defn b [y n] (a y n)) (a \"s\" 3)";
    assert_eq!(local(mutual, "x"), string);
    assert_eq!(local(mutual, "y"), string);
    // Local functions, `fn` literals called in place, and named and `letfn`
    // functions calling themselves.
    assert_eq!(last("(let [g (fn [x] x)] (g 1))"), number);
    assert_eq!(last("((fn [x] x) 1)"), number);
    assert_eq!(
        last("((fn self [x] (if (pos? x) (self (dec x)) x)) 3)"),
        number
    );
    assert_eq!(
        last("(letfn [(g [x] (if (pos? x) (h (dec x)) x)) (h [y] (g y))] (g 3))"),
        number
    );
    // Representations and elements pass through parameters.
    assert_eq!(
        last("(defn f [v] v) (f [1 2])"),
        Inferred::known(Type::VECTOR, Known::Vector).with_elements(number)
    );
    let record = last("(defrecord R [a]) (defn f [r] r) (f (->R 1))");
    assert_eq!(record.ty, Type::RECORD);
    assert!(matches!(record.known, Known::Instance(_)));
    // Anything that may call the function some other way keeps `x` `any`:
    // `apply`, the function as a value (of its global, its `def`, its name or
    // a local), a `loop` local, assignable parameters, a call that underfills a
    // variadic method or selects no method, and `with-redefs`.
    assert_eq!(local("(defn f [x] x) (f 1)", "x"), number);
    // Since light-cljs-243 a core operation that calls its function argument
    // as its callback shape says is one more caller (see
    // `callback_parameters_join_what_core_operations_pass` below).
    let number_or_string = Inferred::of(Type::NUMBER.union(Type::STRING));
    for source in [
        "(defn f [x] x) (mapv f [\"a\"]) (f 1)",
        "(let [g (fn [x] x)] (mapv g [\"a\"]) (g 1))",
    ] {
        assert_eq!(local(source, "x"), number_or_string, "{source}");
    }
    for source in [
        "(defn f [x] x) (apply f [1]) (f 1)",
        "(defn f [x] x) (def g f) (f 1)",
        "(declare f) (def h (defn f [x] x)) (f 1)",
        "(def f (fn self [x] (js/Array.of self) x)) (f 1)",
        "(let [g (fn [x] x) h g] (h \"a\") (g 1))",
        "(loop [g (fn [x] x)] (g 1))",
        "(defn f [x] (js* \"~{} = 'a'\" x) x) (f 1)",
        "(defn f [x] (aset (js-arguments) 0 \"a\") x) (f 1)",
        "(defn f ([] 0) ([a x & r] x)) (f 1)",
        "(defn f ([] 0) ([x] x)) (f 1) (f 1 2)",
        "(defn f [x] x) (with-redefs [f identity] (f 1))",
    ] {
        assert_eq!(local(source, "x"), Inferred::ANY, "{source}");
    }
}

/// `(defn f0 [x] x) (defn f1 [x] (f0 x)) ... (fN "abc")`, or the same chain in
/// one `letfn` followed by `(+ 1 2)`.
fn chain(links: usize, letfn: bool) -> String {
    let functions = (1..links)
        .map(|i| format!("f{i} [x] (f{} x)", i - 1))
        .collect::<Vec<_>>();
    let last = links - 1;
    if letfn {
        let functions = functions.iter().map(|function| format!("({function})"));
        format!(
            "(letfn [(f0 [x] x) {}] (f{last} \"abc\")) (+ 1 2)",
            functions.collect::<Vec<_>>().join(" ")
        )
    } else {
        let functions = functions
            .iter()
            .map(|function| format!("(defn {function})"));
        format!(
            "(defn f0 [x] x) {} (f{last} \"abc\")",
            functions.collect::<Vec<_>>().join(" ")
        )
    }
}

#[test]
fn parameter_types_converge_along_long_call_chains() {
    // Parameter types flow backward from a call to the definitions it reaches,
    // and rounds alternate their direction, so a long chain of top-level
    // definitions converges in a few rounds.
    let source = chain(100, false);
    let advanced = prepared(&source, OptimizationLevel::Advanced);
    let inference = advanced.inference().unwrap();
    assert_eq!(
        inference.value(advanced.program().root),
        Inferred::of(Type::STRING)
    );
    assert!(inference.rounds() <= 4, "{}", inference.rounds());
    assert_eq!(advanced.stats().typed_parameters, 100);
    // In one form each link takes a round. Past the round limit, inference
    // drops the parameter types, not every type.
    let source = chain(100, true);
    let advanced = prepared(&source, OptimizationLevel::Advanced);
    let inference = advanced.inference().unwrap();
    assert_eq!(
        inference.value(advanced.program().root),
        Inferred::of(Type::NUMBER)
    );
    assert_eq!(advanced.stats().typed_parameters, 0);
    every_level(
        &format!("(pr-str [{}])", chain(100, true).replace(" (+ 1 2)", "")),
        "[\"abc\"]",
    );
}

#[test]
fn typed_lowerings_apply_to_the_parameters_of_closed_functions() {
    let js = every_level(
        r#"(defrecord P [x y])
           (defn size [s] (count s))
           (defn at [v i] (nth v i))
           (defn at-or [v i nf] (nth v i nf))
           (defn same? [a b] (= a b))
           (defn differ? [a b] (not= a b))
           (defn circle? [k] (= k :circle))
           (defn px [p] (:x p))
           (defn py [p] (get p :y))
           (defn pick [flag] (if flag :yes :no))
           (defn negate [b] (not b))
           (defn both [a b] (and a b))
           (defn safe [f] (try (f) (catch :default e (.-message e))))
           (pr-str [(size "") (size "aé") (size (str 1 2)) (size (str (js/String.fromCodePoint 128512)))
                    (at [10 20 30] 0) (at [10 20 30] 1.7) (at [10 20 30] -0.0) (at (vector 1 2) 1)
                    (safe #(at [10 20 30] 3)) (safe #(at [10 20 30] -1)) (safe #(at [10 20 30] (/ 0 0)))
                    (at-or [1 2] 5 :nf) (at-or [1 2] (/ 0 0) :nf) (at-or [1 2] -0.5 :nf) (at-or [1 2] 1 :nf)
                    (same? 0 -0.0) (same? (/ 0 0) (/ 0 0)) (same? 1 1.0) (same? "a" "a") (same? 1 "1")
                    (same? "" 0) (same? true 1) (differ? 1 2) (differ? "a" "a")
                    (circle? :circle) (circle? (keyword "circle")) (circle? :ns/circle)
                    (circle? (keyword "x" "circle")) (circle? (keyword nil "circle"))
                    (px (->P 1 2)) (px (map->P {:x 3 :y 4})) (px (P. 5 6)) (py (->P 7 8))
                    (pick true) (pick false) (pick nil) (pick (< 1 2))
                    (negate true) (negate nil) (both true false) (both nil true) (both true :k)])"#,
        "[0 2 2 2 10 20 10 2 \"Index out of bounds\" \"Index out of bounds\" \"Index out of bounds\" \
         :nf :nf :nf 2 true false true true false false false true false true true false false true \
         1 3 5 8 :yes :no :no :yes false true false nil :k]",
    );
    for lowered in [
        "(l",
        ").length",
        "rt$vectorNth(rt$vectors.get(l",
        ", rt$missing)",
        " === ",
        " !== ",
        " || rt$keywordFqn(l",
        ")[\"x\"]",
        ")[\"y\"]",
        "if (l",
        "(!l",
        " && ",
    ] {
        assert!(js.contains(lowered), "{lowered}: {js}");
    }
    for call in [
        "rt$count(",
        "rt$nth(",
        "rt$equal(",
        "rt$notEqual(",
        "truth(",
    ] {
        assert!(!js.contains(call), "{call}: {js}");
    }
}

#[test]
fn parameters_with_unseen_or_mixed_callers_keep_the_runtime_operations() {
    // Each function has a direct call the lowering would accept, and a caller
    // that inference cannot see (`apply`, `partial`, the function or a local
    // bound to it as a value) or a direct call with another type.
    let js = every_level(
        r#"(defrecord P [x])
           (defn size-mixed [s] (count s))
           (defn size-applied [s] (count s))
           (defn size-mapped [s] (count s))
           (defn same-applied? [a b] (= a b))
           (defn x-partial [p] (:x p))
           (defn at-mixed [v i] (nth v i))
           (defn pick-mixed [flag] (if flag :yes :no))
           (let [local-size (fn [s] (count s))]
             (pr-str [(size-mixed "abc") (size-mixed [1 2]) (size-mixed nil)
                      (size-applied "ab") (apply size-applied [[1 2 3]])
                      (size-mapped "ab") (mapv size-mapped [[1] #{1 2} {:a 1}])
                      (same-applied? 1 1) (apply same-applied? [[1] [1]])
                      (x-partial (->P 1)) ((partial x-partial) {:x 9})
                      (at-mixed [1 2] 1) (at-mixed (subvec [1 2 3] 1) 0)
                      (try (at-mixed [1 2] "1") (catch :default e (.-message e)))
                      (pick-mixed true) (pick-mixed 0) (pick-mixed "")
                      (local-size "ab") (mapv local-size [[1 2 3]])]))"#,
        "[3 2 0 2 3 2 [1 2 1] true true 1 9 2 2 \"Index argument to nth must be a number\" \
         :yes :yes :yes 2 [3]]",
    );
    for kept in ["rt$count(", "rt$equal(", "rt$nth(", "truth("] {
        assert!(js.contains(kept), "{kept}: {js}");
    }
    for lowered in [").length", "rt$vectorNth(", ")[\"x\"]", " === "] {
        assert!(!js.contains(lowered), "{lowered}: {js}");
    }
}

/// Since light-cljs-243 a function passed to a core operation with a callback
/// shape in `src/core_operations.tsv` has that operation as one more caller:
/// `map` calls it with an element of each collection, `reduce` with the
/// accumulator and an element, `swap!` with the state and its arguments.
#[test]
fn callback_parameters_join_what_core_operations_pass() {
    let number = Inferred::of(Type::NUMBER);
    let string = Inferred::of(Type::STRING);
    for (source, name, expected) in [
        ("(mapv (fn [x] x) [1 2])", "x", number),
        ("(defn f [x] x) (mapv f [\"a\"])", "x", string),
        ("(let [g (fn [x] x)] (mapv g (range 3)))", "x", number),
        ("(map (fn [x y] y) [1] [\"a\"])", "y", string),
        ("(map-indexed (fn [i x] i) [\"a\"])", "i", number),
        ("(reduce (fn [acc x] (+ acc x)) 0 [1 2])", "acc", number),
        (
            "(reduce (fn ([] 0) ([acc x] (+ acc x))) [1 2])",
            "acc",
            number,
        ),
        (
            "(filterv (fn [x] (pos? x)) (filter even? (range 10)))",
            "x",
            number,
        ),
        ("(mapv (fn [c] c) \"ab\")", "c", string),
        ("(mapv (fn [e] e) {:a 1})", "e", Inferred::of(Type::VECTOR)),
        // `conj` may return a map, whose elements are entries (vectors).
        (
            "(mapv (fn [x] x) (conj [1] \"a\"))",
            "x",
            Inferred::of(Type::NUMBER.union(Type::STRING).union(Type::VECTOR)),
        ),
        (
            "(def a (atom 0)) (swap! a (fn [n k] k) :key)",
            "k",
            Inferred::of(Type::KEYWORD),
        ),
        (
            "(def a (atom 0)) (swap! a (fn [n k] k) :key)",
            "n",
            Inferred::ANY,
        ),
        // The element type of what `map` returns is what its function returns.
        ("(mapv (fn [x] x) (map inc [1 2]))", "x", number),
        (
            "(mapv (fn [x] x) (map (fn [y] (str y)) [1 2]))",
            "x",
            string,
        ),
    ] {
        assert_eq!(local(source, name), expected, "{source}");
    }
    let record = local("(defrecord R [a]) (mapv (fn [r] r) [(->R 1)])", "r");
    assert!(matches!(record.known, Known::Instance(_)), "{record:?}");
    // Every other way an operation may call the function keeps `any`: a
    // transducer or comparator (no callback shape), `apply`, an argument
    // outside the operation's domain (a JavaScript array is not `seqable`),
    // and a program type implementing `IReduce`, `IKVReduce` or `ISwap`, whose
    // method receives the function itself.
    for (source, name) in [
        ("(into [] (map (fn [x] x)) [1])", "x"),
        ("(sort-by first (fn [a b] (compare a b)) [[1] [2]])", "a"),
        ("(apply mapv (fn [x] x) [[1]])", "x"),
        ("(mapv (fn [x] x) (js/Array.of 1))", "x"),
        (
            "(deftype T [] IReduce (-reduce [_ f init] (f init \"\")))
             (reduce (fn [acc x] x) 0 (T.))",
            "acc",
        ),
        (
            "(deftype T [] IReduce (-reduce [_ f init] (f init \"\")))
             (reduce (fn [acc x] x) 0 [1])
             (reduce (fn [acc y] y) 0 (T.))",
            "y",
        ),
        (
            "(deftype S [] ISwap (-swap! [_ f a] (f \"\" a)))
             (swap! (S.) (fn [n k] k) :key)",
            "k",
        ),
    ] {
        assert_eq!(local(source, name), Inferred::ANY, "{source}");
    }
    // Only `IReduce` makes the second program's first `reduce` keep `any`
    // where its collection may be a program type: here it is a vector.
    assert_eq!(
        local(
            "(deftype T [] IReduce (-reduce [_ f init] (f init \"\")))
             (reduce (fn [acc x] x) 0 [1])
             (reduce (fn [acc y] y) 0 (T.))",
            "x"
        ),
        number
    );
}

#[test]
fn typed_lowerings_apply_inside_callbacks_and_keep_the_results() {
    // The lowerings inside callbacks run on adversarial elements. A program
    // type implementing `ISwap` gets the function `swap!` is given and may
    // pass it anything (here `""` where the shape says `false`), so its
    // callback keeps the runtime test; `reduce` over a program type is
    // outside the table's domain.
    let js = every_level(
        r#"(defrecord Item [label qty])
           (deftype Odd [] IReduce (-reduce [_ f init] (f "" init)))
           (deftype Box [] ISwap (-swap! [_ f a] (f nil "")))
           (defn heavy? [item] (> (:qty item) 2))
           (defn flag [b] (if b :yes :no))
           (def total (atom 0))
           (let [items [(->Item "a" 1) (->Item "bb" 3) (map->Item {:label "" :qty 5})]]
             (swap! total (fn [n k] (if (= k :all) (+ n (count items)) n)) :all)
             (swap! total (fn [n k] (if (= k :all) (+ n 100) n)) (keyword "all"))
             (pr-str [(mapv :label (filter heavy? items))
                      (reduce (fn [n item] (+ n (count (:label item)))) 0 items)
                      (mapv flag [nil false true])
                      (mapv (fn [x] (if x 1 0)) [0 "" nil false (/ 0 0)])
                      (reduce (fn [acc x] (if acc :truthy :falsy)) false (Odd.))
                      (swap! (Box.) (fn [n k] (if k :truthy :falsy)) false)
                      @total]))"#,
        "[[\"bb\" \"\"] 3 [:no :no :yes] [1 1 0 0 1] :truthy :truthy 103]",
    );
    for lowered in [")[\"qty\"] > 2", "if (l", " || rt$keywordFqn(l"] {
        assert!(js.contains(lowered), "{lowered}: {js}");
    }
}

#[test]
fn every_callback_shape_matches_what_the_runtime_passes() {
    // Each operation with a callback shape calls a function that tests the
    // truthiness of the parameters its shape types as `nil`, `boolean` or
    // `keyword`, which `Advanced` lowers to JavaScript tests. A shape that put
    // an index, a `\"\"` or another argument where it says such a value is
    // would print differently from `None`.
    let js = every_level(
        r#"(def els [nil false :k])
           (pr-str
            [(mapv (fn [x] (if x :t :f)) els)
             (vec (map (fn [x y] (if y :t :f)) els els))
             (vec (map-indexed (fn [i x] (if x (if i :t :i) :f)) (reverse els)))
             (vec (keep-indexed (fn [i x] (if x (if i :t :i) :f)) (reverse els)))
             (vec (filter (fn [x] (if x true false)) els))
             (vec (remove (fn [x] (if x true false)) els))
             (vec (keep (fn [x] (if x :t nil)) els))
             (vec (take-while (fn [x] (if x false true)) els))
             (vec (drop-while (fn [x] (if x false true)) els))
             (vec (partition-by (fn [x] (if x :t :f)) els))
             (vec (mapcat (fn [x y] [(if x :t :f) (if y :t :f)]) els els))
             (reduce (fn ([] :empty) ([acc x] (if x acc :f))) els)
             (reduce (fn ([] :empty) ([acc x] (if x acc :f))) [])
             (reduce (fn [acc x] (if acc (if x :t :f) :none)) nil els)
             (vec (reductions (fn [acc x] (if acc (if x :t :f) :none)) nil els))
             (vec (reductions (fn ([] :empty) ([acc x] (if x acc :f))) []))
             (reduce-kv (fn [acc k v] (if acc acc (if v :t :f))) nil {:a false :b :k})
             (filterv (fn [x] (if x true false)) els)
             (every? (fn [x] (if x true false)) els)
             (some (fn [x] (if x x nil)) els)
             (not-any? (fn [x] (if x true false)) els)
             (not-every? (fn [x] (if x true false)) els)
             (run! (fn [x] (if x :t :f)) els)
             (group-by (fn [x] (if x :t :f)) els)
             (vec (sort (fn [a b] (if a (if b 0 1) (if b -1 0))) [:k nil :k false]))
             (vec (sort-by (fn [x] (if x 1 0)) els))
             (vec (take 3 (iterate (fn [x] (if x nil :k)) false)))
             (vec (repeatedly 2 (fn [] :r)))
             (update {:a false} :a (fn [v x] (if x :t :f)) false)
             (update-in {:a {:b 1}} [:a :b] (fn [v x] (if x :t :f)) nil)
             (merge-with (fn [a b] (if b :t :f)) {:a 1} {:a false})
             (update-vals {:a false} (fn [v] (if v :t :f)))
             (update-keys {:a 1} (fn [k] (if k k :f)))
             (let [a (atom nil)] (swap! a (fn [v x] (if x :t :f)) false) @a)
             (let [a (atom nil)] (swap-vals! a (fn [v x y] (if y :t :f)) :k false))])"#,
        "[[:f :f :t] [:f :f :t] [:t :f :f] [:t :f :f] [:k] [nil false] [:t] [nil false] [:k] [(nil false) (:k)] [:f :f :f :f :t :t] :f :empty :t [nil :none :f :t] [:empty] :f [:k] false :k false true nil {:f [nil false], :t [:k]} [nil false :k :k] [nil false :k] [false :k nil] [:r :r] {:a :f} {:a {:b :f}} {:a :f} {:a :f} {:a 1} :f [nil :f]]",
    );
    assert!(js.contains("if (l"), "{js}");
}

#[test]
fn callback_shapes_respect_core_protocol_extensions() {
    // A program that extends a core protocol to a runtime type changes what
    // traversing its values yields: here `(seq [false])` is `("")`, so the
    // callback's parameter is not the vector literal's `false`. The results
    // print in a list, since printing a vector traverses it too.
    let source = r#"(extend-type PersistentVector ISeqable (-seq [_] (list "")))
                    (pr-str (list (first (map (fn [x] (if x :yes :no)) [false]))
                                  (nth (mapv (fn [y] (if y :yes :no)) [false]) 0)))"#;
    assert_eq!(local(source, "x"), Inferred::ANY);
    assert_eq!(local(source, "y"), Inferred::ANY);
    every_level(source, "(:yes :yes)");
}

#[test]
fn callback_feedback_joins_return_classes() {
    // What the function returns feeds the accumulator of `reduce` and
    // `reductions` and the next argument of `iterate`: a function that turns
    // `false` into numbers makes the parameter `boolean|number`, so its
    // truthiness test keeps `0` truthy.
    let joined = Inferred::of(Type::BOOLEAN.union(Type::NUMBER));
    for (source, name) in [
        (
            "(reduce (fn [acc x] (if acc 0 1)) false [nil nil nil])",
            "acc",
        ),
        (
            "(vec (reductions (fn [acc x] (if acc 0 1)) false [nil nil nil]))",
            "acc",
        ),
        ("(vec (take 4 (iterate (fn [x] (if x 0 1)) false)))", "x"),
    ] {
        assert_eq!(local(source, name), joined, "{source}");
    }
    every_level(
        "(pr-str [(reduce (fn [acc x] (if acc 0 1)) false [nil nil nil])
                  (vec (reductions (fn [acc x] (if acc 0 1)) false [nil nil nil]))
                  (vec (take 4 (iterate (fn [x] (if x 0 1)) false)))])",
        "[0 [false 1 0 0] [false 1 0 0]]",
    );
}

#[test]
fn replaced_core_operations_leave_callback_parameters_unknown() {
    // A program that replaces an operation with a callback shape calls its
    // function its own way, so the shape says nothing about the parameters.
    let source = r#"(set! cljs.core/mapv (fn [f xs] [(f "")]))
                    (defn f [x] (if x :yes :no))
                    (pr-str (mapv f [false]))"#;
    assert_eq!(local(source, "x"), Inferred::ANY);
    every_level(source, "[:yes]");
}

#[test]
fn protocol_calls_on_parameters_of_one_program_type_are_devirtualized() {
    let source = r#"(defprotocol Area (area [s]))
       (defrecord Rect [w h] Area (area [_] (* w h)))
       (defn twice [s] (* 2 (area s)))
       (defn any-twice [s] (* 2 (area s)))
       (pr-str [(twice (->Rect 2 3)) (any-twice (->Rect 1 1))
                (any-twice (reify Area (area [_] 5)))])"#;
    let js = every_level(source, "[12 2 10]");
    assert!(js.contains(")[\"cljs$user$Area$area$arity$1\"](l"), "{js}");
    let stats = prepared(source, OptimizationLevel::Advanced)
        .stats()
        .clone();
    assert_eq!(stats.devirtualized_calls, 1);
}

/// Since light-cljs-253 a method of a `deftype`, `defrecord`, `reify`,
/// `extend-type` of a program type or `specify!` that implements a program
/// protocol joins the arguments of the protocol method's calls, when the
/// program makes every call of the method: its global is read only as a
/// callee, and nothing names its slots.
#[test]
fn protocol_method_parameters_join_visible_calls() {
    let [nil, boolean, number, string, keyword] = [
        Type::NIL,
        Type::BOOLEAN,
        Type::NUMBER,
        Type::STRING,
        Type::KEYWORD,
    ]
    .map(Inferred::of);
    // The failure scenario of the pellet: every call supplies a boolean.
    let single = "(defprotocol P (m [this flag]))
                  (deftype T [] P (m [_ flag] (if flag 1 0)))
                  (m (T.) true)";
    assert_eq!(local(single, "flag"), boolean);
    // The receiver is what the calls pass, here an instance of `T`.
    let receiver = local(
        "(defprotocol P (m [this flag]))
         (deftype T [] P (m [self flag] self))
         (m (T.) true)",
        "self",
    );
    assert_eq!(receiver.ty, Type::OBJECT, "{receiver:?}");
    assert!(matches!(receiver.known, Known::Instance(_)), "{receiver:?}");
    let mixed = "(defprotocol P (m [this x]))
                 (deftype T [] P (m [_ flag] flag))
                 (defrecord U [] P (m [_ s] s))
                 (m (T.) true) (m (->U) \"s\")";
    let either = "(defprotocol P (m [this x]))
                  (deftype T [] P (m [_ flag] flag))
                  (deftype U [] P (m [_ s] s))
                  (m (T.) true) (m (U.) \"s\")
                  (m (if (odd? (count [1])) (T.) (U.)) 0)";
    for (source, name, expected) in [
        // Mixed callers: a method joins the calls whose receiver may be an
        // instance of its type, and a receiver of either type reaches both.
        (mixed, "flag", boolean),
        (mixed, "s", string),
        (either, "flag", boolean.union(number)),
        (either, "s", string.union(number)),
        // The dispatcher of a method with one arity passes nil for missing
        // arguments; with several, each implementation joins the calls of its
        // count.
        (
            "(defprotocol P (m [this x])) (deftype T [] P (m [_ x] x)) (m (T.))",
            "x",
            nil,
        ),
        (
            "(defprotocol P (m [this] [this x]))
             (deftype T [] P (m [_] :none) (m [_ x] x))
             (m (T.)) (m (T.) \"a\")",
            "x",
            string,
        ),
        // `reify`, `extend-type` of a program type and `specify!`; a `reify`
        // does not receive the calls on an instance of a program type.
        (
            "(defprotocol P (m [this x]))
             (deftype T [] P (m [_ y] y))
             (m (reify P (m [_ x] x)) :k) (m (T.) 1)",
            "x",
            keyword,
        ),
        (
            "(defprotocol P (m [this x])) (deftype T [])
             (extend-type T P (m [_ x] x)) (m (T.) :k)",
            "x",
            keyword,
        ),
        (
            "(defprotocol P (m [this x]))
             (m (specify! (js-obj) P (m [_ x] x)) :k)",
            "x",
            keyword,
        ),
        // Calls inside methods are calls too.
        (
            "(defprotocol P (m [this x]) (n [this y]))
             (deftype T [] P (m [this x] (n this x)) (n [_ y] y))
             (m (T.) :k)",
            "y",
            keyword,
        ),
    ] {
        assert_eq!(local(source, name), expected, "{source}");
    }
    // Every other way to reach a method keeps `any`: the protocol method as a
    // value (`apply`, `mapv`, a `def`), its slot named as a method, a string
    // or in `js*`, an assignment of the method (`with-redefs`), a variadic
    // signature, an `extend-type` whose value (its last method) is used, an
    // `extend-type` of a host type, which the dispatcher reaches by the
    // receiver's host type, a core protocol method and a method that reads
    // `(js-arguments)`.
    let protocol = "(defprotocol P (m [this x])) (deftype T [] P (m [_ x] x)) (m (T.) 1)";
    for source in [
        format!("{protocol} (apply m (T.) [\"a\"])"),
        format!("{protocol} (mapv m [(T.)])"),
        format!("{protocol} (def g m)"),
        format!("{protocol} (.cljs$user$P$m$arity$2 (T.) (T.) \"a\")"),
        format!("{protocol} (aget (T.) \"cljs$user$P$m$arity$2\")"),
        format!("{protocol} (js* \"~{{}}.cljs$user$P$m$arity$2(~{{}}, 'a')\" (T.) (T.))"),
        format!("{protocol} (with-redefs [m (fn [_ x] x)] (m (T.) 1))"),
        "(defprotocol P (m [this x])) (deftype T [])
         (def e (extend-type T P (m [_ x] x))) (m (T.) 1)"
            .to_owned(),
        "(defprotocol P (m [this x])) (extend-type string P (m [_ x] x)) (m \"s\" 1)".to_owned(),
        "(defprotocol P (m [this x]))
         (deftype T [] P (m [_ x] (aset (js-arguments) 1 \"a\") x)) (m (T.) 1)"
            .to_owned(),
    ] {
        assert_eq!(local(&source, "x"), Inferred::ANY, "{source}");
    }
    assert_eq!(
        local(
            "(defprotocol P (m [this & more])) (deftype T [] P (m [_ a more] more)) (m (T.) 1 2)",
            "more"
        ),
        Inferred::ANY
    );
    assert_eq!(
        local(
            "(deftype T [] ILookup (-lookup [_ k] k) (-lookup [_ k nf] nf)) (get (T.) :a)",
            "k"
        ),
        Inferred::ANY
    );
}

#[test]
fn typed_lowerings_apply_inside_protocol_methods_and_keep_the_results() {
    // Every call passes booleans or nil and instances of one type: the
    // methods test truthiness in JavaScript and read the record's field
    // through the receiver.
    let js = every_level(
        r#"(defprotocol Shape (label [this flag]) (scaled [this k] [this k nf]))
           (defrecord Circle [r] Shape
             (label [this flag] (if flag (:r this) :hidden))
             (scaled [this k] (* k (:r this)))
             (scaled [this k nf] (if (pos? k) (* k (:r this)) nf)))
           (deftype Square [s] Shape
             (label [_ flag] (if flag s :hidden))
             (scaled [_ k] (* k s))
             (scaled [_ k nf] (if (pos? k) (* k s) nf)))
           (pr-str [(label (->Circle 2) true) (label (map->Circle {:r 3}) false)
                    (label (Square. 4) (< 1 2)) (label (Square. 5) nil)
                    (scaled (->Circle 2) 3) (scaled (Square. 2) -0.5)
                    (scaled (->Circle 2) 0 :nf) (scaled (Square. 1) 2 nil)])"#,
        "[2 :hidden 4 :hidden 6 -1 :nf 2]",
    );
    for lowered in ["if (l", ")[\"r\"]"] {
        assert!(js.contains(lowered), "{lowered}: {js}");
    }
    assert!(!js.contains("truth("), "{js}");
    // Adversarial callers keep the runtime test, each in a program of its
    // own: a receiver of either type, `specify!` on an instance, an
    // `extend-type` whose value is called, and calls through the slot's name.
    let protocol = "(defprotocol P (m [this flag]))";
    for (source, expected) in [
        (
            r#"(deftype T [] P (m [_ flag] (if flag :yes :no)))
               (deftype U [] P (m [_ flag] (if flag :yes :no)))
               (pr-str [(m (T.) true) (m (T.) false) (m (U.) false)
                        (m (if (odd? (count [1])) (T.) (U.)) (/ 0 0))])"#,
            "[:yes :no :no :yes]",
        ),
        (
            r#"(deftype T [] P (m [_ flag] (if flag :yes :no)))
               (def t (T.))
               (specify! t P (m [_ flag] (if flag :s :not-s)))
               (pr-str [(m (T.) false) (m t false) (m t 0)])"#,
            "[:no :not-s :s]",
        ),
        (
            r#"(deftype V [])
               (def e (extend-type V P (m [_ flag] (if flag :y :n))))
               (pr-str [(m (V.) false) (e nil "")])"#,
            "[:n :y]",
        ),
        (
            r#"(deftype W [] P (m [_ flag] (if flag :yes :no)))
               (def w (W.))
               (pr-str [(m w false) (.cljs$user$P$m$arity$2 w w "")
                        ((aget w "cljs$user$P$m$arity$2") w 0)])"#,
            "[:no :yes :yes]",
        ),
        (
            r#"(deftype X [] P (m [_ flag] (if flag :yes :no)))
               (pr-str [(m (X.) false) (apply m (X.) [""]) (mapv m [(X.)] [0])])"#,
            "[:no :yes [:yes]]",
        ),
    ] {
        every_level(&format!("{protocol} {source}"), expected);
    }
}

/// A linear congruential generator, so that every seed gives one program.
struct Rng(u64);

impl Rng {
    fn choose(&mut self, count: usize) -> usize {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 33) % count as u64) as usize
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.choose(items.len())]
    }
}

/// Adversarial arguments by class. A function's calls often draw a parameter
/// from one class, so that the lowerings apply, and otherwise from any.
const ARGUMENTS: [&[&str]; 9] = [
    &["0", "-0.0", "1.5", "-2", "(/ 0 0)", "(/ 1 0)", "3"],
    &[
        "\"\"",
        "\"ab\"",
        "\"1\"",
        "(str :k)",
        "(str (js/String.fromCodePoint 128512))",
    ],
    &["nil", "js/undefined"],
    &["true", "false", "(< 1 2)"],
    &[
        ":k",
        "(keyword \"k\")",
        ":ns/k",
        "(keyword \"ns\" \"k\")",
        "(keyword \"ns/k\")",
        ":x",
    ],
    &["[1 2 3]", "[]", "(vector :a)", "[nil 2]", "[\"ab\" :k]"],
    &[
        "(subvec [1 2 3] 1)",
        "(first {:a 1})",
        "{:x 1 :a 2}",
        "#{1}",
        "'(1 2)",
        "(range 3)",
        "(seq [1])",
    ],
    &["(->R 1 2)", "(map->R {:x 5 :a 6})", "(->S 3)", "(T. 4)"],
    &[
        "#js [1 2]",
        "#js {:x 1}",
        "(js/BigInt 1)",
        "(js/Date. 0)",
        "'k",
    ],
];

/// Bodies over the parameters `x` and `y`: every typed lowering, with tests
/// that narrow, and calls of protocol methods. (`identical?` compares a
/// parameter with itself: equal constant literals may be one pooled value at
/// `Advanced`, see "Advanced-mode contract".)
const BODIES: [&str; 26] = [
    "(count x)",
    "(nth x 0)",
    "(nth x y)",
    "(nth x 1 :nf)",
    "(= x y)",
    "(= x :k)",
    "(= :k x)",
    "(not= x \"ab\")",
    "(:x x)",
    "(get x :a)",
    "(if x :t :f)",
    "(not x)",
    "(and x y)",
    "(or x y)",
    "(+ x 1)",
    "(< x y)",
    "(inc x)",
    "(== x y)",
    "(seq x)",
    "(when (string? x) (count x))",
    "(if (vector? x) (nth x 0 :e) :nv)",
    "(if (instance? R x) (:x x) :nr)",
    "(cond (nil? x) :nil (keyword? x) (= x :k) :else (count (str x)))",
    "[(identical? x x) (zero? x) (not= x y)]",
    "(pm x)",
    "(if (satisfies? Pm x) (pm x) :np)",
];

/// A program of closed functions, some with callers inference cannot see,
/// whose calls pass adversarial arguments; it prints every result.
fn closed_functions_program(seed: u64) -> String {
    let mut rng = Rng(seed);
    // A program type implementing `IEquiv` keeps `=` with a literal second
    // unlowered, on some seeds.
    let equiv = if rng.choose(3) == 0 {
        "IEquiv (-equiv [_ o] (= o :k))"
    } else {
        ""
    };
    let mut source = format!(
        "(defprotocol Pm (pm [o]))
         (defrecord R [x a] Pm (pm [_] [:r x]))
         (defrecord S [x] ILookup (-lookup [_ k] [:own k]) (-lookup [_ k nf] [:own k nf]))
         (deftype T [x] {equiv})
         (extend-type T Pm (pm [_] :t))\n"
    );
    let mut callable: Vec<String> = Vec::new();
    let mut results = Vec::new();
    for index in 0..10 {
        let name = format!("f{index}");
        let mut body = rng.pick(&BODIES).to_owned();
        if !callable.is_empty() && rng.choose(4) == 0 {
            let callee = callable[rng.choose(callable.len())].clone();
            body = format!("[{body} ({callee} y x)]");
        }
        let body = format!("(try {body} (catch :default e [:threw (.-message e)]))");
        let shape = rng.choose(4);
        let classes = [rng.choose(ARGUMENTS.len()), rng.choose(ARGUMENTS.len())];
        let mode = rng.choose(3);
        let argument = |rng: &mut Rng, position: usize| {
            let class = match (mode, position) {
                (0, _) | (2, 0) => classes[position],
                _ => rng.choose(ARGUMENTS.len()),
            };
            rng.pick(ARGUMENTS[class]).to_owned()
        };
        let mut calls = Vec::new();
        for _ in 0..1 + rng.choose(4) {
            let count = match shape {
                0 if rng.choose(6) == 0 => 1,
                1 | 2 => 1 + rng.choose(if shape == 2 { 3 } else { 2 }),
                _ => 2,
            };
            let arguments = (0..count)
                .map(|position| argument(&mut rng, position.min(1)))
                .collect::<Vec<_>>();
            let callee = if shape == 3 { "g" } else { &name };
            calls.push(format!("({callee} {})", arguments.join(" ")));
        }
        // Callers inference cannot see make the function escape.
        let (a, b) = (argument(&mut rng, 0), argument(&mut rng, 1));
        let callee = if shape == 3 { "g" } else { &name };
        match rng.choose(10) {
            0 => calls.push(format!("(apply {callee} [{a} {b}])")),
            1 => calls.push(format!("(mapv {callee} [{a}] [{b}])")),
            2 => calls.push(format!("(mapv #({callee} % {b}) [{a}])")),
            _ => (),
        }
        let calls = calls.join(" ");
        match shape {
            0 => source.push_str(&format!("(defn {name} [x y] {body})\n")),
            1 => {
                let default = argument(&mut rng, 1);
                source.push_str(&format!(
                    "(defn {name} ([x] ({name} x {default})) ([x y] {body}))\n"
                ));
            }
            2 => source.push_str(&format!(
                "(defn {name} [x & more] (let [y (first more)] {body}))\n"
            )),
            _ => {
                source.push_str(&format!(
                    "(def {name} (let [g (fn [x y] {body})] [{calls}]))\n"
                ));
                results.push(name);
                continue;
            }
        }
        callable.push(name);
        results.push(format!("[{calls}]"));
    }
    source.push_str(&format!("(pr-str [{}])", results.join("\n ")));
    source
}

/// The program prints at every level what it prints at `None`; returns the
/// program part of the readable `Advanced` output and its statistics.
fn same_at_every_level(source: &str) -> (String, ApplicationStats) {
    let program = program(source);
    let printed = OptimizationLevel::ALL.map(|level| {
        let js = prepare_application(&program, ApplicationOptions::new(level)).emit_js();
        execute(&format!("console.log({js});"))
    });
    for (level, output) in OptimizationLevel::ALL.into_iter().zip(&printed) {
        assert_eq!(output, &printed[0], "at {level}:\n{source}");
    }
    let advanced = prepare_application(&program, readable(OptimizationLevel::Advanced));
    (
        program_text(&advanced.emit_js()).to_owned(),
        advanced.stats().clone(),
    )
}

#[test]
fn generated_closed_functions_print_the_same_at_every_level() {
    // `LIGHT_CLJS_INFERENCE_SEEDS=<n>` runs more seeds.
    let seeds = std::env::var("LIGHT_CLJS_INFERENCE_SEEDS")
        .map_or(16, |count| count.parse().expect("a seed count"));
    let mut texts = String::new();
    let mut typed = 0;
    for seed in 0..seeds {
        let (text, stats) = same_at_every_level(&closed_functions_program(seed));
        texts.push_str(&text);
        typed += stats.typed_parameters;
    }
    // The lowerings apply inside the generated functions.
    assert!(typed > 0);
    for lowered in [
        "(l",
        ").length",
        "rt$vectorNth(",
        " === ",
        "rt$keywordFqn(l",
        ")[\"x\"]",
        ")[\"cljs$user$Pm$pm$arity$1\"](l",
    ] {
        assert!(texts.contains(lowered), "{lowered}");
    }
}

#[test]
fn generated_namespace_programs_type_parameters() {
    // The generated programs of `generated_optimization`, which checks them at
    // every level, have closed functions whose parameters are typed.
    let typed: usize = SEEDS
        .into_iter()
        .map(|seed| {
            let program = namespace_program(seed);
            let analyzed = analyze_generated(&program, &program.sources(&|_| false, false));
            prepare_application(
                &analyzed,
                ApplicationOptions::new(OptimizationLevel::Advanced),
            )
            .stats()
            .typed_parameters
        })
        .sum();
    assert!(typed > 0);
}

#[test]
fn function_parameters_are_called_directly_unless_they_may_be_nil() {
    // A parameter that only ever holds one `fn` is called as that `fn`'s
    // method; one that may also be nil keeps `.call(null, ...)`, whose
    // `TypeError` is the one lower levels throw.
    let (js, _) = same_at_every_level(
        r#"(defn twice [f x] (f (f x)))
           (defn run [g] (try (g 1) (catch :default e [:threw (.-message e)])))
           (pr-str [(twice (fn [y] (* 2 y)) 3) (run (fn [y] (inc y))) (run nil)])"#,
    );
    assert!(!js.contains(".call(null, (l"), "{js}");
    assert!(js.contains(".call(null, 1)"), "{js}");
}
