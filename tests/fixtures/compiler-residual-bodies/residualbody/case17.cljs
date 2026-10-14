;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case17 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (do (assert (nil? (into nil #{}))) (assert (= '(3 2 1) (into nil [1 2 3])))) (do (assert (nil? (clojure.set/union #{} nil nil)))) true)))
