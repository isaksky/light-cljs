;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case11 (:require [clojure.set]))
(defn run [] (do (do (do (let [x (clojure.set/select integer? #{}) y #{}] (assert (= x y))) (let [x (clojure.set/select integer? #{1 2}) y #{1 2}] (assert (= x y))) (let [x (clojure.set/select integer? #{1 2 :a :b :c}) y #{1 2}] (assert (= x y))) (let [x (clojure.set/select integer? #{:a :b :c}) y #{}] (assert (= x y)))) true)))
