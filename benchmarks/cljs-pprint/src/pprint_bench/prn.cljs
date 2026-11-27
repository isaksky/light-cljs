(ns pprint-bench.prn)

(enable-console-print!)

(defn main []
  (prn {:a 1 :b (vec (range 20)) :c #{:x :y :z} :d '(foo bar baz)}))

main
