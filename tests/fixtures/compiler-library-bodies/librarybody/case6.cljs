;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case6 (:require [clojure.walk]))
(defn run [] (do (do "Checks that walk preserves the MapEntry type. See CLJS-2909." (let [coll [:html {:a ["b" 1]} ""]         f (fn [e] (if (and (vector? e) (not (map-entry? e))) (apply list e) e))]     (assert (= (list :html {:a (list "b" 1)} "") (clojure.walk/postwalk f coll)))) true)))
