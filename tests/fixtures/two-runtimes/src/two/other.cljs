(ns two.other
  (:require [two.shared :as shared]))

;; The program protocol two.app extends js/Date with, under the same name:
;; upstream's names would make the later load's method replace the other's.
(extend-type js/Date
  shared/Greet
  (greet [_] "other"))

(def registry (shared/register!))
