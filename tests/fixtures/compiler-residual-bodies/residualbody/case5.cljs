;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualbody.case5 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (let [x '({/ 0})]     (do (assert (= x (cljs.reader/read-string (pr-str x)))) (assert (= (cljs.reader/read-string (pr-str x)) x)))) true)))
