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
              recv-rf (fn
                        ([] (this-as t (fn? t)))
                        ([a] (this-as t [(fn? t) a]))
                        ([a x] (+ a x)))
              obj (reify IFn
                    (-invoke [_] :orig0)
                    (-invoke [_ x] [:orig1 x])
                    (-invoke [_ a b] [:orig2 a b]))
              throwing-ifn (reify IFn
                             (-invoke [_] :ok0)
                             (-invoke [_ x] [:ok1 x])
                             (-invoke [_ a b] (+ a b)))
              throwing-fn (fn ([a] a) ([a b] (+ a b)))]
          (js* "(() => {
            for (const target of [~{}, ~{}]) {
              for (const n of [0, 1, 2]) {
                Object.defineProperty(target, 'cljs$core$IFn$_invoke$arity$' + n, {
                  configurable: true,
                  get() { throw new Error('probed-arity-' + n); }
                });
              }
            }
          })()" throwing-ifn throwing-fn)
          (set! (.-call throwing-fn) (fn [_ & args] (case (count args) 0 :fn0 1 [:fn1 (first args)] 2 (+ (first args) (second args) 10))))
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
             (transduce (map inc) obj 0 [1 2])
             (reduce recv-rf [])
             (transduce (map inc) recv-rf 0 [1 2])
             (reduce throwing-ifn [])
             (reduce throwing-ifn 0 [1 2])
             (transduce (map inc) throwing-ifn 0 [1 2])
             (reduce throwing-fn [])
             (reduce throwing-fn 0 [1 2])
             (transduce (map inc) throwing-fn 0 [1 2])]))"#),
        "\"[6 [10 20 30] [10 20 30] [1 2 3] [3 2 1] :override0 6 [:override1 5] true [true 5] :ok0 3 [:ok1 5] :fn0 23 [:fn1 25]]\"\n"
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
              r10 (apply + 1 2 3 v33)
              r11 (apply + 1 2 3 a33)
              r12 (apply + 1 2 3 sub-big)
              r13 (apply + 1 2 3 [4 5])
              r14 (apply vector 1 2 3 [4 5])
              getter-arr (js* "(() => { const a = [2]; Object.defineProperty(a, 1, { enumerable: true, get() { throw new Error('excess-getter-read'); } }); a.length = 2; return a; })()")
              r15 (apply < 1 2 3 getter-arr)
              after (c)]
          (pr-str [(- after before) r1 r2 r3 r4 r5 r6 r7 r8 r9 r10 r11 r12 r13 r14 r15]))"#;
    let program = analyze(source, &parse(source).unwrap()).unwrap();
    let mut options = ApplicationOptions::new(OptimizationLevel::Advanced);
    options.pseudo_names = true;
    let mut js = prepare_application(&program, options).emit_js();
    let head = "function cons(head, tail, kind = \"cons\") {";
    assert!(js.contains(head), "missing {head}");
    js = js.replace(head, &format!("{head} globalThis.__cons++;"));
    assert_eq!(
        execute(&format!("globalThis.__cons = 0; console.log({js});")),
        "[0 529 981 529 598 38 598 35 35 40 534 534 986 15 [1 2 3 4 5] false]\n"
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

#[test]
fn apply_completing_preserves_the_reducer_receiver() {
    let run = |source: &str| super::optimization::json_at_every_level(source);
    assert_eq!(
        run(r#"
        (let [rf (fn ([a] a) ([_ _] (this-as t (identical? t js/globalThis))))]
          (pr-str [(apply (completing rf) [0 1])
                   (reduce (completing rf) 0 [1])]))"#),
        "\"[true true]\"\n"
    );
}

#[test]
fn group_by_observes_ifn_overrides_between_items() {
    let run = |source: &str| super::optimization::json_at_every_level(source);
    assert_eq!(
        run(r#"
        (let [f (fn f ([x]
                        (specify! f IFn (-invoke ([_ _] :patched)))
                        :original)
                    ([x _] x))]
          (pr-str (group-by f [1 2])))"#),
        "\"{:original [1], :patched [2]}\"\n"
    );
}

#[test]
fn array_reduce_reads_elements_before_resolving_call() {
    let run = |source: &str| super::optimization::json_at_every_level(source);
    assert_eq!(
        run(r#"
        (let [rf (fn rf ([a] a) ([a x] (+ a x)))
              arr (js* "((rf, patch) => {
                const a = [];
                Object.defineProperty(a, 0, {
                  enumerable: true,
                  get() { patch(); return 1; }
                });
                a.length = 1;
                return a;
              })(~{}, ~{})" rf #(specify! rf IFn (-invoke ([_ a x] (+ a x 10)))))]
          (pr-str (reduce rf 0 arr)))"#),
        "\"11\"\n"
    );
}

#[test]
fn into_supplies_fixed_arity_reducers_to_custom_ireduce() {
    let run = |source: &str| super::optimization::json_at_every_level(source);
    assert_eq!(
        run(r#"
        (deftype FixedOnlySource [xs]
          IReduce
          (-reduce [_ rf init]
            (let [f2 (.-cljs$core$IFn$_invoke$arity$2 rf)]
              (if f2
                (reduce (fn [acc x] (f2 acc x)) init xs)
                :missing-arity-2))))
        (let [src (FixedOnlySource. [1 2 3])]
          (pr-str [(into [] src)
                   (into () src)]))"#),
        "\"[[1 2 3] (3 2 1)]\"\n"
    );
    assert_eq!(
        run(r#"
        (deftype FixedOnlySource [xs]
          IReduce
          (-reduce [_ rf init]
            (let [f2 (.-cljs$core$IFn$_invoke$arity$2 rf)]
              (if f2
                (reduce (fn [acc x] (f2 acc x)) init xs)
                :missing-arity-2))))
        (let [touched (specify! [9] ICounted (-count [_] 1))
              src (FixedOnlySource. [1 2 3])]
          (pr-str [(count touched)
                   (into () src)]))"#),
        "\"[1 (3 2 1)]\"\n"
    );
}

#[test]
fn callable_combinators_preserve_arities_abi_and_lazy_tails() {
    let run = |source: &str| super::optimization::json_at_every_level(source);
    assert_eq!(
        run(r#"
        (let [;; complement: 0, 1, 2, 3, variadic, apply, ABI, non-fn IFn, and remove
              f-any (fn ([] false) ([x] (= x 1)) ([x y] (= (+ x y) 3)) ([x y z & more] (= (+ x y z (count more)) 10)))
              c (complement f-any)
              c-kw (complement :a)
              ;; constantly: 0..4, apply, ABI
              k (constantly :ok)
              ;; partial: 1, 2, 3, 4+ prefix args, called with 0..4+ args, infinite tail, non-fn IFn
              p1 (partial vector :a)
              p2 (partial vector :a :b)
              p3 (partial vector :a :b :c)
              p4 (partial vector :a :b :c :d)
              p-inf (apply partial (fn [a b c & more] [a b c (vec (take 3 more))]) 1 2 3 (range 10 1000))
              p-kw (partial :k {:k 9})
              ;; comp: 0, 1, 2, 3, 4 fns, 0..4+ args, apply, non-fn IFn
              c0 (comp)
              c1 (comp inc)
              c2 (comp str +)
              c3 (comp count str +)
              c4 (comp inc count str +)
              c-kw2 (comp :b :a)
              ;; juxt: 1, 2, 3, 4 fns, 0..4+ args
              j2 (juxt + *)
              j4 (juxt first second last count)
              ;; fnil: 1, 2, 3 defaults, nil vs false, missing arity, variadic
              fn1 (fnil vector :d1)
              fn2 (fnil vector :d1 :d2)
              fn3 (fnil vector :d1 :d2 :d3)
              ;; some-fn & every-pred: 1..4 preds, 0..4+ args
              sf2 (some-fn even? #(> % 10))
              sf4 (some-fn #(= % 1) #(= % 2) #(= % 3) #(= % 4))
              ep2 (every-pred number? pos?)
              ;; memoize
              calls (atom 0)
              mem (memoize (fn [& xs] (swap! calls inc) (vec xs)))
              ;; map transducer: 0, 1, 2, 3+ arities and arity$2 override
              mf ((map +) (fn ([] :init) ([a] [:done a]) ([a x] (+ a x))))
              ;; remove on chunked vec, lazy seq, and transducer
              rem-vec (vec (remove even? [1 2 3 4 5 6]))
              rem-lazy (vec (remove odd? (list 1 2 3 4 5)))
              rem-xf (into [] (remove even?) [1 2 3 4 5 6])]
          (pr-str
            [[(c) (c 1) (c 2) (c 1 2) (c 1 3) (c 1 2 3 4 5 6 7) (apply c [1 2 3 4 5 6 7])
              (.-cljs$lang$maxFixedArity c) (.cljs$core$IFn$_invoke$arity$1 c 1)
              (.cljs$lang$applyTo c (list 1 2 3 4 5 6 7))
              (c-kw {:a 1}) (c-kw {:b 1})]
             [(k) (k 1) (k 1 2) (k 1 2 3 4) (apply k [1 2 3]) (.-cljs$lang$maxFixedArity k)]
             [(p1) (p1 1) (p1 1 2) (p1 1 2 3) (p1 1 2 3 4 5) (apply p1 [1 2 3 4])
              (p2) (p2 1) (p2 1 2) (p2 1 2 3) (p2 1 2 3 4)
              (p3) (p3 1) (p3 1 2) (p3 1 2 3) (p3 1 2 3 4)
              (p4 1 2) (apply p4 [1 2 3]) (p-inf :x) (p-kw)
              (.-cljs$lang$maxFixedArity p1) (.-cljs$lang$maxFixedArity p4)
              (.cljs$core$IFn$_invoke$arity$2 p1 8 9)]
             [(c0 :z) (c1 4) (c2) (c2 2) (c2 2 3) (c2 2 3 4) (c2 1 2 3 4 5) (apply c2 [1 2 3 4])
              (c3) (c3 10) (c3 5 6) (c3 3 4 5) (c3 10 20 30 40)
              (c4 10 20 30 40) (c-kw2 {:a {:b 7}})
              (.-cljs$lang$maxFixedArity c2) (.-cljs$lang$maxFixedArity c4)]
             [(j2 2 3) (j2 1 2 3 4) (j4 [10 20 30]) (.-cljs$lang$maxFixedArity j2)]
             [(fn1 nil) (fn1 false) (fn1 nil 2 3 4)
              (fn2 nil nil) (fn2 false nil 3 4) (nil? (.-cljs$core$IFn$_invoke$arity$1 fn2))
              (fn3 nil nil nil 4)]
             [(sf2) (sf2 3 4) (sf2 1 3 5 12) (sf4 9 9 9 3) (ep2) (ep2 1 2 3 4) (ep2 1 -2 3 4)]
             [(mem 1 2) (mem 1 2) @calls (.-cljs$lang$maxFixedArity mem)]
             [(mf) (mf 5) (mf 5 6) (mf 5 1 2 3) (.-cljs$lang$maxFixedArity mf)]
             [rem-vec rem-lazy rem-xf]
             [(vec (concat [1] [2] [3] [4])) (vec (sequence (map inc) [1 2 3])) (vec (eduction (map inc) [1 2]))
              (trampoline (fn step [n] (if (pos? n) #(step (dec n)) :done)) 3)]
             ;; Sol #7: JS function `.length` remains 4 across callable wrappers.
             [(.-length c) (.-length k) (.-length p1) (.-length p4) (.-length c2) (.-length c4)
              (.-length j2) (.-length mem) (.-length mf) (.-length concat) (.-length eduction)]]))"#),
        "\"[[true false true false true false false 2 false false false true] [:ok :ok :ok :ok :ok 2] [[:a] [:a 1] [:a 1 2] [:a 1 2 3] [:a 1 2 3 4 5] [:a 1 2 3 4] [:a :b] [:a :b 1] [:a :b 1 2] [:a :b 1 2 3] [:a :b 1 2 3 4] [:a :b :c] [:a :b :c 1] [:a :b :c 1 2] [:a :b :c 1 2 3] [:a :b :c 1 2 3 4] [:a :b :c :d 1 2] [:a :b :c :d 1 2 3] [1 2 3 [10 11 12]] 9 3 0 [:a 8 9]] [:z 5 \\\"0\\\" \\\"2\\\" \\\"5\\\" \\\"9\\\" \\\"15\\\" \\\"10\\\" 1 2 2 2 3 4 7 3 0] [[5 6] [10 24] [10 20 30 3] 3] [[:d1] [false] [:d1 2 3 4] [:d1 :d2] [false :d2 3 4] true [:d1 :d2 :d3 4]] [nil true true true true true false] [[1 2] [1 2] 1 0] [:init [:done 5] 11 11 2] [[1 3 5] [2 4] [1 3 5]] [[1 2 3 4] [2 3 4] [2 3] :done] [4 4 4 4 4 4 4 4 4 4 4]]\"\n"
    );
}

#[test]
fn apply_variadic_multifn_and_seq_tails_preserve_semantics_and_identity() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    // Emitted variadic functions (fixed 0..3, with and without fixed arities),
    // MultiFn dispatch, and chained rest-seq / Cons / List / nil / vector / array tails.
    assert_eq!(
        run(r#"
        (defn f0 [& xs] [:f0 (vec xs)])
        (defn f1 [a & xs] [:f1 a (vec xs)])
        (defn f2 ([a] [:f2-1 a]) ([a b] [:f2-2 a b]) ([a b & xs] [:f2-v a b (vec xs)]))
        (defn f3 [a b c & xs] [:f3 a b c (vec xs)])
        (defn fixed2 ([a] [:fx1 a]) ([a b] [:fx2 a b]) ([a b c] [:fx3 a b c]))
        (defmulti reg (fn [kind & _] kind))
        (defmethod reg :sub [_ id & args]
          (apply f3 id :sub :q args))
        (defmethod reg :plain [_ a b] [:plain a b])
        (defn wrap-reg [kind & args] (apply reg kind args))
        (defn wrap-sub [id & args] (apply wrap-reg :sub id args))
        (let [s (seq [10 20])
              c (cons 20 nil)
              c-lazy (cons 20 (cons 30 (lazy-seq (throw (js/Error. "boom")))))
              arr #js [7 8]
              arr-seq (seq arr)
              ident1 (fn [x & more] (identical? more s))
              ident-c (fn [x & more] (identical? more c))
              ident-rest (fn [x & more] (identical? (rest more) c))
              lazy-first (fn [x & more] (first more))
              mut-rest (fn [x & more] (aset arr 0 99) (first (rest more)))]
          (pr-str
            [(apply f0 nil)
             (apply f0 (seq [1 2]))
             (apply f0 :a (seq [1 2]))
             (apply f0 :a :b (list 1 2))
             (apply f1 :a nil)
             (apply f1 :a (seq [1 2]))
             (apply f1 :a :b (seq [1 2]))
             (apply f1 (cons :a (cons :b nil)))
             (apply f1 (list :a))
             (apply f1 (list :a :b :c))
             (apply f2 :a nil)
             (apply f2 :a :b nil)
             (apply f2 :a :b (seq [3 4]))
             (apply f2 (list :a :b))
             (apply f2 (list :a :b :c))
             (apply f3 :a :b :c nil)
             (apply f3 :a :b :c (seq [1 2]))
             (apply fixed2 :x nil)
             (apply fixed2 :x (seq [:y]))
             (apply fixed2 (cons :x (list :y :z)))
             (wrap-sub :todos :fn1 :fn2)
             (wrap-sub :todos)
             (wrap-reg :plain :p1 :p2)
             (apply reg :plain (list :p1 :p2))
             (apply ident1 1 s)
             (apply ident-c 1 c)
             (apply ident-rest 1 2 c)
             (apply lazy-first 1 c-lazy)
             (apply mut-rest 1 2 arr-seq)]))"#),
        "\"[[:f0 []] [:f0 [1 2]] [:f0 [:a 1 2]] [:f0 [:a :b 1 2]] [:f1 :a []] [:f1 :a [1 2]] [:f1 :a [:b 1 2]] [:f1 :a [:b]] [:f1 :a []] [:f1 :a [:b :c]] [:f2-1 :a] [:f2-2 :a :b] [:f2-v :a :b [3 4]] [:f2-2 :a :b] [:f2-v :a :b [:c]] [:f3 :a :b :c []] [:f3 :a :b :c [1 2]] [:fx1 :x] [:fx2 :x :y] [:fx3 :x :y :z] [:f3 :todos :sub :q [:fn1 :fn2]] [:f3 :todos :sub :q []] [:plain :p1 :p2] [:plain :p1 :p2] true true true 20 99]\"\n"
    );

    // Custom cljs$lang$applyTo on an emitted variadic function and custom
    // extend-type / specify! on IndexedSeq, Cons, and List disable fast paths.
    assert_eq!(
        run(r#"
        (defn vf [a & xs] [a (vec xs)])
        (set! (.-cljs$lang$applyTo vf) (fn [args] [:override (vec args)]))
        (pr-str [(apply vf 1 (seq [2 3])) (apply vf 1 nil)])"#),
        "\"[[:override [1 2 3]] [1 []]]\"\n"
    );

    assert_eq!(
        run(r#"
        (let [s (specify! (seq [1 2 3]) ISeq (-first [_] 99) (-rest [_] nil) INext (-next [_] nil))
              c (specify! (cons 1 (list 2)) ISeq (-first [_] 88) (-rest [_] nil) INext (-next [_] nil))
              l (specify! (list 1 2) ISeq (-first [_] 77) (-rest [_] nil) INext (-next [_] nil))]
          (pr-str [(apply vector 0 s) (apply vector 0 c) (apply vector 0 l)]))"#),
        "\"[[0 99] [0 88] [0 77]]\"\n"
    );

    assert_eq!(
        run(r#"
        (extend-type IndexedSeq
          ISeq (-first [_] 42) (-rest [_] nil)
          INext (-next [_] nil))
        (let [r (apply vector 1 (seq [2 3]))]
          (str (nth r 0) "," (nth r 1) "," (count r)))"#),
        "\"1,42,2\"\n"
    );

    // Extending a builtin collection with a user protocol must keep
    // fieldComparisons/applyDirect fast paths active without breaking
    // subsequent core protocol extensions.
    assert_eq!(
        run(r#"
        (defprotocol UserProto (user-op [this x]))
        (extend-type PersistentVector
          UserProto (user-op [this x] (+ (count this) x)))
        (defn variadic-sum [a & more] (+ a (reduce + 0 more)))
        (let [v [10 20]
              s (seq [3 4])
              r1 (str (user-op v 5) "," (apply variadic-sum 1 s) "," (apply variadic-sum 1 v) "," (= v [10 20]))]
          (extend-type PersistentVector
            ISeqable (-seq [_] (list 100)))
          (str r1 "|" (apply variadic-sum 1 v)))"#),
        "\"7,8,31,true|101\"\n"
    );
}

#[test]
fn lazy_method_key_marker_and_core_method_preserve_protocols() {
    let run = |source: &str| super::optimization::json_at_every_level(source);

    assert_eq!(
        run(r#"
        (defprotocol P (pm [this x]))
        (deftype Box [v]
          ICounted (-count [_] v)
          ILookup (-lookup [_ k] [v k]) (-lookup [_ k nf] [v k nf])
          P (pm [_ x] (+ v x)))
        (defrecord Rec [a])
        (extend-type Rec P (pm [this x] (+ (:a this) x)))
        (defmulti mm identity)
        (-add-method mm :k (fn [x] [:hit x]))
        (let [b (Box. 5)
              r (->Rec 10)
              s (specify! #js {} ICounted (-count [_] 77) ISeqable (-seq [_] (list :a :b)))
              t (transient [1])]
          (-conj! t 2)
          (pr-str
            [(satisfies? ICounted b)
             (implements? ICounted b)
             (count b)
             (get b :k)
             (get b :k :nf)
             (pm b 3)
             (pm r 4)
             (satisfies? P r)
             (satisfies? ICounted s)
             (count s)
             (seq s)
             (mm :k)
             (persistent! t)]))"#),
        "\"[true true 5 [5 :k] [5 :k :nf] 8 14 true true 77 (:a :b) [:hit :k] [1 2]]\"\n"
    );
}
