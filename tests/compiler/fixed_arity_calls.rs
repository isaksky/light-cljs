//! light-cljs-1084: Fixed-arity invocation of function values across core
//! variadics, `reduce`, `transduce`, `into`, `apply`, and `sort-by`.
//!
//! Every scenario runs at `None`, `Prepare`, and `Advanced` through
//! `optimization::json_at_every_level`, verifying exact parity between all
//! optimization levels including protocol overrides (`extend-type`, `specify!`),
//! laziness, custom `IFn`/`applyTo`, `subvec`, and metadata preservation.

use super::optimization::execute;
use light_cljs::{ApplicationOptions, OptimizationLevel, analyze, parse, prepare_application};

#[test]
fn core_variadics_fixed_arity_in_reduce_transduce_and_sort_by() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Core numeric, bitwise, comparison, and collection variadics as first-class
    // values in 2-arity and 3-arity reduce, transduce, mapv, and sort-by.
    assert_eq!(
        run(r#"
        (let [v100 (vec (range 100))
              v0 []
              v1 [7]
              sub-short (subvec v100 0 10)
              sub-mid (subvec v100 20 50)
              sub-empty (subvec v100 50 50)]
          (pr-str
            [(reduce + 0 v100)
             (reduce + v100)
             (reduce + v0)
             (reduce + v1)
             (reduce + 0 sub-short)
             (reduce + sub-short)
             (reduce + 0 sub-mid)
             (reduce + 0 sub-empty)
             (reduce + sub-empty)
             (reduce * [2 3 4])
             (reduce * [])
             (reduce - [10 3 2])
             (reduce / [24 2 3])
             (reduce max [1 9 3 7])
             (reduce min [5 2 8 3])
             (js/Object.is (reduce max [0 -0.0]) -0.0)
             (js/Object.is (reduce min [0 -0.0]) -0.0)
             (js/isNaN (reduce max [1 ##NaN 2]))
             (js/isNaN (reduce min [1 ##NaN 2]))
             (reduce bit-and [15 7 3])
             (reduce bit-or [1 2 4])
             (reduce bit-xor [1 3 7])
             (reduce bit-and-not [15 1 4])
             (reduce + 0 (range 50))
             (reduce + (range 50))
             (reduce + (range 0))
             (reduce nil 42 [])
             (reduce (fn [a x] (if (= x 32) (reduced a) (+ a x))) 0 v100)
             (reduce (fn [a x] (if (= x 0) (reduced :stop) (+ a x))) 0 v100)
             (reduce conj [] [1 2 3])
             (persistent! (reduce conj! (transient []) [1 2 3]))
             (transduce (map -) conj [1 2 3])
             (transduce (map /) + [2 4])
             (transduce (comp (filter odd?) (map inc) (take 4)) + 0 (vec (range 20)))
             (mapv - [1 -2 3])
             (vec (sort-by - [3 1 2]))
             (vec (sort-by - > [3 1 2]))
             (vec (sort-by :k < [{:k 3} {:k 1} {:k 2}]))
             (vec (sort-by :k > [{:k 1} {:k 3} {:k 2}]))
             (vec (sort-by nil []))
             (vec (sort-by nil [42]))
             (meta (sort-by - (with-meta [3 1 2] {:tag :sorted})))]))"#),
        "\"[4950 4950 0 7 45 45 1035 0 0 24 1 5 4 9 2 true true true true 3 7 5 10 1225 1225 0 42 496 :stop [1 2 3] [1 2 3] [-1 -2 -3] 0.75 20 [-1 2 -3] [3 2 1] [1 2 3] [{:k 1} {:k 2} {:k 3}] [{:k 3} {:k 2} {:k 1}] [] [42] {:tag :sorted}]\"\n"
    );

    // Callable IFn objects (keywords, vars, meta fns, reify IFn) preserve `this`
    // binding when invoked through fixed-arity entries in reduce, transducers,
    // and sort-by.
    assert_eq!(
        run(r#"
        (defn bump [x] (inc x))
        (def mf (with-meta (fn ([x] (* x 10)) ([a b] (+ a b 1))) {:m true}))
        (def rf-obj
          (reify IFn
            (-invoke [_] :zero)
            (-invoke [_ x] [:one x])
            (-invoke [_ a b] (+ a (* b 2)))))
        (pr-str
          [(reduce rf-obj 0 [1 2 3])
           (reduce rf-obj [])
           (transduce (map :a) rf-obj 0 [{:a 1} {:a 2} {:a 3}])
           (into [] (map #'bump) [1 2 3])
           (into [] (map mf) [1 2 3])
           (reduce mf 0 [1 2 3])
           (vec (sort-by mf [3 1 2]))
           (vec (sort-by :k [{:k 3} {:k 1} {:k 2}]))
           (let [k (keyword "x")] (reduce k {:x 99} [:y]))])"#),
        "\"[12 :zero [:one 12] [2 3 4] [10 20 30] 9 [1 2 3] [{:k 1} {:k 2} {:k 3}] 99]\"\n"
    );

    // Transducers built with `reducingFunction` and `into3` preserve completion,
    // metadata, early termination, and non-editable targets.
    assert_eq!(
        run(r#"
        (let [v (vec (range 10))]
          (pr-str
            [(into (with-meta [] {:m :v}) (comp (filter even?) (map inc)) v)
             (meta (into (with-meta [] {:m :v}) (map inc) v))
             (into (with-meta () {:m :l}) [1 2 3])
             (meta (into (with-meta () {:m :l}) [1 2 3]))
             (into () (comp (drop 2) (take 3)) v)
             (into [] (take-while #(< % 4)) v)
             (into [] (drop-while #(< % 7)) v)
             (into [] (take-nth 3) v)
             (into [] (interpose :x) [1 2 3])
             (into [] (dedupe) [1 1 2 2 2 3 1])
             (into [] (partition-all 3) [1 2 3 4 5])
             (into [] (partition-by even?) [1 3 2 4 5])
             (into [] (keep #(when (odd? %) (* % 2))) [1 2 3 4])
             (into [] (map-indexed vector) [:a :b])
             (into [] cat [[1 2] [3] [4 5]])
             (transduce (halt-when #{5} (fn [res x] [res x])) + 0 v)
             (transduce (map inc) (completing + (fn [x] (* x 2))) 0 [1 2 3])]))"#),
        "\"[[1 3 5 7 9] {:m :v} (3 2 1) {:m :l} (4 3 2) [0 1 2 3] [7 8 9] [0 3 6 9] [1 :x 2 :x 3] [1 2 3 1] [[1 2 3] [4 5]] [[1 3] [2 4] [5]] [2 6] [[0 :a] [1 :b]] [1 2 3 4 5] [10 5] 18]\"\n"
    );
}

#[test]
fn apply_direct_tail_matches_seq_semantics_and_protocol_overrides() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // 0 to 3 fixed prefix args with vector and JS array tails across lengths
    // 0..5, 32, and 33, plus 4+ prefix args and core variadic functions.
    assert_eq!(
        run(r#"
        (let [v32 (vec (range 32))
              v33 (vec (range 33))
              a32 (to-array v32)
              a33 (to-array v33)]
          (pr-str
            [(apply + [])
             (apply + [5])
             (apply + [2 3])
             (apply + [1 2 3])
             (apply + [1 2 3 4])
             (apply + [1 2 3 4 5])
             (apply + 10 [])
             (apply + 10 [1])
             (apply + 10 [1 2])
             (apply + 10 [1 2 3])
             (apply + 10 [1 2 3 4])
             (apply + 10 20 [])
             (apply + 10 20 [1])
             (apply + 10 20 [1 2])
             (apply + 10 20 [1 2 3])
             (apply + 10 20 30 [])
             (apply + 10 20 30 [1])
             (apply + 10 20 30 [1 2])
             (apply + 10 20 30 [1 2 3])
             (apply + 1 2 3 4 [5 6])
             (apply + v32)
             (apply + v33)
             (apply + #js [1 2 3])
             (apply + 10 #js [1 2 3])
             (apply + a32)
             (apply + a33)
             (apply - [5])
             (apply - 10 [3 2])
             (apply * [])
             (apply * 2 [3 4])
             (apply / [2])
             (apply / 24 [2 3])
             (apply < 1 [2 3 4])
             (apply < 1 [2 2 4])
             (apply = 1 [1 1 1])
             (apply == 0 [-0.0 0])
             (apply max 1 [5 3 2])
             (apply min 5 [2 8 1])
             (apply str "a" ["b" "c" "d"])
             (apply vector 1 [2 3])
             (apply list 1 [2 3])
             (apply list* 1 [2 [3 4]])
             (= (apply hash-map :a [1 :b 2]) {:a 1 :b 2})
             (apply distinct? 1 [2 3 2])
             (apply bit-or 1 [2 4 8])]))"#),
        "\"[0 5 5 6 10 15 10 11 13 16 20 30 31 33 36 60 61 63 66 21 496 528 6 16 496 528 -5 5 1 24 0.5 4 true false true true 5 1 \\\"abcd\\\" [1 2 3] (1 2 3) (1 2 3 4) true false 15]\"\n"
    );

    // Laziness is preserved for lazy seq tails (boundedCount probes past maxFixedArity
    // to choose variadic dispatch, but must not realize the rest of the tail).
    assert_eq!(
        run(r#"
        (let [seen (atom 0)
              s (map (fn [x] (swap! seen inc) x) (list 2 1 9 10 11 12 13 14))]
          (pr-str [(apply < 1 s) (<= @seen 3)]))"#),
        "\"[false true]\"\n"
    );

    // extend-type on PersistentVector disables directTail and toArray vector fast paths.
    assert_eq!(
        run(r#"
        (extend-type PersistentVector
          ISeqable (-seq [_] (list 10 20))
          ICounted (-count [_] 2))
        (pr-str
          (list (apply + 1 [2 3 4])
                (apply list (sort-by - [3 1 2]))))"#),
        "\"(31 (20 10))\"\n"
    );

    // specify! on a vector instance disables directTail and toArray vector fast paths.
    assert_eq!(
        run(r#"
        (let [v (specify! [2 3 4] ISeqable (-seq [_] (list 100 200)))]
          (pr-str [(apply + 1 v) (vec (sort-by - v))]))"#),
        "\"[301 [200 100]]\"\n"
    );

    // specify! on an array instance and extend-type on js/Array disable directTail array fast path.
    assert_eq!(
        run(r#"
        (let [a (specify! #js [2 3 4] ISeqable (-seq [_] (list 30 40)))]
          (pr-str [(apply + 1 a) (apply + 1 #js [2 3 4])]))"#),
        "\"[71 10]\"\n"
    );
    assert_eq!(
        run(r#"
        (extend-type js/Array
          ISeqable (-seq [_] (list 50 60))
          ICounted (-count [_] 2))
        (str (apply + 1 #js [2 3 4]))"#),
        "\"111\"\n"
    );

    // Custom cljs$lang$applyTo override on a function is respected.
    assert_eq!(
        run(r#"
        (let [f (fn [a b] (+ a b))]
          (set! (.-cljs$lang$maxFixedArity f) 1)
          (set! (.-cljs$lang$applyTo f) (fn [args] [:custom (vec args)]))
          (pr-str [(apply f 1 [2 3])]))"#),
        "\"[[:custom [1 2 3]]]\"\n"
    );

    // Public PersistentVector with callerTail observes mutations and does not infinite-loop
    // when cnt > tail.length in reduce.
    assert_eq!(
        run(r#"
        (let [arr #js [1 2 3]
              v (PersistentVector. nil 3 5 #js [] arr nil)
              short-v (PersistentVector. nil 5 5 #js [] #js [10 20] nil)]
          (aset arr 1 20)
          (pr-str
            [(apply + 1 v)
             (vec (sort-by - v))
             (count (reduce conj [] short-v))]))"#),
        "\"[25 [20 3 1] 2]\"\n"
    );
}

#[test]
fn non_number_variadics_ifn_values_extended_protocols_and_sort_by_semantics() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Non-number + behaviour, reduced across vector/range/array/map/key-seq,
    // variadic user fns called at 2 args, and non-JS-function IFn values
    // (keywords, maps, sets, vectors, vars, multimethods, MetaFn, reify IFn).
    assert_eq!(
        run(r#"
        (defmulti mm (fn [a b] (keyword (str a))))
        (defmethod mm :0 [_ b] (+ 10 b))
        (defmethod mm :default [a b] (+ a b))
        (defn add-var [a b] (+ a b))
        (let [v-small (vec (range 10))
              v-big (vec (range 50))
              var-fn (fn [a b & more] (+ a b (count more)))
              multi-var (fn ([x] (* x 3)) ([a b & more] (+ a b (count more))))
              rest-only (fn [& args] (count args))
              m {:a 10 :b 20}
              s #{1 2 3}]
          (pr-str
            [(reduce + "a" ["b" "c"])
             (reduce + [nil 1 2])
             (js/isNaN (reduce + [1 ##NaN 2]))
             (js/Object.is (reduce + [-0.0 -0.0]) -0.0)
             (reduce (fn [a x] (if (= x 4) (reduced [:small a]) (+ a x))) 0 v-small)
             (reduce (fn [a x] (if (= x 40) (reduced [:big a]) (+ a x))) 0 v-big)
             (reduce (fn [a x] (if (= x 5) (reduced [:arr a]) (+ a x))) 0 #js [1 2 3 4 5 6])
             (reduce (fn [a x] (if (= x 5) (reduced [:rng a]) (+ a x))) 0 (range 20))
             (reduce (fn [a k] (if (= k :b) (reduced :hit) (conj a k))) [] (keys {:a 1 :b 2 :c 3}))
             (reduce var-fn 0 [1 2 3])
             (reduce multi-var 0 [1 2 3])
             (transduce (map multi-var) + 0 [1 2 3])
             (reduce rest-only 0 [1 2 3])
             (reduce mm 0 [1 2 3])
             (reduce #'add-var 0 [1 2 3])
             (reduce m :a [:miss])
             (reduce s 2 [:miss])
             (reduce [:z :y :x] 1 [:miss])
             (reduce :b {:a 1} [:fallback])
             (into [] (map m) [:a :b])
             (into [] (filter s) [0 1 2 4])
             (into [] (map [:x :y :z]) [2 0 1])
             (reduce + 0 (vals {:a 1 :b 2 :c 3}))]))"#),
        "\"[\\\"abc\\\" 3 true true [:small 6] [:big 780] [:arr 10] [:rng 10] :hit 6 6 18 2 16 6 10 2 :y :fallback [10 20] [1 2] [:z :x :y] 6]\"\n"
    );

    // JS function with specify! IFn preserves receiver and IFn override in
    // reduce, transducers, sort-by, and apply.
    assert_eq!(
        run(r#"
        (let [f (specify! (fn ([x] x) ([a b] (+ a b)))
                  IFn
                  (-invoke ([_] :zero)
                           ([_ x] (* x 100))
                           ([_ a b] (+ a (* b 10)))))]
          (pr-str
            [(reduce f 0 [1 2 3])
             (reduce f [])
             (into [] (map f) [1 2])
             (vec (sort-by f [3 1 2]))]))"#),
        "\"[60 :zero [100 200] [1 2 3]]\"\n"
    );

    // Extended IReduce on PersistentVector (both extend-type and specify!) bypasses
    // vectorReduce for both <=32 and >32 vectors.
    assert_eq!(
        run(r#"
        (let [v-small (specify! [1 2 3]
                        IReduce
                        (-reduce ([_ f] :spec-2)
                                 ([_ f init] [:spec-3 init])))
              v-big (specify! (vec (range 40))
                      IReduce
                      (-reduce ([_ f init] [:big-spec init])))]
          (pr-str [(reduce + v-small) (reduce + 9 v-small) (reduce + 7 v-big)]))"#),
        "\"[:spec-2 [:spec-3 9] [:big-spec 7]]\"\n"
    );
    assert_eq!(
        run(r#"
        (extend-type PersistentVector
          IReduce
          (-reduce ([_ f] :ext-2)
                   ([_ f init] [:ext-3 init])))
        (pr-str [(reduce + [1 2 3]) (reduce + 0 [1 2 3]) (reduce + 0 (vec (range 40)))])"#),
        "\"[:ext-2 [:ext-3 0] [:ext-3 0]]\"\n"
    );

    // apply with infinite lazy tails, >32 tails with 1..3 prefix args, and
    // non-number / 0-arg variadic core functions.
    assert_eq!(
        run(r#"
        (let [v40 (vec (range 40))
              a40 (to-array v40)]
          (pr-str
            [(apply < 10 (range))
             (apply (fn [a b & more] [a b (vec (take 3 more))]) 1 (range))
             (apply + 10 v40)
             (apply + 10 20 v40)
             (apply + 10 20 30 v40)
             (apply + 10 a40)
             (apply + 10 20 a40)
             (apply + "a" ["b" "c"])
             (js/isNaN (apply - []))
             (js/isNaN (apply / []))
             (apply < [])
             (apply = [])
             (apply == [])
             (apply bit-and [5])]))"#),
        "\"[false [1 0 [1 2 3]] 790 810 840 790 810 \\\"abc\\\" true true false true false 0]\"\n"
    );

    // sort-by stability (<=32 and >32), boolean/numeric/truthy comparators,
    // and `comparator` wrapper.
    assert_eq!(
        run(r#"
        (let [small [{:k 2 :id 1} {:k 1 :id 2} {:k 2 :id 3} {:k 1 :id 4} {:k 2 :id 5}]
              big (vec (for [i (range 40)] {:k (mod i 5) :id i}))]
          (pr-str
            [(mapv :id (sort-by :k small))
             (mapv :id (sort-by :k < small))
             (mapv :id (sort-by :k > small))
             (mapv :id (sort-by :k (fn [a b] (- b a)) small))
             (mapv :id (sort-by :k (fn [a b] (when (< a b) :yes)) small))
             (mapv :id (sort-by :k (comparator <) small))
             (= (mapv :id (sort-by :k big))
                (vec (for [r (range 5) i (range 40) :when (= (mod i 5) r)] i)))]))"#),
        "\"[[2 4 1 3 5] [2 4 1 3 5] [1 3 5 2 4] [1 3 5 2 4] [2 4 1 3 5] [2 4 1 3 5] true]\"\n"
    );
}

#[test]
fn fixed_arity_callbacks_preserve_receivers_and_call_overrides() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Strict JS functions without fixed-arity entries still receive `this === null`,
    // and reified IFn objects honor a replaced `.call` method in call0/call1/call2/fixedFn.
    assert_eq!(
        run(r#"
        (let [strict2 (js* "(function(a, b) { 'use strict'; return this === null ? a + b : -1; })")
              strict1 (js* "(function(x) { 'use strict'; return this === null ? x * 10 : -1; })")
              strict-cmp (js* "(function(a, b) { 'use strict'; return this === null ? b - a : 0; })")
              obj (reify IFn
                    (-invoke [_] :orig0)
                    (-invoke [_ x] [:orig1 x])
                    (-invoke [_ a b] [:orig2 a b]))]
          (set! (.-call obj)
                (fn [_ & args]
                  (case (count args)
                    0 :override0
                    1 [:override1 (first args)]
                    2 (+ (first args) (second args)))))
          (pr-str
            [(reduce strict2 0 [1 2 3])
             (into [] (map strict1) [1 2 3])
             (mapv strict1 [1 2 3])
             (vec (sort-by strict1 [3 1 2]))
             (vec (sort-by identity strict-cmp [1 3 2]))
             (reduce obj [])
             (reduce obj 0 [1 2 3])
             (transduce (map inc) obj 0 [1 2])]))"#),
        "\"[6 [10 20 30] [10 20 30] [1 2 3] [3 2 1] :override0 6 [:override1 5]]\"\n"
    );
}

#[test]
fn apply_direct_array_tails_preserve_indexed_reads() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Throwing getter at index 1 is never read when `<` short-circuits on (10 < 2),
    // and custom array Symbol.iterator is never consulted by apply on arrays.
    assert_eq!(
        run(r#"
        (let [getter-arr (js* "(() => { const a = [2]; Object.defineProperty(a, 1, { enumerable: true, get() { throw new Error('read-index-1'); } }); a.length = 2; return a; })()")
              iter-arr (js* "(() => { const a = [2, 3, 4]; a[Symbol.iterator] = function*() { yield 100; }; return a; })()")
              iter-long (js* "(() => { const a = Array.from({length: 35}, (_, i) => i); a[Symbol.iterator] = function*() { throw new Error('iter-used'); }; return a; })()")]
          (pr-str
            [(apply < 10 getter-arr)
             (apply + iter-arr)
             (apply + 1 iter-arr)
             (apply + 1 2 iter-arr)
             (apply + 1 2 3 iter-arr)
             (apply + 1 iter-long)
             (apply + 1 2 3 iter-long)]))"#),
        "\"[false 9 10 12 15 596 601]\"\n"
    );
}

#[test]
fn apply_numeric_equality_realizes_the_existing_tail() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // `==` keeps its non-variadic `apply` tail realization (materializes all elements).
    assert_eq!(
        run(r#"
        (let [seen (atom 0)
              s (map (fn [x] (swap! seen inc) x) (list 2 3 4 5 6 7))]
          (pr-str [(apply == 1 s) @seen]))"#),
        "\"[false 6]\"\n"
    );
}

#[test]
fn list_conj_preserves_tail_count_overrides() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    assert_eq!(
        run(r#"
        (let [xs (specify! (list 2) ICounted (-count [_] 100))]
          (pr-str [(count (conj xs 1)) (count (conj (conj xs 1) 0))]))"#),
        "\"[101 102]\"\n"
    );
    assert_eq!(
        run(r#"
        (extend-type List ICounted (-count [_] 100))
        (pr-str [(.-count (conj (list 2) 1))])"#),
        "\"[101]\"\n"
    );
}

#[test]
fn apply_direct_tail_dispatches_across_leaf_boundaries() {
    let source = r#"
        (let [v33 (vec (range 33))
              v50 (vec (range 50))
              sub-big (subvec v50 5 45)
              sub-small (subvec v50 5 10)
              a33 (to-array v33)
              a35 (to-array (range 35))
              v35 (vec (range 35))
              c #(aget js/globalThis "__cons")
              before (c)
              r1 (apply + 1 v33)
              r2 (apply + 1 sub-big)
              r3 (apply + 1 a33)
              r4 (apply + 1 2 v35)
              r5 (apply + 1 2 sub-small)
              r6 (apply + 1 2 a35)
              r7 (count (apply vector v35))
              r8 (count (apply vector a35))
              r9 (count (apply vector sub-big))
              after (c)
              r10 (apply + 1 2 3 v33)
              r11 (apply + 1 2 3 a33)
              r12 (apply + 1 2 3 sub-big)]
          (pr-str [(- after before) r1 r2 r3 r4 r5 r6 r7 r8 r9 r10 r11 r12]))"#;
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program, options).emit_js();
    let head = "function cons(head, tail, kind = \"cons\") {";
    assert!(js.contains(head), "missing {head}");
    js = js.replace(head, &format!("{head} globalThis.__cons++;"));
    assert_eq!(
        execute(&format!("globalThis.__cons = 0; console.log({js});")),
        "[0 529 981 529 598 38 598 35 35 40 534 534 986]\n"
    );
}

#[test]
fn native_reduce_loops_use_fixed_arity_entries() {
    let source = r#"
        (let [make-rf (fn []
                        (let [hits (atom 0)
                              f (fn ([a] a) ([a b] (+ a b)))
                              orig (.-cljs$core$IFn$_invoke$arity$2 f)]
                          (set! (.-cljs$core$IFn$_invoke$arity$2 f)
                                (fn [a b] (swap! hits inc) (orig a b)))
                          [f hits]))
              check (fn [run-fn]
                      (let [[f hits] (make-rf)]
                        [(run-fn f) @hits]))
              am (array-map :a 1 :b 2 :c 3)
              hm (zipmap (range 12) (range 12))
              sp-vec (specify! [1 2 3] ISeqable (-seq [_] (list 10 20 30)))
              iter-obj (iteration (fn [k] (when (< k 4) k)) :kf #(inc %) :vf identity :initk 0)]
          (pr-str
            [(check #(reduce % 0 (repeat 5 2)))
             (check #(transduce (take 4) % 0 (iterate inc 1)))
             (check #(transduce (take 5) % 0 (cycle [1 2])))
             (check #(reduce % 0 sp-vec))
             (let [hits (atom 0)
                   rf (fn ([a] a) ([a e] (+ a (val e))))
                   orig (.-cljs$core$IFn$_invoke$arity$2 rf)]
               (set! (.-cljs$core$IFn$_invoke$arity$2 rf)
                     (fn [a e] (swap! hits inc) (orig a e)))
               [(reduce rf 0 (seq am)) @hits])
             (let [hits (atom 0)
                   rf (fn ([a] a) ([a e] (+ a (val e))))
                   orig (.-cljs$core$IFn$_invoke$arity$2 rf)]
               (set! (.-cljs$core$IFn$_invoke$arity$2 rf)
                     (fn [a e] (swap! hits inc) (orig a e)))
               [(reduce rf 0 (seq hm)) @hits])
             (check #(transduce (distinct) % 0 [1 2 1 3 2]))
             (check #(reduce % 0 iter-obj))]))"#;
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let options = ApplicationOptions::new(OptimizationLevel::Advanced);
    let js = prepare_application(&program, options).emit_js();
    assert_eq!(
        execute(&format!("console.log({js});")),
        "[[10 5] [10 4] [7 5] [60 3] [6 3] [66 12] [6 3] [6 4]]\n"
    );
}

#[test]
fn advanced_hot_calls_reach_fixed_arity_entries() {
    let source = r#"
        (let [v100 (vec (range 100))
              a2 (.-cljs$core$IFn$_invoke$arity$2 +)
              s1 (.-cljs$core$IFn$_invoke$arity$1 -)
              hits (js-obj "add2" 0 "sub1" 0 "map2" 0)]
          (set! (.-cljs$core$IFn$_invoke$arity$2 +)
                (fn [a b] (aset hits "add2" (inc (aget hits "add2"))) (a2 a b)))
          (set! (.-cljs$core$IFn$_invoke$arity$1 -)
                (fn [x] (aset hits "sub1" (inc (aget hits "sub1"))) (s1 x)))
          (let [r-sum (reduce + 0 v100)
                xf (map inc)
                wrapped-xf (fn [rf]
                             (let [step (xf rf)
                                   orig2 (.-cljs$core$IFn$_invoke$arity$2 step)]
                               (set! (.-cljs$core$IFn$_invoke$arity$2 step)
                                     (fn [acc x]
                                       (aset hits "map2" (inc (aget hits "map2")))
                                       (orig2 acc x)))
                               step))
                r-into (count (into [] wrapped-xf v100))
                r-sort (first (sort-by - v100))]
            (pr-str [r-sum r-into r-sort
                     (aget hits "add2")
                     (aget hits "map2")
                     (> (aget hits "sub1") 100)
                     (aget js/globalThis "__addDispatch")
                     (aget js/globalThis "__subDispatch")])))"#;
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program, options).emit_js();
    let add_head = "function add(x, y) {";
    let sub_head = "function subtract(x, y) {";
    assert!(js.contains(add_head), "missing {add_head}");
    assert!(js.contains(sub_head), "missing {sub_head}");
    js = js.replace(add_head, &format!("{add_head} globalThis.__addDispatch++;"));
    js = js.replace(sub_head, &format!("{sub_head} globalThis.__subDispatch++;"));
    assert_eq!(
        execute(&format!(
            "globalThis.__addDispatch = 0; globalThis.__subDispatch = 0; console.log({js});"
        )),
        "[4950 100 99 100 100 true 0 0]\n"
    );
}

#[test]
fn generality_edge_cases_for_reduce_apply_and_sort_by() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // (reduce + ...) with reduced via transducers, variadic-only rf on empty/>32,
    // Subvec and deftype IReduce, apply with custom ISeq/ICounted tail, Subvec
    // specify! tail, ICounted-only vector specify!, fixed-arity target with lazy
    // tail, multi-arity+variadic target with infinite tail, and boolean sort-by
    // comparators.
    assert_eq!(
        run(r#"
        (deftype CustomReduce [x]
          IReduce
          (-reduce [_ f] (f x))
          (-reduce [_ f init] (f (f init x) (+ x 1))))
        (deftype CustomSeqTail [a b]
          ISeqable
          (-seq [this] this)
          ISeq
          (-first [_] a)
          (-rest [_] (if (some? b) (CustomSeqTail. b nil) ()))
          INext
          (-next [_] (when (some? b) (CustomSeqTail. b nil)))
          ICounted
          (-count [_] (if (some? b) 2 1)))
        (let [v50 (vec (range 50))
              rest-only (fn [& args] (count args))
              sub-red (specify! (subvec v50 2 10)
                        IReduce
                        (-reduce ([_ _ init] [:sub-red init])))
              sub-seq (specify! (subvec v50 2 10)
                        ISeqable
                        (-seq [_] (list 7 8)))
              cnt-only (specify! [2 3 4]
                         ICounted
                         (-count [_] 2))
              fixed2 (fn ([a] (* a 10)) ([a b] (+ a b)))
              multi-inf (fn ([a] a) ([a b] (+ a b)) ([a b & more] [a b (first more)]))
              items [{:k 2 :id 1} {:k 1 :id 2} {:k 2 :id 3} {:k 1 :id 4}]]
          (pr-str
            [(transduce (take 5) + 0 v50)
             (transduce (halt-when #(= % 4)) + 0 v50)
             (reduce rest-only [])
             (reduce rest-only [1 2 3])
             (reduce rest-only 0 v50)
             (reduce + 10 (CustomReduce. 5))
             (reduce + 3 sub-red)
             (apply + 1 (CustomSeqTail. 10 20))
             (apply + 1 sub-seq)
             (apply + 1 cnt-only)
             (apply fixed2 (take 2 (range 10 20)))
             (apply fixed2 1 [2])
             (apply multi-inf 1 (range))
             (mapv :id (sort-by :k (fn [a b] (< a b)) items))
             (mapv :id (sort-by :k (fn [a b] (> a b)) items))]))"#),
        "\"[10 4 0 2 2 21 [:sub-red 3] 31 16 10 21 3 [1 0 1] [2 4 1 3] [1 3 2 4]]\"\n"
    );
}
