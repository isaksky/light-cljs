(ns extend-ops.main
  "Companion microbenchmark to benchmarks/core-ops with extend-type active on
  a runtime type (Atom), a host type (js/RegExp), and default — matching real
  apps (re-frame, reagent, cljs.spec) that trigger protocol hooks and extended
  dispatch paths. Both compilers must print identical checksums.")

;; ---------------------------------------------------------------- protocols & extensions

(defprotocol IProbe
  (-probe [this]))

(defprotocol IExtra
  (-extra [this x]))

(defrecord Item [a b]
  IProbe
  (-probe [_] (+ a b)))

(deftype CellBox [v]
  IDeref
  (-deref [_] v)
  IProbe
  (-probe [_] v))

(extend-type Atom
  IProbe
  (-probe [this] (let [x @this] (if (number? x) x 1))))

(extend-type js/RegExp
  IProbe
  (-probe [this] (.-length (.-source this))))

(extend-type default
  IProbe
  (-probe [_] 0)
  IExtra
  (-extra [_ x] x))

;; ---------------------------------------------------------------- data helpers

(defn small-map [s]
  {:a s :b (+ s 1) :c (+ s 2) :d (+ s 3) :e (+ s 4) :f 5 :g 6})

(defn small-maps [s]
  (let [arr (array)]
    (dotimes [j 4] (.push arr (small-map (+ s j))))
    arr))

(defn vec-n [n s]
  (into [] (map (fn [i] (+ i s))) (range n)))

;; ---------------------------------------------------------------- benches

(defn b-ext-count-vec [n s]
  (let [vs (array (vec-n 4 s) (vec-n 16 s) (vec-n 64 s) (vec-n 100 s))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (aget vs (bit-and i 3)))))
        acc))))

(defn b-ext-count-str [n s]
  (let [ss (array (str "alpha-" s) (str "beta-gamma-" s) "delta" (str "epsilon-" s "-end"))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (aget ss (bit-and i 3)))))
        acc))))

(defn b-ext-count-arr [n s]
  (let [as (array (array 1 2 s) (array 1 2 3 4 s) (array s) (array 1 2 3 4 5 6 s))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (aget as (bit-and i 3)))))
        acc))))

(defn b-ext-meta-map [n s]
  (let [ms (array (with-meta (small-map s) {:tag 1})
                  (small-map (+ s 1))
                  (with-meta (small-map (+ s 2)) {:tag 2})
                  (small-map (+ s 3)))]
    (loop [i 0 acc 0]
      (if (< i n)
        (let [m (meta (aget ms (bit-and i 3)))]
          (recur (inc i) (+ acc (if m (:tag m) 0))))
        acc))))

(defn b-ext-pred-coll [n s]
  (let [xs (array (small-map s) (vec-n 8 s) #{1 2 3 s} (list 1 2 s))]
    (loop [i 0 acc 0]
      (if (< i n)
        (let [x (aget xs (bit-and i 3))
              b (+ (if (map? x) 1 0)
                   (if (vector? x) 2 0)
                   (if (set? x) 4 0)
                   (if (seq? x) 8 0)
                   (if (coll? x) 16 0))]
          (recur (inc i) (+ acc b)))
        acc))))

(defn b-ext-pred-host [n s]
  (let [xs (array #js {:a s} "hello" (+ s 42) inc)]
    (loop [i 0 acc 0]
      (if (< i n)
        (let [x (aget xs (bit-and i 3))
              b (+ (if (map? x) 1 0)
                   (if (vector? x) 2 0)
                   (if (sequential? x) 4 0)
                   (if (coll? x) 8 0)
                   (if (fn? x) 16 0))]
          (recur (inc i) (+ acc b)))
        acc))))

(defn b-ext-pred-record [n s]
  (let [xs (array (->Item s 1) (->CellBox s) (small-map s) (->Item 2 s))]
    (loop [i 0 acc 0]
      (if (< i n)
        (let [x (aget xs (bit-and i 3))
              b (+ (if (map? x) 1 0)
                   (if (record? x) 2 0)
                   (if (satisfies? IProbe x) 4 0))]
          (recur (inc i) (+ acc b)))
        acc))))

(defn b-ext-deref-atom [n s]
  (let [as (array (atom s) (atom (+ s 1)) (atom (+ s 2)) (atom (+ s 3)))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc @(aget as (bit-and i 3))))
        acc))))

(defn b-ext-deref-deftype [n s]
  (let [bs (array (->CellBox s) (->CellBox (+ s 1)) (->CellBox (+ s 2)) (->CellBox (+ s 3)))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc @(aget bs (bit-and i 3))))
        acc))))

