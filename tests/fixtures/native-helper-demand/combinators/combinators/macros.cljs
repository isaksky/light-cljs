(ns combinators.macros)
(def twice (comp (fn [x] (inc x)) (fn [x] (inc x))))
(def prefix (partial (fn [a b] [a b]) :p))
(def both (juxt (fn [x] (inc x)) (fn [x] (dec x)) (fn [x] (identity x))))
(def fill (fnil (fn [a b] [a b]) :a :b))
(def yes (complement (fn [x] (false? x))))
(def constant (constantly :v))
(defmacro run [x]
  [(twice x) (prefix x) (both x) (fill nil x) (yes false) (constant)])
