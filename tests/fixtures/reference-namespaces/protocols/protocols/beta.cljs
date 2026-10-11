(ns protocols.beta (:require [protocols.alpha :as a]))
(defprotocol P (run [this]))
(defn make-beta [] (reify P (run [_] :beta) a/P (a/run [_] :alpha) (a/run [_ x] x)))