(defn b-ext-into-vec-keys [n s]
  (let [ms (small-maps s)]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (into [] (keys (aget ms (bit-and i 3)))))))
        acc))))

(defn b-ext-into-map-pairs [n s]
  (let [ps [[:a s] [:b (+ s 1)] [:c (+ s 2)] [:d (+ s 3)] [:e (+ s 4)] [:f 5] [:g 6]]]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (into {} ps))))
        acc))))

(defn b-ext-into-set [n s]
  (let [v (vec-n 64 s)]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (count (into #{} v))))
        acc))))

(defn b-ext-eq-mixed [n s]
  (let [a (array (small-map s) (vec-n 8 s) :alpha (+ s 10))
        b (array (small-map s) (vec-n 8 s) :alpha (+ s 10))]
    (loop [i 0 acc 0]
      (if (< i n)
        (let [k (bit-and i 3)]
          (recur (inc i) (+ acc (if (= (aget a k) (aget b k)) 1 0))))
        acc))))

(defn b-ext-hash-mixed [n s]
  (let [xs (array (small-map s) (vec-n 8 s) :alpha (str "k" s))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (bit-and (hash (aget xs (bit-and i 3))) 65535)))
        acc))))

(defn b-ext-host-seq-nth [n s]
  (let [a (array s (+ s 1) (+ s 2) (+ s 3) (+ s 4) (+ s 5) (+ s 6) (+ s 7))]
    (loop [i 0 acc 0]
      (if (< i n)
        (recur (inc i) (+ acc (first a) (nth a (bit-and i 7)) (reduce + 0 a)))
        acc))))

(defn b-ext-cons-bounded [n s]
  (let [xs (array (vec-n 8 s) (small-map s) (list 1 2 3 s) (array 1 2 3 4 s))]
    (loop [i 0 acc 0]
      (if (< i n)
        (let [x (aget xs (bit-and i 3))]
          (recur (inc i) (+ acc (bounded-count 100 x) (if (first (cons 1 x)) 1 0))))
        acc))))

;; ---------------------------------------------------------------- harness

(def benches
  [["ext-count-vec" b-ext-count-vec 2000000]
   ["ext-count-str" b-ext-count-str 2000000]
   ["ext-count-arr" b-ext-count-arr 2000000]
   ["ext-meta-map" b-ext-meta-map 2000000]
   ["ext-pred-coll" b-ext-pred-coll 1000000]
   ["ext-pred-host" b-ext-pred-host 1000000]
   ["ext-pred-record" b-ext-pred-record 1000000]
   ["ext-deref-atom" b-ext-deref-atom 2000000]
   ["ext-deref-deftype" b-ext-deref-deftype 2000000]
   ["ext-into-vec-keys" b-ext-into-vec-keys 200000]
   ["ext-into-map-pairs" b-ext-into-map-pairs 200000]
   ["ext-into-set" b-ext-into-set 10000]
   ["ext-eq-mixed" b-ext-eq-mixed 500000]
   ["ext-hash-mixed" b-ext-hash-mixed 1000000]
   ["ext-host-seq-nth" b-ext-host-seq-nth 200000]
   ["ext-cons-bounded" b-ext-cons-bounded 200000]])

(defn now [] (js/performance.now))

(defn run-one [f n s rounds]
  ;; Touch the protocol extensions once so neither compiler strips them.
  (let [_ (+ (-probe (atom s)) (-probe (js/RegExp. "abc")) (-extra nil 1))]
    (f (max 1 (quot n 10)) s)
    (let [times (array)]
      (loop [r 0 sum 0]
        (if (< r rounds)
          (let [t0 (now) c (f n s) t1 (now)]
            (.push times (- t1 t0))
            (recur (inc r) (+ sum c)))
          (do (.sort times (fn [a b] (- a b)))
              (array (aget times (quot rounds 2)) sum)))))))

(defn main [& args]
  (let [s (js/Number (or (first args) "7"))
        rounds (js/Number (or (second args) "3"))
        only (nth args 2 nil)
        scale (js/Number (nth args 3 "1"))]
    (doseq [[label f n] benches]
      (when (or (nil? only) (= only "all") (some #(= label %) (.split only ",")))
        (let [n (max 1 (js/Math.round (* n scale)))
              res (run-one f n s rounds)
              ms (aget res 0)]
          (js/console.log (str label "\t" (.toFixed (/ (* ms 1e6) n) 2) "\t" (aget res 1))))))))

main
