; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (partition-all 32))
 (fn [xf input] (transduce xf (completing (fn [a group] (+ a (reduce + group)))) 0 input))
 (fn [input] (reduce + 0 (map (fn [group] (reduce + group)) (partition-all 32 input))))
 (vec (range 1001))
 500500]
