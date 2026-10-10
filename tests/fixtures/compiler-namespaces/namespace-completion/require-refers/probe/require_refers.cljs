(ns probe.require-refers)
(require '[clojure.set :as set :refer [union intersection]])
(defn run [observe]
  (assert (identical? union set/union))
  (assert (identical? intersection set/intersection))
  (assert (= #{:a :b} (union #{:a} #{:b})))
  (assert (= #{:b} (intersection #{:a :b} #{:b})))
  true)
run
