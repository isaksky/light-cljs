;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns librarybody.case15 (:require [clojure.set]))
(defn run [] (do (do (do (let [x (clojure.set/index  #{{:c 2} {:b 1} {:a 1 :b 2}} [:b]) y {{:b 2} #{{:a 1 :b 2}}, {:b 1} #{{:b 1}} {} #{{:c 2}}}] (assert (= x y)))) true)))
