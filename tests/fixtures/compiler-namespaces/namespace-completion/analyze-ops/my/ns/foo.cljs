(ns my.ns.foo
  (:require [clojure.repl] [clojure.string] [goog.string] [host.bootstrap])
  (:import [goog.string StringBuffer]))

(defn run [observe]
  (assert (identical? StringBuffer goog.string.StringBuffer))
  (assert (= "ab" (clojure.string/join ["a" "b"])))
  true)
run
