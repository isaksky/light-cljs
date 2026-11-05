(loop [i 0 v []]
  (if (< i 40)
    (recur (inc i) (conj v i))
    (reduce + v)))
