(ns siblings.macros) (def counter (atom 0)) (defmacro next-value [] (swap! counter inc))
(defn twice [x] (list '* 2 x)) (defmacro double [x] (twice x))
