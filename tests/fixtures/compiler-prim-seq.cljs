;; Independent literal contract for pinned public prim-seq and IndexedSeq.
(defn prim-seq-contract []
  (let [a #js [4 5 6] f prim-seq q cljs.core/prim-seq
        like (js-obj "0" 7 "1" 8 "length" 2)
        s (prim-seq a 1) m (with-meta s {:tag :slice})]
    (assert (= [[4 5] [5] nil nil ["a" "b"] ["b"]]
               [(vec (prim-seq #js [4 5])) (vec (prim-seq #js [4 5] 1))
                (prim-seq #js []) (prim-seq #js [4 5] 2)
                (vec (prim-seq "ab")) (vec (prim-seq "ab" 1))]))
    (assert (= [[4 5 6] [5 6] [4 5 6] [5 6] [4 5 6] [5 6]]
               [(vec (f a)) (vec (f a 1)) (vec (q a)) (vec (q a 1))
                (vec (apply prim-seq [a])) (vec (apply cljs.core/prim-seq [a 1]))]))
    (assert (= [[7 8] [8] nil nil nil nil]
               [(vec (prim-seq like)) (vec (prim-seq like 1))
                (prim-seq like 2) (prim-seq like 3)
                (prim-seq "") (prim-seq (js-obj "0" 9))]))
    (assert (= [3 55357 56832 120 [56832 120]]
               [(count (prim-seq "😀x"))
                (js* "~{}.charCodeAt(0)" (first (prim-seq "😀x")))
                (js* "~{}.charCodeAt(0)" (second (prim-seq "😀x")))
                (js* "~{}.charCodeAt(0)" (last (prim-seq "😀x")))
                (vec (map #(js* "~{}.charCodeAt(0)" %) (prim-seq "😀x" 1)))]))
    (assert (instance? IndexedSeq s))
    (assert (identical? a (.-arr s))) (assert (= 1 (.-i s)))
    (assert (nil? (meta s))) (assert (= {:tag :slice} (meta m)))
    (assert (identical? m (with-meta m (meta m))))
    (assert (= [true true true true true true true]
               [(seq? s) (sequential? s) (counted? s) (indexed? s)
                (satisfies? IReduce s) (satisfies? IIterable s) (reversible? s)]))
    (assert (= [2 5 6 :missing [6] [] nil [6 5] 11 21]
               [(count s) (first s) (nth s 1) (nth s 2 :missing)
                (vec (rest s)) (vec (rest (rest s))) (next (next s))
                (vec (rseq s)) (reduce + s) (reduce + 10 s)]))
    (assert (= (hash s) (hash '(5 6)))) (assert (= s '(5 6)))
    (let [i (iter s)] (assert (= [5 6 false] [(.next i) (.next i) (.hasNext i)])))
    (aset a 1 50)
    (assert (= [50 50 [50 6]] [(first s) (first m) (vec (array-seq a 1))]))
    (let [negative (prim-seq #js [4 5 6] -1)
          fractional (prim-seq (js-obj "0.5" 7 "1.5" 8 "length" 2) 0.5)
          nil-start (prim-seq #js [4 5] nil)]
      (assert (= [4 nil 4 :missing [nil 4 5 6]]
                 [(count negative) (first negative) (nth negative 1)
                  (nth negative 0 :missing) (vec negative)]))
      (assert (= [1.5 7 7 8 [7 8] 15]
                 [(count fractional) (first fractional) (nth fractional 0)
                  (first (rest fractional)) (vec fractional) (reduce + fractional)]))
      (assert (= [2 nil 4 5] [(count nil-start) (first nil-start)
                             (nth nil-start 0) (first (next nil-start))])))
    (let [events (atom [])
          source (fn [] (swap! events conj :input) #js [9 10])
          start (fn [] (swap! events conj :start) 1)]
      (assert (= [10] (vec (prim-seq (source) (start)))))
      (assert (= [:input :start] @events)))
    (let [metadata (meta (var prim-seq))]
      (assert (= 'prim-seq (:name metadata)))
      (assert (= "cljs/core.cljs" (:file metadata)))
      (assert (= 1752 (:line metadata)))
      (assert (= "Create seq from a primitive JavaScript Array-like." (:doc metadata)))
      (assert (= '([prim] [prim i]) (:arglists metadata))))
    true))
