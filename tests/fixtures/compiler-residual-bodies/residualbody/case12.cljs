;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualbody.case12 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (assert (= [""] (clojure.string/split "" #"\n"))) (assert (= [] (clojure.string/split "\n\n\n" #"\n"))) (assert (= [""] (clojure.string/split-lines ""))) (assert (= [] (clojure.string/split-lines "\n\n\n"))) true)))
