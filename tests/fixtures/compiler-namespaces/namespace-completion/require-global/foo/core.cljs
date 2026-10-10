(ns foo.core (:require [host.bootstrap]) (:require-global [React :as react :refer [createElement]]))

(defn run [observe]
  (assert (identical? React react))
  (assert (identical? React js/React))
  (assert (identical? createElement React/createElement))
  (assert (identical? createElement react/createElement))
  (assert (= [:element "div"] (createElement "div")))
  true)
run
