(ns hierarchy.first)
(defmulti choose identity)
(defmethod choose :default [_] :first)
(defmacro warm [] (choose :default) 1)
