;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case2 (:require [clojure.walk]))
(defn run [] (do (do (assert (= (clojure.walk/stringify-keys {:a 1, nil {:b 2 :c 3}, :d 4})         {"a" 1, nil {"b" 2 "c" 3}, "d" 4})) true)))
