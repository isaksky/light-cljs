;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns residualbody.case13 (:require [clojure.set] [clojure.string] [cljs.reader] [goog.object]))
(defn run [] (do (defrecord CLJS1780 [a b c]) (do (let [record (->CLJS1780 1 2 3)]     (assert (= (into #{} (sequence (map identity)                                record))            #{[:a 1] [:b 2] [:c 3]}))     (assert (= (into #{} (sequence (map identity)                                (assoc record :d 4 :e 5)) )            #{[:a 1] [:b 2] [:c 3] [:d 4] [:e 5]}))) true)))
