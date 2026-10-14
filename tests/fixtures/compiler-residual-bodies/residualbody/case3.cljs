;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case3 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (do (assert (nil? (cljs.reader/read-string "")))) true)))
