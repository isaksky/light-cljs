//! Checkout map destructuring has explicit release/checkout differences (see
//! docs/compiler.md). These are regression probes,
//! not a claim that the pinned assertion inventory has executable witnesses.
use light_cljs::compile;
use std::process::Command;

fn check(source: &str) {
    let js = compile(source).unwrap_or_else(|e| panic!("{source}\n{e}"));
    let output = Command::new("node")
        .args(["--eval", &format!("console.log({js});")])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{source}\n{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&output.stdout),
        "true\n",
        "{source}"
    );
}

#[test]
fn required_keys_and_unbound_amp_keys() {
    for source in [
        "(let [{:keys! [a & :b]} {:a nil :b false}] (nil? a))",
        "(try (let [{:keys! [a & :b]} {:a 1}] false) (catch js/Error e (= (.-message e) \"Missing required key: :b\")))",
        "(let [{:foo/keys! [a & :b]} {:foo/a 1 :b 2}] (= a 1))",
        "(let [{:foo/syms! [a & 'b]} '{foo/a 1 b 2}] (= a 1))",
        "(let [{:strs! [a & \"b\"]} {\"a\" 1 \"b\" 2}] (= a 1))",
        "(let [{:keys! [a & 'b \"c\"]} {:a 1 'b 2 \"c\" 3}] (= a 1))",
    ] {
        check(source);
    }
    for source in [
        "(let [{:keys! [a & :b]} {:a 1 :b 2}] b)",
        "(let [{:syms! [a & 'b]} '{a 1 b 2}] b)",
        "(let [{:strs! [a & \"b\"]} {\"a\" 1 \"b\" 2}] b)",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
}

#[test]
fn recursive_select_defaults_excess_and_missing() {
    for source in [
        "(let [{:keys [a & :b] :keys! [c] :select s} {:a 1 :b 2 :c 3 :d 4}] (= s {:a 1 :b 2 :c 3}))",
        "(let [{{a :a :or {a 42}} :n :select s} nil] (= s {:n {:a 42}}))",
        "(let [{{a :a} :n :select s} nil] (nil? s))",
        "(let [{{a :a} :n :select s} {:n nil}] (= s {:n nil}))",
        "(let [{:keys [a z & :b] :or {:b 42 z 99} :select s :defaults d} {:a 1 :b nil}] (= [a z s d] [1 99 {:a 1 :b nil :z 99} {:b 42 :z 99}]))",
        "(let [{:syms [a z & 'b] :or {'b 42 z 99} :select s :defaults d} '{a 1 b nil}] (= [a z s d] [1 99 '{a 1 b nil z 99} '{b 42 z 99}]))",
        "(let [{:strs [a z & \"b\"] :or {\"b\" 42 z 99} :select s :defaults d} {\"a\" 1 \"b\" nil}] (= [a z s d] [1 99 {\"a\" 1 \"b\" nil \"z\" 99} {\"b\" 42 \"z\" 99}]))",
        "(let [{:keys [a] {:keys [x] :excess inner} :n :excess e} {:a 1 :b nil :n {:x 2 :z 3}}] (= [inner e] [{:z 3} {:b nil :n {:z 3}}]))",
        "(let [{:keys [a] :excess e} {:a nil}] (nil? e))",
        "(let [{:keys! [a b & :n] {:keys! [x y]} :n :missing m} {:a nil :b false :n {:x 1}}] (= m {:n {:y nil}}))",
        "(let [{:keys! [a b & :n] {:keys! [x y]} :n :missing m} {:a nil :b false}] (= m {:n {:x nil :y nil}}))",
        "(let [{:keys! [a b] :missing m} {:a nil :b false}] (nil? m))",
        "(let [{:defaults d :or {}} {}] (nil? d))",
    ] {
        check(source);
    }
}

#[test]
fn destructuring_all_binding_macro_contexts_and_recur() {
    for source in [
        "(= 3 ((fn [{:keys! [a & :b]}] a) {:a 3 :b 4}))",
        "(do (defn f [{:keys! [a] :missing m}] [a m]) (= [nil {:a nil}] (f {})))",
        "(= 3 ((fn ([{:keys! [a]}] a) ([{:keys! [a]} b] (+ a b))) {:a 1} 2))",
        "(= 3 ((fn [& {:keys! [a]}] a) :a 3))",
        "(= 3 ((fn [{:keys! [a]}] {:pre [(pos? a)] :post [(= % a)]} a) {:a 3}))",
        "(= 0 ((fn f [{:keys! [a]}] (if (zero? a) a (recur {:a (dec a)}))) {:a 3}))",
        "(= 6 (loop [{:keys! [a]} {:a 3} sum 0] (if (zero? a) sum (recur {:a (dec a)} (+ sum a)))))",
        "(= 3 (loop [{:keys! [a]} {:a 3} b a] b))",
        "(= 3 (if-let [{:keys! [a]} {:a 3}] a 0))",
        "(= [1 2] (vec (for [{:keys! [a]} [{:a 1} {:a 2}]] a)))",
        "(= 3 (letfn [(f [{:keys! [a]}] a)] (f {:a 3})))",
    ] {
        check(source);
    }
}

#[test]
fn malformed_extensions_are_compile_errors() {
    for source in [
        "(let [{:defaults d} {}] d)",
        "(let [{:defaults d :or {:a 1}} {}] d)",
        "(let [{:keys [a] :defaults d :or {:a 1 a 1}} {}] d)",
        "(let [{:keys! [a] :or {a 1}} {}] a)",
        "(let [{:keys! [a & b]} {}] a)",
        "(let [{:keys! [a & :b & :c]} {}] a)",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
}

#[test]
fn checkout_helpers_distinguish_absent_nil_and_false() {
    check("(= {:a false :b 0} (some-vals {:a false :b 0 :c nil}))");
    check("(nil? (some-vals {:a nil}))");
    check(
        "(and (nil? (req! {:a nil} :a)) (false? (req! {:a false} :a)) (try (req! {} :a) false (catch js/Error e true)))",
    );
}

#[test]
fn extension_effect_order_and_literal_provenance() {
    for source in [
        "(let [events (atom []) {:keys [a] :or {a (do (swap! events conj :default) 4)} :select s} (do (swap! events conj :map) {:a 1})] (= [a s @events] [1 {:a 1} [:default :map]]))",
        "(let [calls (atom 0) {:keys! [& (do (swap! calls inc) :x)]} {:x 1}] (= @calls 1))",
        "(let [calls (atom 0) {:keys! [& (do (swap! calls inc) :x)] :missing m} {:x false}] (= [@calls m] [1 nil]))",
        "(let [{:keys! [x]} {:x #js [1 2]}] (= 2 (alength x)))",
        "(let [{:keys! [x]} {:x #js {:a 1}}] (= 1 (.-a x)))",
        "(let [{:keys [a] :or {a 1} :all result} {:b false}] (= result {:a 1 :b false}))",
        "(do (defprotocol P (-f [this m])) (let [x (reify P (-f [_ {:keys! [a]}] a))] (= 3 (-f x {:a 3}))))",
        "(do (defprotocol P (-f [this m])) (defrecord R [offset] P (-f [_ {:keys! [a]}] (+ offset a))) (= 5 (-f (->R 2) {:a 3})))",
    ] {
        check(source);
    }
}

const ASSERTION_MACROS: &str = r#"
(ns destructure.test-macros)
(defmacro deftest [name & body] (concat (list 'do) body (list true)))
(defmacro testing [description & body] (cons 'do body))
(defmacro is [expression & message]
  (let [test (if (and (seq? expression) (= 'thrown? (first expression)))
               (list 'try (cons 'do (concat (drop 2 expression) (list false)))
                 (list 'catch (second expression) 'caught true))
               expression)]
    (list 'if test true (list 'throw (list 'new 'js/Error (str "Assertion failed: " expression))))))
(defmacro are [bindings predicate & values]
  (cons 'do
    (map (fn [row]
           (list 'let (vec (interleave bindings row))
             (list 'destructure.test-macros/is predicate)))
      (partition (count bindings) values))))
"#;

#[test]
fn complete_pinned_checkout_destructuring_bodies() {
    use light_cljs::{MacroHost, compile_with_macros};
    let mut host = MacroHost::new();
    for (name, body) in CHECKOUT_BODIES {
        // Keep every complete upstream body unchanged; first-party assertion
        // macros are now compiled source declarations in the same namespace.
        let macros = ASSERTION_MACROS
            .trim()
            .strip_prefix("(ns destructure.test-macros)")
            .unwrap()
            .replace("destructure.test-macros/is", "cljs.user/is");
        let source = format!("{macros}\n{body}");
        let js = compile_with_macros(&source, &mut host).unwrap_or_else(|e| panic!("{name}: {e}"));
        let output = Command::new("node")
            .args(["--eval", &format!("console.log({js});")])
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{name}: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8_lossy(&output.stdout), "true\n", "{name}");
    }
}

// Verbatim complete bodies at a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e.
// Copyright (c) Rich Hickey. All rights reserved.
// The use and distribution terms for this software are covered by the
// Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
// which can be found in the file epl-v10.html at the root of this distribution.
// By using this software in any fashion, you are agreeing to be bound by
// the terms of this license.
// You must not remove this notice, or any other, from this software.
const CHECKOUT_BODIES: &[(&str, &str)] = &[
    (
        "keys-bang",
        r####"(deftest keys-bang
  (let [sample-map {:a 1 :b 2}]
    (testing ":keys! happy path, binds and throws when key missing"
      (is (= 1 (let [{:keys! [a b]} sample-map] a)))
      (is (thrown? js/Error (let [{:keys! [a b]} {:a 1}] a))))
    (testing ":keys! with & bind and don't bind"
      (is (= 1 (let [{:keys! [a & :b]} sample-map] a)))
      (is (thrown? js/Error (let [{:keys! [a & :b]} {:a 1}] a))))
    (testing "nested maps with :keys! &"
      (let [sample-map {:a 1 :b {:a 2 :b 3 :c 4 :d 42}}
            {a :a {aa :a :as m :keys! [b c & :d]} :b} sample-map]
        (is (= m (:b sample-map)))
        (is (= a 1))
        (is (= aa 2))
        (is (= b 3))
        (is (= c 4))
        (is (thrown? js/Error (let [{a :a {aa :a :as m :keys! [b c & :d :e]} :b} sample-map] a)))
        (is (thrown? js/Error (let [{a :a {aa :a :as m :keys! [b c & :d]} :b} (update sample-map :b dissoc :c)] a)))))
    (testing "a broad range of qualified names/declarators with :keys! &"
      (let [sample-map {:foo/a 1 :b 2 :foo/c 3}
            {:keys! [foo/a & :b]} sample-map
            {:keys! [b & :foo/c]} sample-map
            sample-map2 {:foo/aa 1 :bb 2 :foo/cc 3}
            {:foo/keys! [aa & :foo/cc]} sample-map2]
        (is (= a 1))
        (is (= b 2))
        (is (thrown? js/Error (let [{:keys! [b & :foo/c]} (dissoc sample-map :b)] b)))
        (is (thrown? js/Error (let [{:keys! [b & :foo/c]} (dissoc sample-map :foo/c)] b)))
        (is (= 1 (let [{:foo/keys! [aa & :bb]} sample-map2] aa)))
        (is (= aa 1))
        (is (= 1 (let [{:keys! [::a & ::b]} {::a 1 , ::b 2}] a)))))))"####,
    ),
    (
        "syms-bang",
        r####"(deftest syms-bang
  (let [sample-map '{a 1 b 2}]
    (testing ":syms! happy path, binds and throws when key missing"
      (is (= 1 (let [{:syms! [a b]} sample-map] a)))
      (is (thrown? js/Error (let [{:syms! [a b]} {:a 1}] a))))
    (testing ":syms! with & bind and don't bind"
      (is (= 1 (let [{:syms! [a & 'b]} sample-map] a)))
      (is (thrown? js/Error (let [{:syms! [a & 'b]} {:a 1}] a))))
    (testing "nested maps with :syms! &"
      (let [sample-map '{a 1 b {a 2 b 3 c 4 d 42}}
            {a 'a {aa 'a :as m :syms! [b c & 'd]} 'b} sample-map]
        (is (= m ('b sample-map)))
        (is (= a 1))
        (is (= aa 2))
        (is (= b 3))
        (is (= c 4))
        (is (thrown? js/Error (let [{a 'a {aa :a :as m :syms! [b c & 'd 'e]} 'b} sample-map] a)))
        (is (thrown? js/Error (let [{a 'a {aa :a :as m :syms! [b c & 'd]} 'b} (update sample-map 'b dissoc 'c)] a)))))
    (testing "a broad range of qualified names/declarators with :syms! &"
      (let [sample-map '{foo/a 1 b 2 foo/c 3}
            {:syms! [foo/a & 'b]} sample-map
            {:syms! [b & 'foo/c]} sample-map
            sample-map2 '{foo/aa 1 bb 2 foo/cc 3}
            {:foo/syms! [aa & 'foo/cc]} sample-map2]
        (is (= a 1))
        (is (= b 2))
        (is (thrown? js/Error (let [{:syms! [b & 'foo/c]} (dissoc sample-map 'b)] b)))
        (is (thrown? js/Error (let [{:syms! [b & 'foo/c]} (dissoc sample-map 'foo/c)] b)))
        (is (= aa 1))
        (is (= 1 (let [{:foo/syms! [aa & 'bb]} sample-map2] aa)))))))"####,
    ),
    (
        "strs-bang",
        r####"(deftest strs-bang
  (let [sample-map {"a" 1 "b" 2}]
    (testing ":strs! happy path, binds and throws when key missing"
      (is (= 1 (let [{:strs! [a b]} sample-map] a)))
      (is (thrown? js/Error (let [{:strs! [a b]} {:a 1}] a))))
    (testing ":strs! with & bind and don't bind"
      (is (= 1 (let [{:strs! [a & "b"]} sample-map] a)))
      (is (thrown? js/Error (let [{:strs! [a & "b"]} {:a 1}] a))))
    (testing "nested maps with :strs! &"
      (let [sample-map {"a" 1 "b" {"a" 2 "b" 3 "c" 4 "d" 42}}
            {a "a" {aa "a" :as m :strs! [b c & "d"]} "b"} sample-map]
        (is (= m (get sample-map "b")))
        (is (= a 1))
        (is (= aa 2))
        (is (= b 3))
        (is (= c 4))
        (is (thrown? js/Error (let [{a "a" {aa "a" :as m :strs! [b c & "d" "e"]} "b"} sample-map] a)))
        (is (thrown? js/Error (let [{a "a" {aa "a" :as m :strs! [b c & "d"]} "b"} (update sample-map "b" dissoc "c")] a)))))))"####,
    ),
    (
        "select-directive",
        r####"(deftest select-directive
  (let [m {:a 1 :b 2 :c 3 :d 4
           'sa 10 'sb 20 'sc 30 'sd 40
           "stra" 100 "strb" 200 "strc" 300 "strd" 400
           :foo/x 1000 :foo/y 2000 :foo/z 3000
           ::x 10000 ::y 20000 ::z 30000
           :nested {:aa 1 'saa 10 "straa" 100}}

        {:keys [a b & :c :z]
         :keys! [d]
         :select keys-sel} m

        {:syms [sa sb & 'sc 'sz]
         :syms! [sd]
         :select syms-sel} m

        {:strs [stra strb & "strc" "strz"]
         :strs! [strd]
         :select strs-sel} m

        {:foo/keys [x & :y :zz]
         :foo/keys! [z]
         :select qkeys-sel} m

        {::keys [x & :y :zz]
         ::keys! [z]
         :select aqkeys-sel} m

        {{aa :aa saa 'saa
          :select nest-sel} :nested
         aqx ::x
         :select tl-sel} m

        {:keys! [a b & :c]
         :keys [d & :z]
         :or {}
         :select or-sel} m

        {:keys [a b c d]
         :syms [sa sb sc sd]
         :strs [stra strb strc strd]
         :foo/keys! [x y z]
         ::keys [x y z]
         nest :nested
         :as mm
         :select sel-mm} m]
    (are [expected result] (= expected result)
      keys-sel {:a 1 :b 2 :c 3 :d 4}
      syms-sel '{sa 10 sb 20 sc 30 sd 40}
      strs-sel {"stra" 100 "strb" 200 "strc" 300 "strd" 400}
      qkeys-sel {:foo/x 1000 :foo/z 3000}
      aqkeys-sel {::x 10000 ::z 30000}
      nest-sel '{:aa 1, saa 10}
      tl-sel '{:nested {:aa 1, saa 10} ::x 10000}
      or-sel {:a 1 :b 2 :c 3 :d 4}
      sel-mm mm))
  (testing "base cases"
    (is (nil? (let [{{a :a} :n :select s} nil] s))
        "if you haven't supplied a map, select won't make one for no reason")

    (testing "you get what you supplied if nothing else"
      (is (= {} (let [{{a :a} :n :select s} {}] s)))
      (is (= {:n nil} (let [{{a :a} :n :select s} {:n nil}] s)))
      (is (= {:n {}} (let [{{a :a} :n :select s} {:n {}}] s))))

    (testing "defaults can turn nothing into something"
      (is (= {:n {:a 42}} (let [{{a :a :or {a 42}} :n :select s} nil] s)))
      (is (= {:n {:a 42}} (let [{{a :a :or {a 42}} :n :select s} {:n nil}] s))))))"####,
    ),
    (
        "select-or-defaults",
        r####"(deftest select-or-defaults
  (let [sample-map {:a 1, :b 2, :c  {:aa 10 :bb 20},
                    'd 4  'e 5  'f  {'dd 40 'ee 50},
                    "g" 6 "h" 7 "i" {"gg" 60 "hh" 70},}]
    (testing "happy path"
      (testing ":defaults"
        (is (empty? (let [{:defaults d :or {}} {}] d)))
        
        (is (= {:a 1} (let [{:keys [a] :defaults d :or {:a 1}} {}] d)))
        (is (= {:a 1} (let [{:keys [a] :defaults d :or {a 1}} {}] d)))
        (is (= {:a 1, 'b 2, "c" 3} (let [{b 'b, c "c", :keys [a] :defaults d :or {:a 1, 'b 2, "c" 3}} {}] d))))
      
      (testing ":keys + :select + :or + defaults"
        (let [{:keys [a b z & :c :d] {:keys! [aa & :bb]} :c
               :or {:d 42, z :or-z}
               :select m
               :defaults dfs} sample-map]
          (is (= 1 a))
          (is (= 2 b))
          (is (= 10 aa))
          (is (= {:z :or-z, :c {:aa 10, :bb 20}, :b 2, :d 42, :a 1} m))
          (is (= {:d 42, :z :or-z} dfs))))

      (testing ":syms + :select + :or + defaults"
        (let [{:syms [d e z & 'd 'f] {:syms! [dd & 'ee]} 'f
               :or {'d 42, z :or-z}
               :select m
               :defaults dfs} sample-map]
          (is (= 4 d))
          (is (= 5 e))
          (is (= 40 dd))
          (is (= '{f {dd 40, ee 50}, e 5, d 4, z :or-z} m))
          (is (= '{d 42, z :or-z} dfs))))

      (testing ":strs + :select + :or + defaults"
        (let [{:strs [g h z & "d" "i"] {:strs! [gg & "hh"]} "i"
               :or {"d" 42, z :or-z}
               :select m
               :defaults dfs} sample-map]
          (is (= 6 g))
          (is (= 7 h))
          (is (= 60 gg))
          (is (= {"d" 42, "z" :or-z, "i" {"gg" 60, "hh" 70}, "g" 6, "h" 7} m))
          (is (= {"d" 42, "z" :or-z} dfs))))

      (testing "mixed things after &"
        (is (= 1 (let [{:keys [a & 'b]} {:a 1}] a)))
        (is (= 1 (let [{:keys! [a & 'b "c"]} {:a 1, 'b 2, "c" 3}] a)))))))"####,
    ),
    (
        "excess",
        r####"(deftest excess
  (let [sample-map {:a 1, :b 2, :c  {:aa 10},
                    'd 4  'e 5  'f  {'dd 40 'ee 50},
                    "g" 6 "h" 7 "i" {"gg" 60 "hh" 70}}]
    (testing "happy path"
      (let [{:keys [a] :excess exa} (select-keys sample-map [:a :b :c])
            {:keys [a b c] :excess exnil} (select-keys sample-map [:a :b :c])
            {{:excess exnest} :c} sample-map
            {:keys [a c] :excess exkws} (select-keys sample-map [:a :b :c])
            {:syms [d f] :excess exsyms} (select-keys sample-map '[d e f])
            {:strs [g i] :excess exstrs} (select-keys sample-map ["g" "h" "i"])]
        (is (= {:b 2 :c {:aa 10}} exa))
        (is (= {:aa 10} exnest))
        (is (= {:b 2} exkws))
        (is (= '{e 5} exsyms))
        (is (= {"h" 7} exstrs))

        (testing ":excess predicative use"
          (is (nil? exnil))
          (is (nil? (let [{:excess ex} {}] ex)))
          (is (nil? (let [{:keys [a] :excess ex} nil] ex)))

          (let [{:keys [a] :excess ex-some} (select-keys sample-map [:a :b :c])
                {:keys [a b c] :excess ex-none} (assoc (select-keys sample-map [:a :b :c]) :b nil)]
            (is (some-vals ex-some))
            (is (not (some-vals ex-none)))))))

    (testing ":excess retains nil values"
      (let [{:keys [a] :excess ex-nil1} (assoc (select-keys sample-map [:a :b :c]) :b nil)
            {{:keys [aa] :excess ex-nil2} :c} (assoc-in sample-map [:c :bb] nil)]
        (is (= {:b nil :c {:aa 10}} ex-nil1))
        (is (= {:bb nil} ex-nil2))))

    (testing ":excess and :or to ensure that defaults do not show up"
      (let [{:keys [a z] :or {z 99} :excess exor} (select-keys sample-map [:a :b :c])]
        (is (= {:b 2 :c {:aa 10}} exor))))

    (testing "nested :excess, also with &"
      (let [{:keys [a] {:keys [aa] :excess exc} :c} sample-map
            {:keys! [a & :b :c]
             :syms! [& 'd 'e 'f]
             :strs! [&  "g" "h" "i"]
             {:keys [zz] :or {:zz 999} :excess exinner1} :c
             {:syms [yy] :or {'yy 999} :excess exinner2} 'f
             {:strs [xx] :or {"xx" 999} :excess exinner3} "i"
             :excess exouter} sample-map]
        (is (nil? exc))
        (is (= {:aa 10} exinner1))
        (is (= {'dd 40 'ee 50} exinner2))
        (is (= {"gg" 60 "hh" 70} exinner3))
        (is (= '{:c {:aa 10}, f {dd 40, ee 50}, "i" {"gg" 60, "hh" 70}} exouter))))

    (testing ":excess with namespace-qualification"
      (let [nsmap {:foo/x 1000, :foo/y 2000, ::z 3000}
            {:foo/keys [x] :excess exfoo} nsmap
            {::keys [z] :excess exauto} nsmap
            {:foo/keys [x] ::keys [z] :excess exmix} nsmap
            {:foo/keys [x y] ::keys [z] :excess exall} nsmap]
        (is (= {:foo/y 2000 ::z 3000} exfoo))
        (is (= {:foo/x 1000 :foo/y 2000} exauto))
        (is (= {:foo/y 2000} exmix))
        (is (nil? exall))))))"####,
    ),
    (
        "missing-directive",
        r####"(deftest missing-directive
  (let [keys-map {:a 1 :b 2 :nest {:aa 10}}
        {:keys! [a b], :missing mkeys-nil} keys-map
        {:keys! [a b c], :missing mkeys-c} keys-map
        {:keys! [a b & :c], :missing mkeys-c&} keys-map
      
        syms-map '{a 1 b 2 nest {aa 10}}
        {:syms! [a b], :missing msyms-nil} syms-map
        {:syms! [a b c], :missing msyms-c} syms-map
        {:syms! [a b & 'c], :missing msyms-c&} syms-map

        strs-map {"a" 1 "b" 2 "nest" {"aa" 10}}
        {:strs! [a b], :missing mstrs-nil} strs-map
        {:strs! [a b c], :missing mstrs-c} strs-map
        {:strs! [a b & "c"], :missing mstrs-c&} strs-map

        q-map {:foo/a 1 :b 2 :foo/c 3 ::d 4}
        {:keys! [foo/a :foo/c] :missing mq-nil} q-map
        {:keys! [b & :foo/d] :missing mq-d} q-map
        {:keys! [foo/a & :foo/d] :missing mq-d&} q-map
        {:foo/keys! [a c] :missing q-nil} q-map
        {:foo/keys! [a & :foo/d] :missing q-d} q-map
        {:foo/keys! [a & :foo/d] :missing q-d&} q-map
        {::keys! [d] :missing aq-nil} q-map]

    (testing "1-level :missing keys for keys/qkeys/syms/strs"
      (is (nil? mkeys-nil))
      (is (= mkeys-c {:c nil}))
      (is (= mkeys-c& {:c nil}))

      (is (nil? msyms-nil))
      (is (= msyms-c '{c nil}))
      (is (= msyms-c& '{c nil}))

      (is (nil? mstrs-nil))
      (is (= mstrs-c {"c" nil}))
      (is (= mstrs-c& {"c" nil}))
      
      (is (nil? mq-nil))
      (is (= mq-d #:foo{:d nil}))
      (is (= mq-d& #:foo{:d nil}))
      (is (nil? q-nil))
      (is (= q-d #:foo{:d nil}))
      (is (= q-d& #:foo{:d nil}))
      (is (nil? aq-nil)))

    (testing "required nested map that also has required keys, covering the following cases:
               - :nest is missing
               - :nest is nil
               - :nest is a map missing :x and/or :y
               - :nest is a map having everything that's required"
      (let [sample-nest {:a 0 :b 0}]
        (are [input-map expected] (= expected
                                     (let [{:keys! [a b & :nest]
                                            {:keys! [x y]} :nest
                                            :missing mkeys-missing}
                                           input-map]
                                       mkeys-missing))

          sample-nest {:nest {:x nil, :y nil}}
          
          (assoc sample-nest :nest nil) {:nest {:x nil, :y nil}}
          
          (assoc sample-nest :nest {:x 1}) {:nest {:y nil}}

          (assoc sample-nest :nest {:x 1 :y 2}) nil)))

    (testing "that a nested map required keys is captured with the outer :missing"
      (let [{:keys! [a b], {:keys! [bb]} :nest :missing mkeys-outer} keys-map]
        (is (= mkeys-outer {:nest {:bb nil}}))))))"####,
    ),
    (
        "selector-test",
        r####"(deftest selector-test
  (let [sample-map {:a 1 :b 2 :c 3 :d 4
                    :e 5
                    ::x 10000
                    :nested {:aa 1 'saa 10}}]

    ;; these are compile time
    #_(testing "error cases"
        (is (thrown? js/Error (selector {:keys [a b]})))
        (is (thrown? js/Error (selector sample-map)))
        (is (thrown? js/Error (selector nil)))
        (is (thrown? js/Error (selector {}))))

    (testing "single directives return their values directly"
      (let [ex1 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :select keys-sel})]
        (is (= {:a 1 :b 2 :c 3 :d 4}
               (ex1 sample-map)))

        (testing "checked keys without :missing should throw"
          (is (thrown? js/Error (ex1 (dissoc sample-map :d)))))

        (testing ":select with :or"
          (let [ex1 (selector {:keys [a b & :c :z]
                               :keys! [d]
                               :select keys-sel
                               :or {:z 42}})]
            (is (= {:a 1 :b 2 :c 3 :d 4 :z 42}
                   (ex1 sample-map))))))

      (let [ex1 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :all keys-all})]
        (is (= sample-map (ex1 sample-map))))

      (let [ex1 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :missing keys-missing})]
        (is (nil? (ex1 sample-map)))
        (is (= {:d nil} (ex1 (dissoc sample-map :d)))))

      (let [ex1 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :excess keys-excess})]
        (is (= (dissoc sample-map :a :b :c :d) (ex1 sample-map)))))

    (testing ":select plus :missing, but nothing missing"
      (let [ex2 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :select keys-sel
                           :missing keys-missing})]
        (is (= {:select {:a 1 :b 2 :c 3 :d 4}}
               (ex2 sample-map)))))

    (testing ":select plus :all"
      (let [ex3 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :select keys-sel
                           :missing keys-missing
                           :all keys-all})]
        (is (= {:select {:a 1 :b 2 :c 3 :d 4}
                :all sample-map}
               (ex3 sample-map)))))

    (testing ":select, :all, and :excess"
      (let [ex4 (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :select keys-sel
                           :missing keys-missing
                           :all keys-all
                           :excess keys-excess})]
        (is (= {:select {:a 1 :b 2 :c 3 :d 4}
                :excess (dissoc sample-map :a :b :c :d)
                :all sample-map}
               (ex4 sample-map)))
        (testing "plus :missing"
          (is (= {:d nil}
                 (:missing (ex4 (dissoc sample-map :d))))))))

    (testing "directive names bound to _"
      (let [ex_ (selector {:keys [a b & :c :z]
                           :keys! [d]
                           :select _
                           :missing _
                           :all _
                           :excess _})]
        (is (= {:select {:a 1 :b 2 :c 3 :d 4}
                :excess (dissoc sample-map :a :b :c :d)
                :all sample-map}
               (ex_ sample-map)))))

    (testing "nested :select"
      (let [exnest (selector {{aa :aa saa 'saa
                               :select nest-sel} :nested
                              aqx ::x
                              :select tl-sel})]
        (is (= {::x 10000 :nested {:aa 1 'saa 10}}
               (exnest sample-map)))))

    (testing "nested selection behavior with _ bindings"
      (let [exnest_ (selector {{aa :aa saa 'saa
                                :select _} :nested
                               aqx ::x
                               :select _})]
        (is (= {::x 10000 :nested {:aa 1 'saa 10}}
               (exnest_ sample-map)))))))"####,
    ),
];

#[test]
fn lazy_cat_compiler_macro_defers_each_collection_expression() {
    check(
        "(let [events (atom []) xs (lazy-cat (do (swap! events conj :a) [1 2]) (do (swap! events conj :b) [3]))] (and (= [] @events) (= 1 (first xs)) (= [:a] @events) (= [1 2 3] (vec xs)) (= [:a :b] @events)))",
    );
    check("(empty? (lazy-cat))");
}

#[test]
fn selector_requires_literal_map_with_result_directive() {
    for source in [
        "(selector nil)",
        "(selector {})",
        "(selector {:keys [a]})",
        "(let [m {:select _}] (selector m))",
    ] {
        assert!(compile(source).is_err(), "{source}");
    }
    check("(= nil ((selector {:missing _}) {}))");
}

/// Complete sources in both compiler phases and both host lifecycles, each run in
/// QuickJS and Node (see `tests/support/phase_oracle.rs`).
mod phase_oracles {
    use crate::phase_oracle::{Case, run};

    #[test]
    fn trailing_map_destructuring() {
        run(&[Case::expression(
            "trailing-map-destructuring",
            include_str!(
                "../fixtures/phase-oracles/destructuring-completion/trailing-map-destructuring.cljs"
            ),
        )]);
    }
}
