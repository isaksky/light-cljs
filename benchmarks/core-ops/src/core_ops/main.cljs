(ns core-ops.main
  "Per-operation micro-benchmarks of core operations, one source for
  light-cljs and shadow-cljs (benchmarks/core-ops/README.md).
  Every bench takes an iteration count N and a seed S, builds its data from S
  outside the loop, and returns a numeric checksum so nothing is dead. Both
  compilers' outputs must print the same checksums; run.bb fails otherwise.")

;; ---------------------------------------------------------------- data

(defn small-map [s]
  {:a s :b (+ s 1) :c (+ s 2) :d (+ s 3) :e (+ s 4) :f 5 :g 6})

(defn small-maps [s]
  (let [arr (array)]
    (dotimes [j 4] (.push arr (small-map (+ s j))))
    arr))

(defn hash-keys [] (into-array (map (fn [i] (keyword (str "k" i))) (range 100))))

(defn big-map [ks s]
  (loop [i 0 m {}]
    (if (< i (alength ks)) (recur (inc i) (assoc m (aget ks i) (+ s i))) m)))

(defn vec-n [n s] (into [] (map (fn [i] (+ i s))) (range n)))

(defprotocol Shape (area [this]))
(defrecord Rect [w h] Shape (area [_] (* w h)))
(defrecord Circle [r] Shape (area [_] (* 3 r r)))

(defmulti marea :type)
(defmethod marea :rect [m] (* (:w m) (:h m)))
(defmethod marea :circle [m] (* 3 (:r m) (:r m)))
(defmethod marea :default [_] 0)

(defn ma ([a] (ma a 1)) ([a b] (+ a b)) ([a b c] (+ a b c)))
(defn va [a & more] (+ a (count more)))

;; ---------------------------------------------------------------- benches

(defn b-kw-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (:c (aget ms (bit-and i 3))))) acc))))

(defn b-get-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (get (aget ms (bit-and i 3)) :e))) acc))))

(defn b-get-small-miss [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (get (aget ms (bit-and i 3)) :zz 1))) acc))))

(defn b-contains-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (if (contains? (aget ms (bit-and i 3)) :d) 1 0))) acc))))

(defn b-count-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (aget ms (bit-and i 3))))) acc))))

(defn b-assoc-small-existing [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (assoc (aget ms (bit-and i 3)) :c i)))) acc))))

(defn b-assoc-small-new [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (assoc (aget ms (bit-and i 3)) :h i)))) acc))))

(defn b-dissoc-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (dissoc (aget ms (bit-and i 3)) :c)))) acc))))

(defn b-update-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (:a (update (aget ms (bit-and i 3)) :a inc)))) acc))))

;; The literal goes through a JavaScript array before it is counted: a
;; compiler that knows a literal's count could otherwise fold the loop to a
;; constant and never build the map (light-cljs did).
(defn b-map-literal [n s]
  (let [sink (array nil)]
    (loop [i 0 acc 0]
      (if (< i n)
        (do (aset sink 0 {:a i :b s :c 3})
            (recur (inc i) (+ acc (count (aget sink 0)))))
        acc))))

(defn b-get-hash [n s]
  (let [ks (hash-keys) m (big-map ks s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (get m (aget ks (mod i 100))))) acc))))

(defn b-kw-hash [n s]
  (let [ks (hash-keys) m (big-map ks s) ms (array m (assoc m :k3 1) m (assoc m :k4 2))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (:k42 (aget ms (bit-and i 3))))) acc))))

(defn b-assoc-hash [n s]
  (let [ks (hash-keys) m (big-map ks s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (assoc m (aget ks (mod i 100)) i)))) acc))))

(defn b-dissoc-hash [n s]
  (let [ks (hash-keys) m (big-map ks s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (dissoc m (aget ks (mod i 100)))))) acc))))

(defn b-update-hash [n s]
  (let [ks (hash-keys) m (big-map ks s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (update m (aget ks (mod i 100)) inc)))) acc))))

(defn b-get-in [n s]
  (let [m {:a {:b {:c s :d 1} :e 2} :f 3}]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (get-in m [:a :b :c]))) acc))))

(defn b-assoc-in [n s]
  (let [m {:a {:b {:c s :d 1} :e 2} :f 3}]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (assoc-in m [:a :b :c] i)))) acc))))

