; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (filter (constantly false)))
 (fn [xf input] (into [] xf input))
 (fn [input] (into [] (filter (constantly false) input)))
 (vec (range 1000))
 []]
