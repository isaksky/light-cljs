(ns coll.app (:require [coll.macros :refer [describe]]))
(defn a [] (describe :get 1 2))
(defn b [] (describe :put 1))
(js/console.log (pr-str (a)) (pr-str (b)))
