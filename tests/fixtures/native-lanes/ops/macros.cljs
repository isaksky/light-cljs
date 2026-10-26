(ns ops.macros)
(def state (atom [0]))
;; Every atom operation on the shared atom, with the values cljs.core gives:
;; vals pairs in [old new] order, compare-and-set! by `=` (an equal,
;; distinct vector matches), a mismatch that changes nothing, and variadic
;; swap! arguments.
(defmacro step [k]
  (let [[old new] (swap-vals! state conj k)
        hit (compare-and-set! state (vec (seq new)) (conj new :cas))
        miss (compare-and-set! state [:nope] [:never])
        r (reset! state (conj @state :r))
        [rold rnew] (reset-vals! state (conj r k))
        s (swap! state (fn [v a b] (conj v a b)) :x :y)]
    (list 'quote [(count old) (count new) hit miss (count r) (= rold r) (count rnew) (count s)])))
;; A swap whose function changes the same atom: the function runs once and
;; its result is stored, as in cljs.core.
(defmacro reentrant []
  (let [a (atom 0)]
    (list 'quote [(swap! a (fn [x] (reset! a 10) (inc x)))
                  @a
                  (swap-vals! a (fn [x] (reset! a 20) (+ x 5)))])))
