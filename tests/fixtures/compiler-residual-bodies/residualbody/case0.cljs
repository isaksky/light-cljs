;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case0 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (assert (= :anomaly (into [] (comp (filter some?) (halt-when #{:anomaly}))                     [1 2 3 :anomaly 4]))) (assert (= {:anomaly         :oh-no!,           :partial-results [1 2]}         (into []           (halt-when :anomaly #(assoc %2 :partial-results %1))           [1 2 {:anomaly :oh-no!} 3 4]))) true)))
