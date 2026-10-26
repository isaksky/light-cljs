(ns demand.macros)
(defn helper [x] (list '+ x 2))
(defmacro demand [x] (helper x))
