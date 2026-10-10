(ns use.probe)
(use '[clojure.set :only [intersection]])
(defn run [observe]
  (assert (identical? intersection clojure.set/intersection))
  (assert (= #{:b} (intersection #{:a :b} #{:b :c})))
  true)
run
