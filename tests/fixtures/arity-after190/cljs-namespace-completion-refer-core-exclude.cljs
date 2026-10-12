(ns refer.exclude)
(refer-clojure :exclude '[map mapv])
(defn map [] :own-map)
(defn mapv [] :own-mapv)
(defn run [observe]
  (assert (= [:own-map :own-mapv] [(map) (mapv)]))
  true)
run
