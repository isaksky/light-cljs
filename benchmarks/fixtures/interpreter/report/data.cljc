(ns report.data)

(def rows
  (mapv (fn [n] {:region (mod n 8)
                 :amount (+ 10 (mod (* n 17) 113))
                 :active (not (zero? (mod n 5)))})
        (range 256)))
