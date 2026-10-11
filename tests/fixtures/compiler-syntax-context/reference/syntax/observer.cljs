(ns syntax.observer)
(defmacro caller [] (str (get-in &env [:ns :name])))
