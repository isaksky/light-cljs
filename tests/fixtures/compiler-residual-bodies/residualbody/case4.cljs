;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case4 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (do (let [a (cljs.reader/read-string "{:a 1 :b 2 :c 3}")         b (cljs.reader/read-string "{:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}")]     (assert (= a {:a 1 :b 2 :c 3}))          (assert (instance? PersistentArrayMap a))     (assert (= b {:a 1 :b 2 :c 3 :d 4 :e 5 :f 6 :g 7 :h 8 :i 9}))     (assert (instance? PersistentHashMap b))) true)))
