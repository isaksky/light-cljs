//! light-cljs-1084: Fixed-arity invocation of function values across core
//! variadics, `reduce`, `transduce`, `into`, `apply`, and `sort-by`.
//!
//! Every scenario runs at `None`, `Prepare`, and `Advanced` through
//! `optimization::json_at_every_level`, verifying exact parity between all
//! optimization levels including protocol overrides (`extend-type`, `specify!`),
//! laziness, custom `IFn`/`applyTo`, `subvec`, and metadata preservation.

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
