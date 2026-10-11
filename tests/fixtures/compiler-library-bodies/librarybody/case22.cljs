;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case22 (:require [clojure.walk]))
(defn run [] (do (do (do (assert (= (-> (clojure.walk/prewalk identity [1 (with-meta [1 2] {:foo 3})])              (nth 1) meta)            {:foo 3})) (assert (= (-> (clojure.walk/postwalk identity [1 (with-meta [1 2] {:foo 3})])              (nth 1) meta)           {:foo 3}))) true)))
