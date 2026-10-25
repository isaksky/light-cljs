(loop [i 0 m {}]
  (if (< i 16)
    (recur (inc i) (assoc m i (* i 2)))
    (reduce + (vals m))))
