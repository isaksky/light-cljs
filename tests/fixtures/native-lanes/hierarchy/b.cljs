(ns hierarchy.b)
(defmacro check [] (derive ::child ::parent) (isa? ::child ::parent))
(def b (check))
