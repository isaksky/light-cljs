;; Ordinary .cljs macro imports; original JVM phase-map contract is pinned in the reference tree.
(ns import.probe (:require [namespace-observer.macros :refer [import-maps]]))
(import '[goog.math Long Integer])
(defn run [observe]
  (assert (= (:imports (import-maps)) (:requires (import-maps)) '{Long goog.math.Long Integer goog.math.Integer}))
  true)
run
