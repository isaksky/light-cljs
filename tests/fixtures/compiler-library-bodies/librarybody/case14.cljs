;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case14 (:require [clojure.set]))
(defn run [] (do (do (do (let [x (clojure.set/rename-keys {:a "one" :b "two"} {:a :z}) y {:z "one" :b "two"}] (assert (= x y))) (let [x (clojure.set/rename-keys {:a "one" :b "two"} {:a :z :c :y}) y {:z "one" :b "two"}] (assert (= x y))) (let [x (clojure.set/rename-keys {:a "one" :b "two" :c "three"} {:a :b :b :a}) y {:a "two" :b "one" :c "three"}] (assert (= x y)))) true)))
