;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case18 (:require [clojure.set]))
(defn run [] (do (do (do (let [sub #{} super #{}] (assert (clojure.set/subset? sub super))) (let [sub #{} super #{1}] (assert (clojure.set/subset? sub super))) (let [sub #{1} super #{1}] (assert (clojure.set/subset? sub super))) (let [sub #{1 2} super #{1 2}] (assert (clojure.set/subset? sub super))) (let [sub #{1 2} super #{1 2 42}] (assert (clojure.set/subset? sub super))) (let [sub #{false} super #{false}] (assert (clojure.set/subset? sub super))) (let [sub #{nil} super #{nil}] (assert (clojure.set/subset? sub super))) (let [sub #{nil} super #{nil false}] (assert (clojure.set/subset? sub super))) (let [sub #{1 2 nil} super #{1 2 nil 4}] (assert (clojure.set/subset? sub super)))) (do (let [notsub #{1} super #{}] (assert (not (clojure.set/subset? notsub super)))) (let [notsub #{2} super #{1}] (assert (not (clojure.set/subset? notsub super)))) (let [notsub #{1 3} super #{1}] (assert (not (clojure.set/subset? notsub super)))) (let [notsub #{nil} super #{false}] (assert (not (clojure.set/subset? notsub super)))) (let [notsub #{false} super #{nil}] (assert (not (clojure.set/subset? notsub super)))) (let [notsub #{false nil} super #{nil}] (assert (not (clojure.set/subset? notsub super)))) (let [notsub #{1 2 nil} super #{1 2}] (assert (not (clojure.set/subset? notsub super))))) true)))
