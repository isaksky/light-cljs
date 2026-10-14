;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case10 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (do (assert (= [1 2 3]            (transduce (distinct) conj [] [1 1 2 2 3 3]))) (assert (= [1 :foo 2 :foo 3]            (transduce (interpose :foo) conj [] [1 2 3]))) (assert (= [[0 1] [1 2] [2 3]]            (transduce (map-indexed (fn [i x] [i x])) conj [] [1 2 3])))) true)))
