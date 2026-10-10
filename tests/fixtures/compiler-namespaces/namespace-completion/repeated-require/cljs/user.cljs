(ns cljs.user (:require [host.bootstrap]))
(do (require '[clojure.string :as str])
    (require '[clojure.string :as str])
    (require 'clojure.string))
(defn run [observe]
  (assert (identical? str/join clojure.string/join))
  (assert (= "a:b" (str/join ":" ["a" "b"])))
  true)
run
