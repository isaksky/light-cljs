;; Copyright (c) Rich Hickey. Eclipse Public License 1.0.
;; See epl-v10.html at the repository root; retain this notice.
(ns clojure.edn-test (:require [clojure.edn :as edn] [cljs.reader :as reader] [cljs.reader :as reader-types]))
(defn- test-reader []
  (reader-types/string-push-back-reader "[1 2 3]"))
(defn run [] (do (do (do (assert (= (edn/read (test-reader))
         (reader/read (test-reader)))) (assert (= (edn/read {} (test-reader))
           (reader/read {} (test-reader)))) (assert (= (edn/read (test-reader) false "EOF" {})
           (reader/read (test-reader) false "EOF" {})))) true)))
