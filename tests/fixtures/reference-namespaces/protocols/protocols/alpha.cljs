(ns protocols.alpha)
(defprotocol ^{:label :alpha} P "Alpha protocol" :extend-via-metadata true
  (run [this] [this x] "Run alpha"))
(defprotocol Marker)
(defn make-alpha [n]
  (reify P (run [_] n) (run [_ x] (+ n x)) Marker))
