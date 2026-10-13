(ns npm.refer-rename
  (:require [host.packages]
            ["react" :refer [createElement useState] :rename {useState use-state}]))

(defn run [observe]
  (assert (= [:element "p"] (createElement "p")))
  (assert (= [:state 1] (use-state 1)))
  (assert (identical? use-state (.-useState (aget js/lightCljsPackages "react"))))
  true)
run
