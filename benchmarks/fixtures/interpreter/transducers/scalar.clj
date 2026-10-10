; Factory, transducing runner, lazy runner, resident input, expected full value.
[(fn [] (comp (filter odd?) (map inc) (take 1000)))
 (fn [xf input] (transduce xf + 0 (range)))
 (fn [input] (reduce + 0 (take 1000 (map inc (filter odd? (range))))))
 nil
 1001000]
