(ns refer.rename)
(refer-clojure :exclude '[map mapv] :rename '{mapv core-mapv})
(defn map [] :own-map)
(defn mapv [] :own-mapv)
(defn run [observe]
  (assert (= [:own-map :own-mapv] [(map) (mapv)]))
  (assert (identical? core-mapv cljs.core/mapv))
  (assert (= [2 3] (core-mapv inc [1 2])))
  true)
run
