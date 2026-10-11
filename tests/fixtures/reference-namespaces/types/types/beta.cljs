(ns types.beta (:require [types.alpha :as a]))
(deftype UserBox [value]
  a/P
  (a/read-value [_] [:beta value])
  (write-value [_ next] next))
