;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case14 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (do (assert (= (transduce (halt-when #{1}) conj [] [5 4 1 2 3])            1)) (assert (= (transduce (halt-when #{1} (fn [ret input] input)) conj [] [5 4 1 2 3])            1)) (assert (= (transduce (halt-when #{1} (fn [ret input] ret)) conj [] [5 4 1 2 3])            [5 4])) (assert (= (transduce (halt-when #{1} (fn [ret input] (conj ret input))) conj [] [5 4 1 2 3])            [5 4 1])) (assert (= (into [] (halt-when #{1} (fn [ret in] (conj ret in))) [2 3 1])           [2 3 1]))) true)))
