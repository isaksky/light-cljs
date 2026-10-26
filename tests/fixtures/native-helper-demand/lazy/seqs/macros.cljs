(ns seqs.macros)
;; Lazy helper values: the first two use lowered operations only, so their
;; unrealized sequences publish and every task realizes the same cell; the
;; others call core functions through the task's runtime object and are
;; initialized in each task.
(def pairs (for [x [1 2 3]] [x x]))
(def incs (map inc [1 2 3 4]))
(def squares (map (fn [x] (* x x)) [1 2 3 4]))
(def evens (for [x (range 10) :when (even? x)] x))
(defn tagged [xs] (for [x xs] [:item x]))
(defmacro total [] (reduce + squares))
(defmacro listing [& xs] (list 'quote (concat (take 2 evens) (tagged xs))))
(defmacro summed [& xs] `(+ ~@(map (fn [x] `(* 10 ~x)) xs) ~(count evens)))
(defmacro walked [xs]
  (let [seen (js/Array.)]
    (doseq [x xs :when (odd? x) y (take 2 squares)] (.push seen (+ x y)))
    (vec seen)))
(defmacro shared [] (list 'quote [(nth pairs 2) (reduce + incs)]))
