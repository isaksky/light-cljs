(ns npm.default (:require [host.packages] ["@scope/pkg" :default scoped :as scope]))

(defn run [observe]
  (assert (= [:scoped 1] (scoped 1)))
  (assert (identical? scoped scope/default))
  (assert (= 7 scope/named))
  true)
run
