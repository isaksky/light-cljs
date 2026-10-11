;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case19 (:require [clojure.set]))
(defn run [] (do (do (do (let [super #{} sub #{}] (assert (clojure.set/superset? super sub))) (let [super #{1} sub #{}] (assert (clojure.set/superset? super sub))) (let [super #{1} sub #{1}] (assert (clojure.set/superset? super sub))) (let [super #{1 2} sub #{1 2}] (assert (clojure.set/superset? super sub))) (let [super #{1 2 42} sub #{1 2}] (assert (clojure.set/superset? super sub))) (let [super #{false} sub #{false}] (assert (clojure.set/superset? super sub))) (let [super #{nil} sub #{nil}] (assert (clojure.set/superset? super sub))) (let [super #{false nil} sub #{false}] (assert (clojure.set/superset? super sub))) (let [super #{1 2 4 nil false} sub #{1 2 nil}] (assert (clojure.set/superset? super sub)))) (do (let [notsuper #{} sub #{1}] (assert (not (clojure.set/superset? notsuper sub)))) (let [notsuper #{2} sub #{1}] (assert (not (clojure.set/superset? notsuper sub)))) (let [notsuper #{1} sub #{1 3}] (assert (not (clojure.set/superset? notsuper sub)))) (let [notsuper #{nil} sub #{false}] (assert (not (clojure.set/superset? notsuper sub)))) (let [notsuper #{false} sub #{nil}] (assert (not (clojure.set/superset? notsuper sub)))) (let [notsuper #{nil} sub #{false nil}] (assert (not (clojure.set/superset? notsuper sub)))) (let [notsuper #{nil 2 3} sub #{false nil 2 3}] (assert (not (clojure.set/superset? notsuper sub))))) true)))
