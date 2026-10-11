;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case7 (:require [clojure.walk]))
(defn run [] (let [library-failures (atom []) library-check (fn [id check] (when-not check (swap! library-failures conj id)))] (do (defrecord RM [a]) (do (let [m {:foo true}]     (do (let [o '(1 2)] (library-check "assertion-001" (= m (meta (clojure.walk/postwalk identity (with-meta o m)))))) (let [o [1 2]] (library-check "assertion-002" (= m (meta (clojure.walk/postwalk identity (with-meta o m)))))) (let [o #{1 2}] (library-check "assertion-003" (= m (meta (clojure.walk/postwalk identity (with-meta o m)))))) (let [o {1 2}] (library-check "assertion-004" (= m (meta (clojure.walk/postwalk identity (with-meta o m)))))) (let [o (map inc (range 3))] (library-check "assertion-005" (= m (meta (clojure.walk/postwalk identity (with-meta o m)))))) (let [o (->RM 1)] (library-check "assertion-006" (= m (meta (clojure.walk/postwalk identity (with-meta o m)))))))) true)) (if (empty? @library-failures) true @library-failures)))
