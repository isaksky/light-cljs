//! Public parsing, scalar, random and array contracts in canonical JS phases.
use light_cljs::evaluator::{Evaluator, EvaluatorOptions, RUNTIME_SOURCE};
use light_cljs::{MacroHost, analyze_with_macros, emitter::emit_js_unit, parse};
use std::process::Command;

fn both(source: &str, expected: &str) {
    let p = analyze_with_macros(source, &parse(source).unwrap(), &mut MacroHost::new())
        .unwrap_or_else(|e| panic!("{source}: {e}"));
    let e = Evaluator::new(EvaluatorOptions::default()).unwrap();
    assert_eq!(
        e.print(e.evaluate(&p).unwrap()).unwrap(),
        expected,
        "QuickJS: {source}"
    );
    let out = Command::new("node")
        .args([
            "-e",
            &format!(
                "const rt = {RUNTIME_SOURCE}; console.log(rt.core.prStr(({})(rt)));",
                emit_js_unit(&p)
            ),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8(out.stdout).unwrap().trim(),
        expected,
        "Node: {source}"
    );
}

#[test]
fn public_utilities_execute_in_applications_and_macros() {
    both(
        r#"
      (defn witness []
        (let [events (atom []) old js/Math.random]
          (try
            (set! js/Math.random (fn [] (swap! events conj :random) 0.75))
            (assert (= [0.75 3 3 :d :d] [(rand) (rand 4) (rand-int 4)
                                        (rand-nth [:a :b :c :d]) (rand-nth '(:a :b :c :d))]))
            (assert (= [:random :random :random :random :random] @events))
            (set! js/Math.random (fn [] 0))
            (assert (= [0 0 :a nil] [(rand) (rand-int 4) (rand-nth [:a]) (rand-nth nil)]))
            (assert (try (rand-nth []) false (catch :default _ true)))
            (finally (set! js/Math.random old))))
        (assert (= [100 -42 77 nil nil nil] (mapv parse-long ["+100" "-42" "077" "1.2" "0x10" "9007199254740992"])))
        (assert (= [0.5 -125 2 ##Inf ##-Inf nil nil] (mapv parse-double [".5" "-1.25e2" " 2F " "Infinity" "-Infinity" "0x1" "\u00a02"])))
        (assert (js/isNaN (parse-double " +NaN\n")))
        (assert (js/Object.is (js* "-0") (parse-long "-0")))
        (assert (js/Object.is (js* "-0") (parse-double "-0.0")))
        (assert (= [true false nil nil] (mapv parse-boolean ["true" "false" "TRUE" " true "])))
        (assert (= "123e4567-e89b-12d3-a456-426614174000" (str (parse-uuid "123E4567-E89B-12D3-A456-426614174000"))))
        (assert (nil? (parse-uuid "123e4567-eg9b-12d3-a456-426614174000")))
        (assert (nil? (parse-uuid "123e4567-e89b-12d3-a456-426614174000\n")))
        (assert (nil? (parse-long "1\n")))
        (doseq [f [parse-long parse-double parse-uuid parse-boolean] x [nil false 1 :x #js []]]
          (assert (try (f x) false (catch js/Error e (.startsWith (.-message e) "Expected string, got: ")))))
        (assert (= [true true false false] [(js-symbol? (js/Symbol "x")) (js-symbol? (js/Object (js/Symbol "x"))) (js-symbol? 'x) (js-symbol? nil)]))
        (assert (= [true true false false false] (mapv special-symbol? ['if 'fn* 'fn 'cljs.core/if :if])))
        (assert (= ["bc" "bc" "" "a" "ab"] [(subs "abc" 1) (subs "abc" 3 1) (subs "abc" 9) (subs "abc" -2 1) (subs "abc" 0 2.9)]))
        (assert (= [55357 56832] [(.charCodeAt (subs "😀" 0 1) 0) (.charCodeAt (subs "😀" 1) 0)]))
        (assert (= ["foo" "foo" "ns" nil] [(name "foo") (name :ns/foo) (namespace 'ns/foo) (namespace :foo)]))
        (assert (= [[1 2] [] [3]] (mapv vec (to-array-2d [[1 2] nil '(3)]))))
        (doseq [f [int-array long-array double-array object-array]]
          (assert (= [nil nil] (vec (f 2))))
          (assert (= [1 "x" nil] (vec (f [1 "x" nil]))))
          (assert (= [1 "x" nil] (vec (f 3 '(1 "x" nil 9)))))
          (assert (= [1 nil nil] (vec (f 3 '(1)))))
          (let [v [1 2] a (f 2 v)] (assert (identical? v (aget a 0))) (assert (identical? v (aget a 1))))
          (assert (= [] (vec (f 0))))
          (assert (try (f) false (catch :default _ true)))
          (assert (try (f 1 2 3) false (catch :default _ true))))
        (assert (not (identical? int-array long-array)))
        (let [v (with-meta [1 2] {:m 1}) c (clone v)]
          (assert (= v c)) (assert (not (identical? v c))) (assert (= {:m 1} (meta c))))
        (let [x (reify ICloneable (-clone [_] :copy))] (assert (= :copy (clone x))))
        (assert (= ["" "ab" "ab"] [(str) (str "ab") (str "a" nil "b")]))
        (let [metas [(meta #'rand-int) (meta #'rand-nth) (meta #'parse-long)
                     (meta #'parse-double) (meta #'parse-uuid) (meta #'parse-boolean)
                     (meta #'js-symbol?) (meta #'special-symbol?) (meta #'to-array-2d)
                     (meta #'int-array) (meta #'long-array) (meta #'double-array)
                     (meta #'object-array) (meta #'clone)]]
          (assert (= '[rand-int rand-nth parse-long parse-double parse-uuid parse-boolean
                       js-symbol? special-symbol? to-array-2d int-array long-array double-array object-array clone]
                     (mapv :name metas)))
          (assert (every? #(and (string? (:doc %)) (number? (:line %)) (seq (:arglists %))) metas)))
        true)
      (defmacro phase-witness [] (witness))
      [(witness) (phase-witness)]
    "#,
        "[true true]",
    );
}

#[test]
fn array_initializers_preserve_lazy_demand_holes_and_receiver_effects() {
    both(
        r#"
      (defn array-witness []
        (let [events (atom [])]
          (letfn [(items [n] (lazy-seq (swap! events conj n) (cons n (items (inc n)))))]
            (let [a (int-array 2 (items 0)) before @events
                  z (object-array 0 (items 10))
                  b (long-array 4 '(7 8))]
              [before @events (vec a) (vec z) (mapv #(if (nil? %) nil %) b)
               (js* "Object.hasOwn(~{}, 2)" b)]))))
      (defmacro array-phase [] (array-witness))
      [(array-witness) (array-phase)]
    "#,
        "[[[0 1 2] [0 1 2 10] [0 1] [] [7 8 nil nil] false] [[0 1 2] [0 1 2 10] [0 1] [] [7 8 nil nil] false]]",
    );
    both(
        r#"
      (let [events (atom []) s #js {}]
        (aset s "substring" (fn [& xs] (this-as self (swap! events conj (identical? self s)) (vec xs))))
        (let [f subs] [(f s 1 2) @events]))
    "#,
        "[[1 2] [true]]",
    );
}

#[test]
fn scalar_conversions_saved_functions_and_metadata_remain_public() {
    both(
        r#"
      (let [i int l long d double f float c char]
        [(i 2147483648) (i ##Inf) (l -1.9) (l ##Inf) (js/isNaN (l ##NaN))
         (js/Object.is (js* "-0") (l (js* "-0"))) (d "x") (f 1.2) (c 65)
         (str (:arglists (meta #'parse-long))) (str (:arglists (meta #'int-array)))
         (str (:arglists (meta #'clone)))])
    "#,
        "[-2147483648 0 -1 ##Inf true true \"x\" 1.2 \"A\" \"([s])\" \"([size-or-seq] [size init-val-or-seq])\" \"([value])\"]",
    );
}

#[test]
fn seeded_number_parse_roundtrips_cover_one_hundred_thousand_inputs_in_both_phases() {
    both(
        r#"
      (defn property-witness []
        (let [seed (atom 123456789)]
          (dotimes [i 100000]
            (let [s (swap! seed #(js* "(Math.imul(~{}, 1664525) + 1013904223) >>> 0" %))
                  integer (js* "Math.floor((~{} / 4294967296) * 9007199254740991) * (~{} % 2 ? -1 : 1)" s i)
                  floating (js* "(~{} / 4294967296) * Math.pow(10, (~{} % 634) - 324) * (~{} % 2 ? -1 : 1)" s i i)]
              (assert (= integer (parse-long (str integer))))
              (assert (= floating (parse-double (str floating))))))
          (assert (js/isNaN (parse-double (str ##NaN)))))
        true)
      (defmacro property-phase [] (property-witness))
      [(property-witness) (property-phase)]
    "#,
        "[true true]",
    );
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn utility_test_bodies() {
        run(&[
            Case::expression(
                "test-parse-uuid",
                include_str!("../fixtures/phase-oracles/utilities-completion/test-parse-uuid.cljs"),
            ),
            Case::expression(
                "checked-aget-test",
                include_str!(
                    "../fixtures/phase-oracles/utilities-completion/checked-aget-test.cljs"
                ),
            ),
            Case::expression(
                "checked-aset-test",
                include_str!(
                    "../fixtures/phase-oracles/utilities-completion/checked-aset-test.cljs"
                ),
            ),
            Case::expression(
                "unchecked-aget-test",
                include_str!(
                    "../fixtures/phase-oracles/utilities-completion/unchecked-aget-test.cljs"
                ),
            ),
            Case::expression(
                "unchecked-set-test",
                include_str!(
                    "../fixtures/phase-oracles/utilities-completion/unchecked-set-test.cljs"
                ),
            ),
            Case::expression(
                "uri-predicate",
                include_str!("../fixtures/phase-oracles/utilities-completion/uri-predicate.cljs"),
            ),
            Case::observed_during_expansion(
                "test-make-array",
                include_str!("../fixtures/phase-oracles/utilities-completion/test-make-array.cljs"),
                r#"{"id":"test-make-array","outcome":{"status":"returned","value":true},"trace":[],"stdout":""}"#,
            ),
        ]);
    }
}
