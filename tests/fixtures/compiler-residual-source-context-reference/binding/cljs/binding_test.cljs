;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns cljs.binding-test (:require [cljs.binding-test-other-ns :as o]))
(def ^:dynamic *a* 1)
(def ^:dynamic *b* nil)
(def a 1)
(def b nil)
(def ^:dynamic *foo* false)
(def ^:dynamic ^boolean *foo-tagged* false)
(defn bar [] (if *foo* 1 2))
(defn bar-tagged [] (if *foo-tagged* 1 2))
(defn run [] (do (do (assert (binding [o/*foo* 2]
        (= o/*foo* 2))) (assert (= o/*foo* 1)) true) (do (assert (with-redefs [o/bar 2]
        (= o/bar 2))) (assert (= o/bar 10)) true) (do (assert (= 2 (binding [*a* 10
                     *b* (inc *a*)]
             *b*))) true) (do (assert (= 2 (with-redefs [a 10
                         b (inc a)]
             b))) true)))
