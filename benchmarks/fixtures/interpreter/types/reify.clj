(defprotocol P (p [r]))
[(fn [n] (reify P (p [_] n))) p]
