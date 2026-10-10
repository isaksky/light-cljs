; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (map identity))
 (fn [xf input] (into [] xf input))
 (fn [input] (into [] (map identity input)))
 (vec (range 1000))
 (vec (range 1000))]
