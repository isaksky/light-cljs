;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualbody.case3 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (do (assert (nil? (cljs.reader/read-string "")))) true)))
