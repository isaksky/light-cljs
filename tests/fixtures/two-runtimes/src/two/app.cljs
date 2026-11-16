(ns two.app
  (:require [two.shared :as shared]))

(goog-define flavor "app")

;; A host constructor extended with a core and a program protocol.
(extend-type js/Date
  ICounted
  (-count [_] 42)
  shared/Greet
  (greet [_] flavor))

;; A load the page asks to extends this runtime's own Box: another copy of
;; this build keeps its Box's count. It also hashes a host object first, so
;; that this runtime's next identity hash is not the other copy's.
(when (shared/load-option "extendBox")
  (hash #js {})
  (extend-type shared/Box
    ICounted
    (-count [_] 99)))

(def registry (shared/register!))
