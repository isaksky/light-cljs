; Public iterator helper contract, independent of generated runtime/metadata tables.
(defn iterator-helper-contract []
  (doseq [make-iter [(fn [coll] (seq-iter coll)) seq-iter
                    (fn [coll] (apply seq-iter [coll]))]]
    (let [events (atom [])
          coll (lazy-seq (swap! events conj :head)
                 (cons 1 (lazy-seq (swap! events conj :tail) (list 2))))
          i (make-iter coll)]
      (assert (instance? SeqIter i))
      (assert (identical? INIT (.-_seq i)))
      (assert (identical? coll (.-_next i)))
      (assert (= [] @events))
      (assert (.hasNext i)) (assert (.hasNext i))
      (assert (= [:head] @events))
      (assert (identical? START (.-_seq i)))
      (assert (= 1 (.next i)))
      (assert (= [:head] @events))
      (assert (.hasNext i)) (assert (.hasNext i))
      (assert (= [:head :tail] @events))
      (assert (= 2 (.next i)))
      (assert (not (.hasNext i))) (assert (not (.hasNext i)))
      (assert (= "No such element" (try (.next i) nil (catch js/Error e (.-message e)))))
      (assert (= "Unsupported operation" (.-message (.remove i)))))
    (doseq [coll [nil '() []]]
      (let [i (make-iter coll)]
        (assert (not (.hasNext i)))
        (assert (= "No such element" (try (.next i) nil (catch js/Error e (.-message e)))))))
    (let [arr #js [1 2] i (make-iter arr)]
      (aset arr 0 9)
      (assert (= 9 (.next i)))
      (aset arr 1 8)
      (assert (= 8 (.next i)))
      (assert (not (.hasNext i)))))
  (doseq [make-iter [(fn [v start end] (ranged-iterator v start end)) ranged-iterator
                    (fn [v start end] (apply ranged-iterator [v start end]))]]
    (let [v (vec (range 70)) i (make-iter v 31 66) arr (.-arr i)]
      (assert (instance? RangedIterator i))
      (assert (identical? v (.-v i)))
      (assert (= [31 0 31 66] [(.-i i) (.-base i) (.-start i) (.-end i)]))
      (assert (.hasNext i)) (assert (.hasNext i))
      (assert (= 31 (.-i i)))
      (aset arr 31 91)
      (assert (= 91 (.next i)))
      (assert (identical? arr (.-arr i)))
      (assert (= 32 (.next i)))
      (assert (= 32 (.-base i)))
      (assert (not (identical? arr (.-arr i))))
      (loop [n 33] (when (< n 66)
        (assert (.hasNext i)) (assert (.hasNext i))
        (assert (= n (.next i))) (recur (inc n))))
      (assert (= [66 64] [(.-i i) (.-base i)]))
      (assert (not (.hasNext i))) (assert (not (.hasNext i)))
      ; RangedIterator's next is intentionally unchecked after exhaustion.
      (assert (= 66 (.next i))) (assert (not (.hasNext i))))
    (doseq [[start end] [[0 0] [32 32] [40 39] [70 70]]]
      (let [v (vec (range 70)) i (make-iter v start end)]
        (assert (= start (.-i i)))
        (assert (= (- start (js-mod start 32)) (.-base i)))
        (assert (not (.hasNext i)))
        (assert (= (< start 70) (some? (.-arr i))))))
    (let [i (make-iter [] 0 0)]
      (assert (nil? (.-arr i))) (assert (not (.hasNext i)))
      (assert (true? (try (.next i) false (catch js/Error e true)))))
    (let [i (make-iter [3 4 5] 1 3)]
      (assert (= [4 5 false] [(.next i) (.next i) (.hasNext i)]))))
  (assert (= '([coll]) (:arglists (meta #'seq-iter))))
  (assert (= '([v start end]) (:arglists (meta #'ranged-iterator))))
  (assert (= 'seq-iter (:name (meta #'seq-iter))))
  (assert (= 'ranged-iterator (:name (meta #'ranged-iterator))))
  true)
