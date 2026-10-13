(ns npm.names
  (:require [host.packages]
            ["@scope/pkg" :as scope]
            ["lodash.debounce" :as debounce :default debounce-fn]
            ["some-package/sub" :refer [helper]]))

(defn run [observe]
  (assert (= 7 scope/named))
  (assert (= :flushed (debounce/flush)))
  (assert (identical? debounce-fn debounce/default))
  (assert (= 42 (helper 21)))
  true)
run