(defn b-update-in [n s]
  (let [m {:a {:b {:c s :d 1} :e 2} :f 3}]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (update-in m [:a :b :c] + i)))) acc))))

(defn b-vec-conj-64 [n s]
  (loop [i 0 acc 0]
    (if (< i n)
      (recur (inc i) (+ acc (count (loop [j 0 v []] (if (< j 64) (recur (inc j) (conj v (+ j s))) v)))))
      acc)))

(defn b-vec-literal [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count [i s 3 4]))) acc)))

(defn b-vec-nth [n s]
  (let [v (vec-n 1024 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (nth v (bit-and i 1023)))) acc))))

(defn b-vec-invoke [n s]
  (let [v (vec-n 1024 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (v (bit-and i 1023)))) acc))))

(defn b-vec-peek [n s]
  (let [vs (array (vec-n 10 s) (vec-n 40 s) (vec-n 100 s) (vec-n 3 s))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (peek (aget vs (bit-and i 3))))) acc))))

(defn b-vec-assoc [n s]
  (let [v (vec-n 1024 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (assoc v (bit-and i 1023) i)))) acc))))

(defn b-destructure-vec [n s]
  (let [vs (array [s 1 2] [s 3 4] [s 5 6] [s 7 8])]
    (loop [i 0 acc 0]
      (if (< i n) (recur (inc i) (+ acc (let [[a b c] (aget vs (bit-and i 3))] (+ a b c)))) acc))))

(defn b-destructure-map [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0]
      (if (< i n) (recur (inc i) (+ acc (let [{:keys [a c e]} (aget ms (bit-and i 3))] (+ a c e)))) acc))))

(defn b-reduce-vec-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (reduce + 0 v))) acc))))

(defn b-reduce-map-inc-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (reduce + 0 (map inc v)))) acc))))

(defn b-filter-count-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (filter even? v)))) acc))))

(defn b-into-xform-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into [] (map inc) v)))) acc))))

(defn b-mapv-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (mapv inc v)))) acc))))

;; A walk of a map's keys or values, as views over maps walk them
;; (light-cljs-1064): `next` and `first` of the key or value seq.
(defn walk-keys [ks] (loop [q ks a 0] (if q (recur (next q) (+ a (if (identical? :a (first q)) 7 1))) a)))
(defn walk-vals [vs] (loop [q vs a 0] (if q (recur (next q) (+ a (first q))) a)))

(defn b-keys-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (walk-keys (keys (aget ms (bit-and i 3)))))) acc))))

(defn b-vals-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (walk-vals (vals (aget ms (bit-and i 3)))))) acc))))

(defn b-keys-hash [n s]
  (let [ks (hash-keys) m (big-map ks s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (walk-keys (keys m)))) acc))))

(defn b-vals-hash [n s]
  (let [ks (hash-keys) m (big-map ks s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (walk-vals (vals m)))) acc))))

(defn b-lazy-walk-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (loop [q (seq (map inc v)) a 0] (if q (recur (next q) (+ a (first q))) a))))
        acc))))

(defn b-seq-walk-vec-1000 [n s]
  (let [v (vec-n 1000 s)]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (loop [q (seq v) a 0] (if q (recur (next q) (+ a (first q))) a))))
        acc))))

(defn b-reduce-range-1000 [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (reduce + s (range 1000)))) acc)))

(defn b-reduce-kv-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0]
      (if (< i n) (recur (inc i) (+ acc (reduce-kv (fn [a _ v] (+ a v)) 0 (aget ms (bit-and i 3))))) acc))))

(defn b-reduce-entries-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0]
      (if (< i n) (recur (inc i) (+ acc (reduce (fn [a [_ v]] (+ a v)) 0 (aget ms (bit-and i 3))))) acc))))

(defn b-str-nested [n s]
  (let [m {:id s :name "todo" :done false :tags [:a :b] :meta {:x 1 :y [1 2 3]}}]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (str m)))) acc))))

(defn b-str-concat [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (str "a" i "b" s)))) acc)))

(defn b-eq-map [n s]
  (let [ms (small-maps s) ns (small-maps s)]
    (loop [i 0 acc 0]
      (if (< i n) (recur (inc i) (+ acc (if (= (aget ms (bit-and i 3)) (aget ns (bit-and i 3))) 1 0))) acc))))

(defn b-eq-vec [n s]
  (let [a (vec-n 10 s) b (vec-n 10 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (if (= a b) 1 0))) acc))))

