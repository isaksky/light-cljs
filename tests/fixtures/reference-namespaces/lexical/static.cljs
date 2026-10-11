(ns lexical.static)
(def value 10)
(let* [value 11]
  (def read-local (fn* [] value)))
(def read-global (fn* [] value))
(def value 12)
