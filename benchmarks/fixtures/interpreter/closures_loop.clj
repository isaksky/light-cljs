(let [bias 7
      score (fn [x] (+ x bias))]
  (loop [i 0 total 0]
    (if (< i 512)
      (recur (inc i) (+ total (score i)))
      total)))
