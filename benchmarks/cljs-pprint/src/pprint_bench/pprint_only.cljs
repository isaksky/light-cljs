(ns pprint-bench.pprint-only
  (:require [cljs.pprint :refer [pprint]]))

(enable-console-print!)

(defn main []
  (pprint {:a 1 :b (vec (range 20)) :c #{:x :y :z} :d '(foo bar baz)}))

main
