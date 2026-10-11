;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualbody.case15 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (deftype Foo2455 []   ISequential) (do (assert (= :x (nth (eduction [:x]) 0))) (assert (try (nth (eduction [:x]) 1) false (catch js/Error residual-error (boolean (re-find #"Index out of bounds" (.-message residual-error)))))) (assert (= :x (nth (eduction [:x]) 0 :not-found))) (assert (= :not-found (nth (eduction [:x]) 1 :not-found))) (assert (try (nth (->Foo2455) 0) false (catch js/Error residual-error (boolean (re-find #".* is not ISeqable" (.-message residual-error)))))) true)))
