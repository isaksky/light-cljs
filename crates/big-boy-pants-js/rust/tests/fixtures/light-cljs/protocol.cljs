(defprotocol IWeight (weight [x]))
(deftype Box [n] IWeight (weight [this] n))
(loop [i 0 total 0]
  (if (< i 16)
    (recur (inc i) (+ total (weight (Box. i))))
    total))
