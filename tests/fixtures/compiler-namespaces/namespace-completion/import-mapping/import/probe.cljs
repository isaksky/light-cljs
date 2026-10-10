(ns import.probe (:require-macros [namespace-observer.macros :refer [import-maps]]))
(import '[goog.math Long Integer])
(defn run [observe]
  (assert (= (:imports (import-maps)) (:requires (import-maps)) '{Long goog.math.Long Integer goog.math.Integer}))
  true)
run
