(defprotocol P (p [r]))
(defrecord R [n] P (p [_] n))
(defrecord S [n] P (p [_] n))
[(fn [n] (if (even? n) (->R n) (->S n))) p]
