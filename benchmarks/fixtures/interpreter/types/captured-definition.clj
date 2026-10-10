(let [capture (vec (range 128))]
  (defrecord R [n] P (p [_] (+ n (first capture)))))
