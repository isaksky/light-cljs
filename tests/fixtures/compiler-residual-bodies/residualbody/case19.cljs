;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case19 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (defprotocol IFoo3125   (-mutate [this])) (defrecord Foo3125 [^:mutable x]   IFoo3125   (-mutate [this] (* 3 (set! x (inc x))))) (def ^:dynamic *test-cljs-3125* 4) (do (assert (== 12 (let [o #js {}] (* 6 (set! (.-a o) 2))))) (assert (== 12 (let [o #js {}] (* 6 (set! o -a 2))))) (assert (== 15 (* 3 (set! *test-cljs-3125* (inc *test-cljs-3125*))))) (assert (== 18 (-mutate (->Foo3125 5)))) true)))
