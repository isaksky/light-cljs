(ns types.alpha)
(defprotocol P (read-value [this]) (write-value [this value]))
(deftype UserBox [^:mutable value]
  P
  (read-value [_] value)
  (write-value [_ next] (set! value next))
  IFn
  (-invoke [_ delta] (+ value delta)))
(def saved-constructor UserBox)
(def saved-factory ->UserBox)
(def original (UserBox. 1))
(deftype UserBox [^:mutable value]
  P
  (read-value [_] [:new value])
  (write-value [_ next] (set! value next)))