(defn b-hash-fresh-vec [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (bit-xor acc (hash [i s 3 4]))) acc)))

(defn b-hash-fresh-map [n s]
  (let [m (small-map s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (bit-xor acc (hash (assoc m :a i)))) acc))))

(defn b-proto-record [n s]
  (let [rs (array (->Rect s 2) (->Circle s) (->Rect 3 s) (->Circle 2))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (area (aget rs (bit-and i 3))))) acc))))

(defn b-record-kw [n s]
  (let [rs (array (->Rect s 2) (->Rect 1 s) (->Rect 3 s) (->Rect 2 2))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (:w (aget rs (bit-and i 3))))) acc))))

(defn b-record-dot [n s]
  (let [rs (array (->Rect s 2) (->Rect 1 s) (->Rect 3 s) (->Rect 2 2))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (.-w ^js (aget rs (bit-and i 3))))) acc))))

(defn b-record-assoc [n s]
  (let [r (->Rect s 2)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (:w (assoc r :w i)))) acc))))

(defn b-multimethod [n s]
  (let [ms (array {:type :rect :w s :h 2} {:type :circle :r s} {:type :rect :w 1 :h 1} {:type :other})]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (marea (aget ms (bit-and i 3))))) acc))))

(defn b-multi-arity [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (ma i s))) acc)))

(defn b-variadic [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (va i s 2))) acc)))

(defn b-apply [n s]
  (let [v [1 2 s]]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (apply + i v))) acc))))

(defn b-unknown-fn [n s]
  (let [fs (array inc dec identity (fn [x] (+ x s)))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc ((aget fs (bit-and i 3)) i))) acc))))

(defn b-closure [n s]
  (loop [i 0 acc 0]
    (if (< i n) (recur (inc i) (+ acc (let [f (fn [x] (+ x i))] (f s)))) acc)))

(defn b-swap-inc [n s]
  (let [a (atom s)]
    (dotimes [_ n] (swap! a inc))
    @a))

(defn b-swap-assoc [n s]
  (let [a (atom (small-map s))]
    (dotimes [i n] (swap! a assoc :c i))
    (:c @a)))

(defn b-merge [n s]
  (let [ms (small-maps s) o {:x 1 :y 2 :a 3}]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (merge (aget ms (bit-and i 3)) o)))) acc))))

(defn b-select-keys [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (select-keys (aget ms (bit-and i 3)) [:a :c :e])))) acc))))

(defn b-group-by-100 [n s]
  (let [v (vec-n 100 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (group-by (fn [x] (mod x 7)) v)))) acc))))

(defn b-sort-by-100 [n s]
  (let [v (vec-n 100 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (first (sort-by - v)))) acc))))

(defn b-transient-vec-100 [n s]
  (let [v (vec-n 100 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (persistent! (reduce conj! (transient []) v))))) acc))))

(defn b-transient-map-100 [n s]
  (let [ks (hash-keys)]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (persistent! (loop [j 0 t (transient {})] (if (< j 100) (recur (inc j) (assoc! t (aget ks j) j)) t))))))
        acc))))

(defn b-sorted-map-assoc [n s]
  (let [m (into (sorted-map) (map (fn [i] [i i])) (range 100))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (assoc m (+ 50 (mod i 100)) s)))) acc))))

(defn b-str-sorted-map [n s]
  (let [m (into (sorted-map) (map (fn [i] [i {:id i :title "t" :done false}])) (range 20))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (str m)))) acc))))

;; `into` without a transducer (light-cljs-1065): a vector of a map's keys,
;; a seq, and a set of a vector, each item conjoined onto a transient.
(defn b-into-vec-keys-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into [] (keys (aget ms (bit-and i 3))))))) acc))))

(defn b-into-set-100 [n s]
  (let [v (vec-n 100 s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into #{} v)))) acc))))

;; `pop`, `disj` and the variadic updates (light-cljs-1073). A vector of 33
;; items has one in its tail, so its pop takes the tail from the tree; the
;; other vectors' pops shorten their tails. The small sets hold s+j to s+j+6
;; for j from 0 to 3, so each holds s+3 to s+6, the items the disjs remove.
(defn b-vec-pop [n s]
  (let [vs (array (vec-n 10 s) (vec-n 33 s) (vec-n 100 s) (vec-n 3 s))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (pop (aget vs (bit-and i 3)))))) acc))))

