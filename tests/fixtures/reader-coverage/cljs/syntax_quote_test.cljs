;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

;; Generated from a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e; complete bodies, local harness.
(ns cljs.syntax-quote-test (:require [cljs.test :as test]))
(def assertions (atom 0))
(defmacro is [x] (list 'do '(swap! assertions inc) (list 'assert x)))
(defmacro deftest [name & body] (list 'defn name [] (cons 'do body)))
(deftest test-syntax-quote
  (is (= 'cljs.syntax-quote-test/foo `foo))
  (is (= 'cljs.test/test-vars `test/test-vars))
  (is (= 'cljs.test/deftest `test/deftest))
  (is (= 'cljs.test/foo `test/foo)))
(test-syntax-quote)
@assertions
