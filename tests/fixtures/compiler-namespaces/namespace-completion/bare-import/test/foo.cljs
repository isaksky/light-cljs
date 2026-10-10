(ns test.foo (:import goog))

(defn run [observe]
  (assert (= {} (ns-imports 'test.foo)))
  true)
run
