;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns cljs.macro-test (:refer-clojure :exclude [==]))

(defn run [] (do (do (assert (= ":a" (first (js-keys (js-obj :a 1))))) true)))
