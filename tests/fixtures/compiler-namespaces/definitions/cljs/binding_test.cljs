;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns cljs.binding-test
  (:require [cljs.binding-test-other-ns :as o]))

(defn test-binding []
  (assert (binding [o/*foo* 2]
        (= o/*foo* 2)))
  (assert (= o/*foo* 1)))

(defn test-with-redefs []
  (assert (with-redefs [o/bar 2]
        (= o/bar 2)))
  (assert (= o/bar 10)))

(def ^:dynamic *a* 1)
(def ^:dynamic *b* nil)

(defn test-binding-parallel []
  (assert (= 2 (binding [*a* 10
                     *b* (inc *a*)]
             *b*))))

(def a 1)
(def b nil)

(defn test-redefs-parallel []
  (assert (= 2 (with-redefs [a 10
                         b (inc a)]
             b))))

(def ^:dynamic *foo* false)
(def ^:dynamic ^boolean *foo-tagged* false)

(defn bar [] (if *foo* 1 2))
(defn bar-tagged [] (if *foo-tagged* 1 2))

(defn test-tag-inference []
  (assert (= 2 (bar)))
  (binding [*foo* "abc"]
    (assert (= 1 (bar))))
  (binding [*foo* ""]
    (assert (= 1 (bar))))

  (assert (= 2 (bar-tagged)))
  (binding [*foo-tagged* "abc"]
    (assert (= 1 (bar-tagged))))
  (binding [*foo-tagged* ""]
    (assert (= 2 (bar-tagged)))))

(defn run [observe] (test-binding) (test-with-redefs) (test-binding-parallel) (test-redefs-parallel) (test-tag-inference) true)
run
