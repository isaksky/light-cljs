(let [capture (vec (range 128))]
  (extend-type R P (p [r] (+ (:n r) (first capture)))))
