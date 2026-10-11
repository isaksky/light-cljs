(def visited (atom []))
(def pipeline
  (->> (iterate inc 1)
       (map (fn [n] (swap! visited conj n) (* n n)))
       (filter odd?)
       (take 4)))
(def values (vec pipeline))
(println "Squares" (pr-str values))
[values @visited (vec pipeline)]
