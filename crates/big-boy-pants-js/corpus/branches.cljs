(defmacro left [xs] (list 'quote (mapv inc xs)))
(defmacro right [xs] (list 'quote (mapv (fn [x] (* x x)) xs)))
(def left-result (left [1 2]))
(def right-result (right [3 4]))
;; The consumer demands both compiler-owned definitions, whose independent
;; producer expansions above are real prerequisites, not copied literals.
(defmacro consume [] (list 'quote (into (vec left-result) right-result)))
(js/console.log (pr-str [left-result right-result (consume)]))
