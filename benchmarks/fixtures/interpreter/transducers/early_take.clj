; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (comp (map inc) (take 16)))
 (fn [xf input] (into [] xf input))
 (fn [input] (into [] (take 16 (map inc input))))
 (vec (range 1000))
 (vec (range 1 17))]
