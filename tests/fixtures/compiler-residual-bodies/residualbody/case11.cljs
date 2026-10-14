;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case11 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (defn foo-1284   ([] nil)   ([x0 & xs] [x0 xs])) (do (let [xs (IndexedSeq. #js [] 0 nil)         ys (IndexedSeq. #js [1] 3 nil)]     (assert (nil? (first xs)))     (assert (nil? (seq xs)))     (assert (= (rest xs) ()))     (assert (= (pr-str xs) "()"))     (assert (= (foo-1284 0) [0 nil]))     (assert (= (pr-str (foo-1284 0)) "[0 nil]"))     (assert (zero? (count ys)))     (assert (= (transduce (map inc) conj [] ys) []))) true)))
