;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

(ns cljs.var-test)

(defn cljs-3411-function
  "this function adds two numbers"
  {:test #(do
            (assert (= (cljs-3411-function 2 3) 5))
            (assert (= (cljs-3411-function 4 4) 8)))}
  ([x y] (+ x y)))

(defn cljs-3411 []
  (do
    (assert (= :ok (test cljs-3411-function)))
    (assert (= :ok (test #'cljs-3411-function)))))

(defn run [observe] (cljs-3411) true)
run
