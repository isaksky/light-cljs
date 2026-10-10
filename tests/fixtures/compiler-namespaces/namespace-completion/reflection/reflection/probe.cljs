(ns reflection.probe (:require [host.bootstrap] [clojure.string] [goog.string]))

(defn run [observe]
  (assert (contains? (ns-publics 'clojure.string) 'join))
  (assert (not (contains? (ns-publics 'clojure.string) 'replace-all)))
  (assert (= (find (ns-publics 'clojure.string) 'join)
             ['join #'clojure.string/join]))
  (assert (contains? (ns-imports 'clojure.string) 'StringBuffer))
  (assert (= (find (ns-imports 'clojure.string) 'StringBuffer)
             ['StringBuffer goog.string.StringBuffer]))
  (assert (= "a:b" (clojure.string/join ":" ["a" "b"])))
  true)
run
