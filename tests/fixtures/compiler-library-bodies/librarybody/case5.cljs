;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case5 (:require [clojure.walk]))
(defn run [] (do (defrecord Foo [a b c]) (defmulti get-comparator type) (defmethod get-comparator PersistentTreeMap   [o] (.-comp o)) (defmethod get-comparator PersistentTreeSet   [o] (get-comparator (.-tree-map o))) (do (do (let [colls ['(1 2 3)                  [1 2 3]                  #{1 2 3}                  (sorted-set-by > 1 2 3)                  {:a 1, :b 2, :c 3}                  (sorted-map-by > 1 10, 2 20, 3 30)                  (->Foo 1 2 3)                  (map->Foo {:a 1 :b 2 :c 3 :extra 4})]]       (doseq [c colls]         (do (let [walked (clojure.walk/walk identity identity c)]             (assert (= c walked))                          (if (map? c)               (assert (= (clojure.walk/walk #(update-in % [1] inc) #(reduce + (vals %)) c)                      (reduce + (map (comp inc val) c))))               (assert (= (clojure.walk/walk inc #(reduce + %) c)                      (reduce + (map inc c)))))             (when (or (instance? PersistentTreeMap c)                       (instance? PersistentTreeSet c))               (assert (= (get-comparator c) (get-comparator walked))))))))) true)))
