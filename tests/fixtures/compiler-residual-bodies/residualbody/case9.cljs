;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See research/clojurescript/epl-v10.html; retain this notice.
(ns residualbody.case9 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (do (assert (= (clojure.set/rename-keys {:a "one" :b "two"} {:a :z}) {:z "one" :b "two"})) (assert (= (clojure.set/rename-keys {:a "one" :b "two"} {:a :z :c :y}) {:z "one" :b "two"})) (assert (= (clojure.set/rename-keys {:a "one" :b "two" :c "three"} {:a :b :b :a})           {:a "two" :b "one" :c "three"}))) true)))
