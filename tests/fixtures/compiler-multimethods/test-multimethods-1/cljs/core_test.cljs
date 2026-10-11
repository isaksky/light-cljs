;   Copyright (c) Rich Hickey. All rights reserved.
;   The use and distribution terms for this software are covered by the
;   Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;   which can be found in the file epl-v10.html at the root of this distribution.
;   By using this software in any fashion, you are agreeing to be bound by
;   the terms of this license.
;   You must not remove this notice, or any other, from this software.

;; Adapted from pinned ClojureScript core_test.cljs; EPL-1.0.
(ns cljs.core-test)

(derive ::rect ::shape)

(defmulti bar (fn [x y] [x y]))

(defmethod bar [::rect ::shape] [x y] :rect-shape)

(defmethod bar [::shape ::rect] [x y] :shape-rect)

(defn run [observe]
(do
  (do
    (assert (zero? (count (prefers bar))))
    (prefer-method bar [::rect ::shape] [::shape ::rect])
    (assert (= 1 (count (prefers bar))))
    (assert (= :rect-shape (bar ::rect ::rect)))
    (assert (= :rect-shape (apply (-get-method bar [::rect ::shape]) [::rect ::shape])))
    ))
true)

run
