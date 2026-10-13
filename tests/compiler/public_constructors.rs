//! Public positional construction in both canonical execution phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn both(source: &str, expected: &str) {
    let program = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        evaluator
            .print(evaluator.evaluate(&program).unwrap())
            .unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let output = Command::new("node")
        .args([
            "-e",
            &format!(
                "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
                emit_js_unit(&program)
            ),
        ])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8(output.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}

#[test]
fn public_constructor_api_arities_and_macro_phase_execute() {
    for row in include_str!("../fixtures/compiler-public-constructors.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        both(fields[1], fields[2]);
        // Macro values are persistent booleans; intermediate types, markers and
        // saved protocol methods execute in the canonical macro runtime.
        both(
            &format!("(defmacro witness [] {}) (witness)", fields[1]),
            fields[2],
        );
    }
    for row in include_str!("../fixtures/compiler-public-constructor-bodies.tsv")
        .lines()
        .filter(|r| !r.starts_with('#') && !r.is_empty())
    {
        let fields: Vec<_> = row.split('\t').collect();
        both(fields[1], fields[2]);
        both(
            &format!("(defmacro upstream [] {}) (upstream)", fields[1]),
            fields[2],
        );
    }
    both(
        r#"(let [vs [#'->Cons #'->Reduced #'->Symbol]]
      (assert (= '[->Cons ->Reduced ->Symbol] (mapv #(-> % meta :name) vs)))
      (assert (every? #(= 'cljs.core (-> % meta :ns)) vs))
      (assert (= '([meta first rest __hash]) (:arglists (meta #'->Cons))))
      (assert (= '([val]) (:arglists (meta #'->Reduced))))
      (assert (= '([ns name str _hash _meta]) (:arglists (meta #'->Symbol)))) true)"#,
        "true",
    );
    both(
        r#"
      (def counter (atom 0))
      (defn witness []
        (let [n (swap! counter inc) c (->Cons nil n nil nil)
              r (apply ->Reduced [n]) s (->Symbol nil "foo" "foo" -1 nil)]
          [(first c) @r (name s)]))
      (defmacro phase [] (witness))
      [(phase) @counter (witness) @counter (phase) @counter]
    "#,
        "[[1 1 \"foo\"] 0 [1 1 \"foo\"] 1 [2 2 \"foo\"] 1]",
    );
    both(
        "(defmacro form [] (Cons. nil '+ (list 1 2) nil)) (form)",
        "3",
    );
    both(
        "(defmacro form [] (list 'quote (->Symbol nil \"foo\" \"foo\" -1 nil))) (form)",
        "foo",
    );
}

#[test]
fn unsupported_constructor_results_keep_expansion_diagnostics_and_recover() {
    let mut host = MacroHost::new();
    for expression in [
        "(Reduced. 42)",
        "(->Reduced 42)",
        "Cons",
        "->Symbol",
        "PersistentVector",
        "->PersistentHashMap",
        "Cycle",
        "->Iterate",
    ] {
        let source = format!("(defmacro bad [] {expression}) (bad)");
        let error = analyze_with_macros(&source, &parse(&source).unwrap(), &mut host)
            .unwrap_err()
            .to_string();
        assert!(
            error.contains("Unsupported macro result") && error.contains("bad"),
            "{error}"
        );
        let source = "(defmacro good [] (inst-ms #inst \"1970-01-01T00:00:00.042Z\")) (good)";
        let program = analyze_with_macros(source, &parse(source).unwrap(), &mut host).unwrap();
        let evaluator = Evaluator::new(EvaluatorOptions::default()).unwrap();
        assert_eq!(
            evaluator
                .print(evaluator.evaluate(&program).unwrap())
                .unwrap(),
            "42"
        );
    }
}

#[test]
fn constructors_report_invalid_arities_and_tail_arguments_without_overflow() {
    both(
        r#"(let [message (fn [f] (try (f) "missing error" (catch js/Error e (.-message e))))]
      [(message #(->Cons nil 1)) (message #(apply ->Reduced []))
       (message #(Symbol. nil "foo" "foo" -1 nil 9))
       (message #(next (Cons. nil 1 42 nil)))])"#,
        "[\"Invalid arity: 2\" \"Invalid arity: 0\" \"Invalid arity: 6\" \"42 is not ISeqable\"]",
    );
}

#[test]
fn collection_constructor_phase_roots_and_bounded_repetition() {
    both(
        r#"(def counter (atom 0))
        (defn witness []
          (let [n (swap! counter inc) x (->List nil n nil 1 nil)
                k (apply ->Keyword [nil "key" "key" nil])
                q (->PersistentQueue nil 1 x [] nil)]
            [(first x) (get (->PersistentArrayMap nil 1 #js [k n] nil) k) (peek q)]))
        (defmacro phase [] (witness))
        [(phase) @counter (witness) @counter (phase) @counter]"#,
        "[[1 1 1] 0 [1 1 1] 1 [2 2 2] 1]",
    );
    both(
        "(defmacro form [] (->List nil '+ (list 1 2) 3 nil)) (form)",
        "3",
    );
    both(
        r#"(loop [n 1000 total 0]
          (if (zero? n) total
            (let [x (PersistentVector. nil 3 5 nil #js [1 2 3] nil)
                  s (->Subvec nil x 1 3 nil)
                  m (apply ->PersistentArrayMap [nil 1 #js [:a n] nil])
                  l (List. nil n nil 1 nil)]
              (recur (dec n) (+ total (count l) (reduce + s) (get m :a))))))"#,
        "506500",
    );
}

#[test]
fn explicit_collection_slots_and_protocol_membership_survive_construction() {
    let source = r#"(let [c (Cycle. nil (list 1 2) nil (list 1 2) nil)
          i (Iterate. nil inc 40 7 nil) r (Repeat. nil 2 9 nil nil)
          v (PersistentVector. nil 1 5 nil #js [1 2] nil)
          h (hash-map :a 1 :b 2)
          copy (PersistentHashMap. nil 2 (.-root h) false 99 nil)]
        (assert (not (satisfies? IHash c))) (assert (not (satisfies? IEquiv c)))
        (assert (not (satisfies? IStack c))) (assert (not (satisfies? IIterable c)))
        (assert (not (satisfies? IHash i))) (assert (not (satisfies? IEquiv i)))
        (assert (not (satisfies? IStack r))) (assert (not (satisfies? IIterable r)))
        (assert (number? (hash c))) (assert (number? (hash i)))
        (assert (satisfies? ASeq (ChunkedSeq. [1 2] #js [1 2] 0 0 nil nil)))
        (assert (= 1 (count v))) (assert (= [1 2] (vec (seq v))))
        (assert (= 1 (reduce + v))) (assert (= 3 (reduce + 0 v)))
        (assert (= -2 (count (Subvec. nil [0 1 2 3] 3 1 nil))))
        (assert (= h copy)) (assert (= 99 (.-nil-val copy)))
        (assert (= #{:a :b} (PersistentHashSet. nil h nil))) true)"#;
    both(source, "true");
    both(&format!("(defmacro slots [] {source}) (slots)"), "true");
}
