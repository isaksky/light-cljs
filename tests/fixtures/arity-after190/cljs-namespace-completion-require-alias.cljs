(ns probe.require-alias)
(require '[clojure.set :as set])
(defn run [observe]
  (assert (identical? set/intersection clojure.set/intersection))
  (assert (= #{:b} (set/intersection #{:a :b} #{:b :c})))
  true)
run
