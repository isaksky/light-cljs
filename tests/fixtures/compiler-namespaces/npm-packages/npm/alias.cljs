(ns npm.alias (:require [host.packages] ["react" :as r]))

(defn run [observe]
  (assert (identical? r (aget js/lightCljsPackages "react")))
  (assert (= [:element "div"] (r/createElement "div")))
  (assert (identical? r/createElement (.-createElement r)))
  (assert (= "stub" (.-version r/default)))
  (assert (= "stub" r/default.version))
  true)
run