(defn b-list-pop [n s]
  (let [ls (array (apply list (vec-n 3 s)) (apply list (vec-n 10 s)) (apply list (vec-n 2 s)) (apply list (vec-n 5 s)))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (pop (aget ls (bit-and i 3)))))) acc))))

(defn small-sets [s]
  (let [arr (array)]
    (dotimes [j 4] (.push arr (set (vec-n 7 (+ s j)))))
    arr))

(defn b-disj-set [n s]
  (let [ss (small-sets s) k (+ s 3)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (disj (aget ss (bit-and i 3)) k)))) acc))))

(defn b-disj-sorted-set [n s]
  (let [st (into (sorted-set) (vec-n 100 s))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (disj st (+ s (mod i 100)))))) acc))))

(defn b-disj-variadic-set [n s]
  (let [ss (small-sets s) k (+ s 3) k2 (+ s 6)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (disj (aget ss (bit-and i 3)) k k2)))) acc))))

(defn b-vec-assoc-variadic [n s]
  (let [v (vec-n 1024 s)]
    (loop [i 0 acc 0]
      (if (< i n) (recur (inc i) (+ acc (count (assoc v (bit-and i 1023) i (bit-and (+ i 512) 1023) s)))) acc))))

;; `into` of a small collection onto an empty one (light-cljs-1067), where
;; the work of each call (the target's transient, `persistent!` and its
;; metadata) weighs most: a vector of vectors of 2 to 8 items, and a map of
;; the 7-key maps.
(defn b-into-vec-small [n s]
  (let [vs (array (vec-n 2 s) (vec-n 4 s) (vec-n 6 s) (vec-n 8 s))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into [] (aget vs (bit-and i 3)))))) acc))))

(defn b-into-map-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into {} (aget ms (bit-and i 3)))))) acc))))

;; `into` onto a target that is no editable collection (light-cljs-1076),
;; each item conjoined through the target's `-conj`: a sorted map of the
;; 7-key maps' entries, and a list of vectors of 2 to 8 items.
(defn b-into-sorted-map-small [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into (sorted-map) (aget ms (bit-and i 3)))))) acc))))

(defn b-into-list-small [n s]
  (let [vs (array (vec-n 2 s) (vec-n 4 s) (vec-n 6 s) (vec-n 8 s))]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (count (into () (aget vs (bit-and i 3)))))) acc))))

;; Two loops with no core operation, the cost of the benches' loop itself.
(defn b-bit-and [n s]
  (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (bit-and i 3) s)) acc)))

(defn b-js-array-index [n s]
  (let [ms (array s 1 2 3)]
    (loop [i 0 acc 0] (if (< i n) (recur (inc i) (+ acc (aget ms (bit-and i 3)))) acc))))

;; ---------------------------------------------------------------- harness

