;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case12 (:require [clojure.set]))
(defn run [] (do (def compositions   #{{:name "Art of the Fugue" :composer "J. S. Bach"}     {:name "Musical Offering" :composer "J. S. Bach"}     {:name "Requiem" :composer "Giuseppe Verdi"}     {:name "Requiem" :composer "W. A. Mozart"}}) (do (do (let [x (clojure.set/project compositions [:name]) y #{{:name "Art of the Fugue"}                                          {:name "Requiem"}                                          {:name "Musical Offering"}}] (assert (= x y))) (let [x (clojure.set/project compositions [:composer]) y #{{:composer "W. A. Mozart"}                                              {:composer "Giuseppe Verdi"}                                              {:composer "J. S. Bach"}}] (assert (= x y))) (let [x (clojure.set/project compositions [:year]) y #{{}}] (assert (= x y))) (let [x (clojure.set/project #{{}} [:name]) y #{{}}] (assert (= x y)))) true)))
