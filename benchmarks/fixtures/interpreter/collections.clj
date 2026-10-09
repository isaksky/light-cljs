(let [xs (range 256)
      indexed (into {} (map (fn [x] [x (* x x)]) xs))]
  (reduce + (map (fn [x] (get indexed x)) (filter even? xs))))
