; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (partition-all 32))
 (fn [xf input] (into [] xf input))
 (fn [input] (into [] (map vec (partition-all 32 input))))
 (vec (range 1001))
 (mapv (fn [start] (vec (range start (min 1001 (+ start 32))))) (range 0 1001 32))]
