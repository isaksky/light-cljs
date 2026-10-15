(ns npm.split
  "npm.bundle as a lazy feature: with --out-dir only its chunk imports the
  packages it requires."
  (:require [light.lazy :as lazy]))

(def bundle (lazy/loadable npm.bundle/line))

(defn start [] (lazy/load bundle))

start
