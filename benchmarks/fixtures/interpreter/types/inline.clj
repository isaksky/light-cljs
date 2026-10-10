(defprotocol P (p [r]))
(defrecord R [n] P (p [_] n))
[->R p]
