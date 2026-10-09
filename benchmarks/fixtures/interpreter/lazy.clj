(reduce + (take 128 (map (fn [x] (* x x)) (filter odd? (range)))))
