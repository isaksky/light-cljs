;; Copyright (c) Rich Hickey. All rights reserved.
;; The use and distribution terms for this software are covered by the
;; Eclipse Public License 1.0 (http://opensource.org/licenses/eclipse-1.0.php)
;; which can be found in the file epl-v10.html at the root of this distribution.
;; By using this software in any fashion, you are agreeing to be bound by
;; the terms of this license.
;; You must not remove this notice, or any other, from this software.

;; Generated from a5e94c988fd0b50ffcff1ce6add12ffbb7839a3e; complete bodies, local harness.
(ns cljs.tagged-literals-test (:require [cljs.reader :as reader] [clojure.set :as set]))
(def assertions (atom 0))
(defmacro is [x] (list 'do '(swap! assertions inc) (list 'assert x)))
(defmacro deftest [name & body] (list 'defn name [] (cons 'do body)))
(reader/register-tag-parser! 'cljs/tag identity)
(reader/register-tag-parser! 'cljs/inc inc)
(reader/register-tag-parser! 'cljs/union set/union)
(deftest test-identity-custom-literal
  (is (= #cljs/tag [1 2 3] [1 2 3])))

(deftest test-runtime-reader
  (is (object? (reader/read-string "#js {}")))
  (is (= {} (reader/read-string "#cljs/tag {}")))
  (is (= (reader/read-string "#cljs/inc 0") 1))
  (is (= (reader/read-string "#cljs/union #{1}") #{1})))
(test-identity-custom-literal) (test-runtime-reader)
@assertions
