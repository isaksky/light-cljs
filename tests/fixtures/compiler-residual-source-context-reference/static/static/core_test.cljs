;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns static.core-test)

(defn run [] (do (do (assert (= 1 1)) true)))
