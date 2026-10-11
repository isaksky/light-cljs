;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

;; Adapted from pinned ClojureScript core_test.cljs; EPL-1.0.
(ns cljs.core-test)

(def tmph (make-hierarchy))

(defmulti fooz (fn [a b] (keyword b)) :hierarchy #'tmph)

(defmethod fooz :a [a b] a)

(defmethod fooz :b [a b] b)

(prefer-method fooz :a :b)

(defn run [observe]
(do
  (do
    (assert (some? (prefer-method fooz :a :b)))))
true)

run
