; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (fn [rf] (let [n (volatile! 0)] (fn ([] (rf)) ([a] (rf a)) ([a x] (let [i @n] (vswap! n inc) (rf a (+ x i))))))))
 (fn [xf input] (into [] xf input))
 (fn [input] (into [] (map-indexed (fn [i x] (+ x i)) input)))
 (vec (range 1000))
 (vec (range 0 2000 2))]
