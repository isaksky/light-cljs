(defprotocol P (p [r]))
(defrecord R [n])
(extend-type R P (p [r] (:n r)))
[->R p]
