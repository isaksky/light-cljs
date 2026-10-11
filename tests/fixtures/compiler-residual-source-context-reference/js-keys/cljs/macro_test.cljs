;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns cljs.macro-test (:refer-clojure :exclude [==]))

(defn run [] (do (do (assert (= ":a" (first (js-keys (js-obj :a 1))))) true)))
