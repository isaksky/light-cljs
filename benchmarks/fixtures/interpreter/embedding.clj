(let [offset 11]
  (fn [n]
    (loop [i 0 sum offset]
      (if (< i n)
        (recur (inc i) (+ sum (* i i)))
        sum))))
