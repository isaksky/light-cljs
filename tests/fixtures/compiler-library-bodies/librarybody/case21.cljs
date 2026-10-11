;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case21 (:require [clojure.walk]))
(defn run [] (do (defrecord Rec1 [a]) (defn inc-leaf [x]   (if (number? x)     (inc x)     x)) (do (do (assert (= [2 {1 "1", :two 2}] (clojure.walk/postwalk inc-leaf [1 {0 "1", :two 1}]))) (assert (= [(Rec1. 2)] (clojure.walk/postwalk inc-leaf [(Rec1. 1)]))) (assert (= (map->Rec1 {:a 1, ":a" 1})            (clojure.walk/prewalk #(if (keyword? %) (str %) %) (Rec1. 1))) "Mirror Clojure behavior")) true)))
