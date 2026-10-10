(ns probe.require-multiple (:require [host.bootstrap]))
(require '[clojure.set :as set] '[clojure.string :as str])
(defn run [observe]
  (assert (= #{:a} (set/intersection #{:a} #{:a :b})))
  (assert (= "ok" (str/join ["o" "k"])))
  true)
run
