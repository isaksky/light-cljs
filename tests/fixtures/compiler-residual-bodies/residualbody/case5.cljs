;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case5 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (let [x '({/ 0})]     (do (assert (= x (cljs.reader/read-string (pr-str x)))) (assert (= (cljs.reader/read-string (pr-str x)) x)))) true)))
