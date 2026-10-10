; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (comp cat cat))
 (fn [xf input] (into [] xf input))
 (fn [input] (into [] (apply concat (apply concat input))))
 [[] [[0 1] [2]] [[3] [] [4 5]]]
 [0 1 2 3 4 5]]