;; A process runs the selected benches in this order, so the engine's type
;; feedback from earlier benches is part of a later one's number: compare runs
;; of the same selection.
(def benches
  [["kw-small" b-kw-small 5000000]
   ["get-small" b-get-small 5000000]
   ["get-small-miss" b-get-small-miss 5000000]
   ["contains-small" b-contains-small 5000000]
   ["count-small" b-count-small 5000000]
   ["assoc-small-existing" b-assoc-small-existing 1000000]
   ["assoc-small-new" b-assoc-small-new 1000000]
   ["dissoc-small" b-dissoc-small 1000000]
   ["update-small" b-update-small 1000000]
   ["map-literal-3" b-map-literal 2000000]
   ["get-hash100" b-get-hash 2000000]
   ["kw-hash100" b-kw-hash 2000000]
   ["assoc-hash100" b-assoc-hash 1000000]
   ["dissoc-hash100" b-dissoc-hash 1000000]
   ["update-hash100" b-update-hash 1000000]
   ["get-in-3" b-get-in 2000000]
   ["assoc-in-3" b-assoc-in 500000]
   ["update-in-3" b-update-in 500000]
   ["vec-conj-64" b-vec-conj-64 50000]
   ["vec-literal-4" b-vec-literal 2000000]
   ["vec-nth-1024" b-vec-nth 5000000]
   ["vec-invoke-1024" b-vec-invoke 5000000]
   ["vec-peek" b-vec-peek 5000000]
   ["vec-assoc-1024" b-vec-assoc 1000000]
   ["destructure-vec-3" b-destructure-vec 2000000]
   ["destructure-map-3" b-destructure-map 2000000]
   ["reduce-vec-1000" b-reduce-vec-1000 5000]
   ["reduce-map-inc-1000" b-reduce-map-inc-1000 2000]
   ["filter-count-1000" b-filter-count-1000 2000]
   ["into-xform-1000" b-into-xform-1000 2000]
   ["mapv-1000" b-mapv-1000 2000]
   ["keys-small" b-keys-small 1000000]
   ["vals-small" b-vals-small 1000000]
   ["keys-hash100" b-keys-hash 50000]
   ["vals-hash100" b-vals-hash 50000]
   ["lazy-walk-1000" b-lazy-walk-1000 2000]
   ["seq-walk-vec-1000" b-seq-walk-vec-1000 5000]
   ["reduce-range-1000" b-reduce-range-1000 5000]
   ["reduce-kv-small" b-reduce-kv-small 1000000]
   ["reduce-entries-small" b-reduce-entries-small 500000]
   ["str-nested-map" b-str-nested 100000]
   ["str-concat" b-str-concat 2000000]
   ["eq-map-7" b-eq-map 1000000]
   ["eq-vec-10" b-eq-vec 1000000]
   ["hash-fresh-vec-4" b-hash-fresh-vec 1000000]
   ["hash-fresh-map-7" b-hash-fresh-map 500000]
   ["protocol-on-record" b-proto-record 5000000]
   ["record-kw-field" b-record-kw 5000000]
   ["record-dot-field" b-record-dot 5000000]
   ["record-assoc" b-record-assoc 1000000]
   ["multimethod" b-multimethod 2000000]
   ["multi-arity-call" b-multi-arity 10000000]
   ["variadic-call" b-variadic 2000000]
   ["apply-vec" b-apply 1000000]
   ["unknown-fn-call" b-unknown-fn 5000000]
   ["closure-alloc-call" b-closure 5000000]
   ["swap-inc" b-swap-inc 2000000]
   ["swap-assoc" b-swap-assoc 1000000]
   ["merge-small" b-merge 500000]
   ["select-keys-3" b-select-keys 500000]
   ["group-by-100" b-group-by-100 10000]
   ["sort-by-100" b-sort-by-100 10000]
   ["transient-vec-100" b-transient-vec-100 20000]
   ["transient-map-100" b-transient-map-100 5000]
   ["sorted-map-assoc-100" b-sorted-map-assoc 200000]
   ["str-sorted-map-20" b-str-sorted-map 5000]
   ["into-vec-keys-small" b-into-vec-keys-small 500000]
   ["into-set-100" b-into-set-100 10000]
   ["vec-pop" b-vec-pop 2000000]
   ["list-pop" b-list-pop 5000000]
   ["disj-set-7" b-disj-set 1000000]
   ["disj-sorted-set-100" b-disj-sorted-set 200000]
   ["disj-variadic-set-7" b-disj-variadic-set 500000]
   ["vec-assoc-variadic-1024" b-vec-assoc-variadic 500000]
   ["into-vec-small" b-into-vec-small 1000000]
   ["into-map-small" b-into-map-small 500000]
   ["into-sorted-map-small" b-into-sorted-map-small 200000]
   ["into-list-small" b-into-list-small 1000000]
   ["bit-and" b-bit-and 10000000]
   ["aget-bit-and" b-js-array-index 10000000]])

(defn now [] (js/performance.now))

(defn run-one [f n s rounds]
  (f (max 1 (quot n 10)) s)
  (let [times (array)]
    (loop [r 0 sum 0]
      (if (< r rounds)
        (let [t0 (now) c (f n s) t1 (now)]
          (.push times (- t1 t0))
          (recur (inc r) (+ sum c)))
        (do (.sort times (fn [a b] (- a b)))
            (array (aget times (quot rounds 2)) sum))))))

(defn main [& args]
  (let [s (js/Number (or (first args) "7"))
        rounds (js/Number (or (second args) "5"))
        only (nth args 2 nil)
        scale (js/Number (nth args 3 "1"))]
    (doseq [[label f n] benches]
      (when (or (nil? only) (= only "all") (some #(= label %) (.split only ",")))
        (let [n (max 1 (js/Math.round (* n scale)))
              res (run-one f n s rounds)
              ms (aget res 0)]
          (js/console.log (str label "\t" (.toFixed (/ (* ms 1e6) n) 2) "\t" (aget res 1))))))))

main
