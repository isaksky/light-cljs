;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualbody.case7 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (assert (nil? (cljs.reader/read-string {:readers {'foo (constantly nil)}} "#foo 1"))) true)))
