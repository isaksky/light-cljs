; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (mapcat (fn [x] [x x])))
 (fn [xf input] (transduce xf + 0 input))
 (fn [input] (reduce + 0 (mapcat (fn [x] [x x]) input)))
 (vec (range 1000))
 999000]
